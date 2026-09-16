//! Reactive saving: event-driven with debounce, polling fallback with
//! capped reconnect backoff, and graceful shutdown with a final save.
//!
//! The niri event stream is parsed tolerantly: a line that the pinned
//! niri-ipc version cannot deserialize (niri unstable adds event variants
//! between releases — observed live 2026-09-15, when `CastsChanged` in the
//! up-front state-sync burst killed the stream ~2 ms after subscribe) is
//! logged, conservatively treated as layout-relevant, and SKIPPED — the
//! stream itself must survive protocol drift.

use anyhow::{Context, Result};
use niri_ipc::{Reply, Request, Response};
use std::io::{BufRead, BufReader};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::spawn_blocking;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::config::{AppConfig, Config};
use crate::ipc::{open_niri_socket, request_reply};
use crate::session::save_session_with_backup;

pub const SAVE_DEBOUNCE_SECS: u64 = 2;
/// Wait before the first reconnect attempt after a live event stream dies.
pub const RECONNECT_DELAY_INITIAL: Duration = Duration::from_secs(1);
/// Upper bound for the exponential reconnect backoff.
pub const RECONNECT_DELAY_MAX: Duration = Duration::from_secs(30);
/// How many consecutive event-stream subscriptions that died without living
/// [`RECONNECT_HEALTHY_STREAM`] trigger periodic saves between reconnect
/// attempts. Guards against a stream that always subscribes successfully but
/// never delivers events: without this, the polling fallback (which only
/// engages when the *subscribe* fails) would never run and no save would
/// ever happen (observed live 2026-09-15).
pub const RAPID_DEATH_FALLBACK_THRESHOLD: u32 = 3;
/// How long shutdown waits for the reactive save task to stop gracefully
/// before falling back to an abort.
pub const SAVE_TASK_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// A stream that survived this long counts as healthy and resets the
/// reconnect backoff; quicker deaths count as niri flapping.
pub const RECONNECT_HEALTHY_STREAM: Duration = Duration::from_secs(5);
/// Per event-stream connection: how many unparsable lines get their own WARN
/// log line before further logging is suppressed to a summary (a protocol
/// mismatch can make every line of a burst unparsable, and reconnect flapping
/// would otherwise repeat the whole dump).
pub const MAX_UNPARSED_LOG_PER_CONNECTION: usize = 3;
/// Length limit (chars) for logging raw unparsable event lines.
pub const UNPARSED_LOG_CHAR_LIMIT: usize = 200;
/// Emit a stream-health summary at most once per this many stream deaths, so
/// chronic flapping stays visible in the journal without per-death noise
/// (each death still logs one INFO reconnect line).
pub const FLAPPING_SUMMARY_EVERY: u32 = 10;

/// Rate-limited stream-health summary: bumps the since-summary counter on
/// every call and returns `Some(message)` only every
/// [`FLAPPING_SUMMARY_EVERY`] deaths, resetting the counter. Extracted so
/// tests can pin the cadence and reset behavior.
pub fn flapping_summary(
    deaths_since_summary: &mut u32,
    total_deaths: u32,
    rapid_death_streak: u32,
    next_delay: Duration,
) -> Option<String> {
    *deaths_since_summary = deaths_since_summary.saturating_add(1);
    if *deaths_since_summary < FLAPPING_SUMMARY_EVERY {
        return None;
    }
    *deaths_since_summary = 0;
    Some(format!(
        "niri event stream health: {total_deaths} deaths so far, current rapid-death streak \
         {rapid_death_streak} (periodic-fallback threshold {RAPID_DEATH_FALLBACK_THRESHOLD}), \
         next reconnect in {}s",
        next_delay.as_secs_f32()
    ))
}

/// Doubles the reconnect delay, capped so a flapping niri cannot pin the save
/// loop into a hot reconnect cycle.
pub fn next_reconnect_delay(current: Duration) -> Duration {
    current.saturating_mul(2).min(RECONNECT_DELAY_MAX)
}

/// Resolves once a graceful shutdown has been requested (immediately if it
/// already was). A dropped sender also counts as a shutdown request.
pub async fn shutdown_requested(shutdown: &mut watch::Receiver<bool>) {
    if *shutdown.borrow_and_update() {
        return;
    }
    let _ = shutdown.changed().await;
}

/// Whether a niri event can change what a session save would capture.
pub const fn layout_relevant(event: &niri_ipc::Event) -> bool {
    use niri_ipc::Event;
    matches!(
        event,
        Event::WorkspacesChanged { .. }
            | Event::WorkspaceActivated { .. }
            | Event::WorkspaceActiveWindowChanged { .. }
            | Event::WindowsChanged { .. }
            | Event::WindowOpenedOrChanged { .. }
            | Event::WindowClosed { .. }
            | Event::WindowFocusChanged { .. }
            | Event::WindowLayoutsChanged { .. }
    )
}

/// One line off the niri event stream: either an event the pinned niri-ipc
/// version understands, or the raw line it refused to deserialize.
#[derive(Debug, Clone)]
pub enum ReadEvent {
    Event(niri_ipc::Event),
    Unparsed(String),
}

/// Bounded for logging: an unparsable line can be a huge `WindowsChanged`
/// dump, and the WARN log only needs enough to identify the variant.
fn truncate_for_log(line: &str) -> String {
    line.chars().take(UNPARSED_LOG_CHAR_LIMIT).collect()
}

/// Long-lived save loop: subscribes to niri's event stream and saves shortly
/// after layout activity settles (debounced), instead of blind polling.
/// When the stream is unavailable or dies, falls back to saving at the
/// configured interval until niri accepts a subscription again.
pub async fn reactive_save_session(
    file_path: std::path::PathBuf,
    config: Config,
    app_config: AppConfig,
    shutdown: watch::Receiver<bool>,
) {
    let interval = Duration::from_secs(config.save_interval.max(1).saturating_mul(60));
    run_reactive_save_session(file_path, config, app_config, interval, shutdown).await;
}

/// The save loop with an injectable fallback interval, so tests can exercise
/// the polling-fallback branch without waiting out the configured minutes.
pub async fn run_reactive_save_session(
    file_path: std::path::PathBuf,
    config: Config,
    app_config: AppConfig,
    fallback_interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    let debounce = Duration::from_secs(SAVE_DEBOUNCE_SECS);
    let mut reconnect_delay = RECONNECT_DELAY_INITIAL;
    let mut rapid_deaths: u32 = 0;
    let mut total_deaths: u32 = 0;
    let mut deaths_since_summary: u32 = 0;
    info!(
        "Starting reactive save task (niri event stream, debounce {}s, fallback interval {} min)",
        SAVE_DEBOUNCE_SECS,
        config.save_interval.max(1)
    );

    'outer: loop {
        if *shutdown.borrow_and_update() {
            break;
        }
        let connection = if rapid_deaths >= RAPID_DEATH_FALLBACK_THRESHOLD {
            warn!(
                "Niri event stream died {rapid_deaths} times without staying up {}s; \
                 mixing periodic saves ({fallback_min} min) between reconnects",
                RECONNECT_HEALTHY_STREAM.as_secs(),
                fallback_min = config.save_interval.max(1)
            );
            match periodic_fallback_until_stream(
                &file_path,
                &config,
                &app_config,
                fallback_interval,
                &mut shutdown,
            )
            .await
            {
                Some(connection) => connection,
                None => break 'outer,
            }
        } else {
            match subscribe_event_stream().await {
                Ok(connection) => connection,
                Err(e) => {
                    warn!(
                        "Niri event stream unavailable ({e}); falling back to periodic saves ({} min)",
                        config.save_interval.max(1)
                    );
                    match periodic_fallback_until_stream(
                        &file_path,
                        &config,
                        &app_config,
                        fallback_interval,
                        &mut shutdown,
                    )
                    .await
                    {
                        Some(connection) => connection,
                        None => break 'outer,
                    }
                }
            }
        };

        let stream_started = std::time::Instant::now();
        drive_event_driven_saves(
            connection,
            &file_path,
            &config,
            &app_config,
            debounce,
            shutdown.clone(),
        )
        .await;
        if *shutdown.borrow_and_update() {
            break;
        }
        info!("Niri event stream ended; reconnecting");
        total_deaths = total_deaths.saturating_add(1);
        if stream_started.elapsed() >= RECONNECT_HEALTHY_STREAM {
            reconnect_delay = RECONNECT_DELAY_INITIAL;
            rapid_deaths = 0;
        } else {
            rapid_deaths = rapid_deaths.saturating_add(1);
            reconnect_delay = next_reconnect_delay(reconnect_delay);
        }
        if let Some(summary) = flapping_summary(
            &mut deaths_since_summary,
            total_deaths,
            rapid_deaths,
            reconnect_delay,
        ) {
            warn!("{summary}");
        }
        if rapid_deaths < RAPID_DEATH_FALLBACK_THRESHOLD {
            tokio::select! {
                () = sleep(reconnect_delay) => {}
                () = shutdown_requested(&mut shutdown) => break,
            }
        }
    }
    info!("Reactive save task stopped");
}

/// Periodic-save fallback: saves at `fallback_interval` and retries the
/// subscription after each save, until niri accepts an event stream again (or
/// shutdown is requested). Returns the newly accepted connection, or `None`
/// when shutdown was requested.
async fn periodic_fallback_until_stream(
    file_path: &Path,
    config: &Config,
    app_config: &AppConfig,
    fallback_interval: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> Option<EventConnection> {
    loop {
        tokio::select! {
            () = sleep(fallback_interval) => {}
            () = shutdown_requested(shutdown) => return None,
        }
        if *shutdown.borrow_and_update() {
            return None;
        }
        if let Err(save_err) = save_session_with_backup(file_path, config, app_config).await {
            error!("Error saving session: {}", save_err);
        }
        if let Ok(connection) = subscribe_event_stream().await {
            return Some(connection);
        }
    }
}

/// A live niri event-stream connection: a blocking event reader plus a
/// duplicate of the socket handle, so the async side can shut the connection
/// down and unblock the reader even while niri is idle. The reader is boxed
/// so every producer (`subscribe_event_stream`, the fallback loop) yields the
/// same concrete type.
pub struct EventConnection {
    pub read_event: Box<dyn FnMut() -> std::io::Result<Option<ReadEvent>> + Send>,
    pub socket_shutdown: UnixStream,
}

/// Blocking reader over an established event stream. Each call yields:
/// - `Ok(Some(ReadEvent::Event(_)))` — one deserialized niri event,
/// - `Ok(Some(ReadEvent::Unparsed(_)))` — one line the pinned niri-ipc
///   version could not parse (protocol drift from a newer niri); the stream
///   itself is still alive and must keep being read,
/// - `Err(_)` — the connection died (EOF, I/O error, or shutdown from the
///   async side).
pub fn event_reader(
    stream: BufReader<UnixStream>,
) -> impl FnMut() -> std::io::Result<Option<ReadEvent>> {
    let mut stream = stream;
    move || {
        let mut buf = String::new();
        if stream.read_line(&mut buf)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "niri event stream closed",
            ));
        }
        if let Ok(event) = serde_json::from_str(&buf) {
            return Ok(Some(ReadEvent::Event(event)));
        }
        Ok(Some(ReadEvent::Unparsed(buf)))
    }
}

pub async fn subscribe_event_stream() -> Result<EventConnection> {
    spawn_blocking(move || {
        let (stream, socket_shutdown) =
            open_niri_socket().context("Failed to connect to Niri IPC socket")?;
        let mut stream = BufReader::new(stream);
        match request_reply(&mut stream, &Request::EventStream)
            .context("Failed to request event stream")?
        {
            Reply::Ok(Response::Handled) => {}
            Reply::Err(msg) => anyhow::bail!("Niri refused the event stream: {msg}"),
            _ => anyhow::bail!("Unexpected reply to event-stream request"),
        }
        // The handshake reply was read under the request/reply timeout; event
        // lines legitimately arrive minutes apart, so clear it before the
        // reader parks on the stream.
        stream
            .get_mut()
            .set_read_timeout(None)
            .context("Failed to clear the event-stream read timeout")?;
        Ok(EventConnection {
            read_event: Box::new(event_reader(stream)),
            socket_shutdown,
        })
    })
    .await
    .context("Event stream task join error")?
}

/// Saves (debounced) whenever a layout-relevant event arrives; returns when
/// the event stream dies or a shutdown is requested.
pub async fn drive_event_driven_saves(
    connection: EventConnection,
    file_path: &std::path::Path,
    config: &Config,
    app_config: &AppConfig,
    debounce: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    let EventConnection {
        read_event,
        socket_shutdown,
    } = connection;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(16);
    let reader = spawn_blocking(move || {
        let mut read_event = read_event;
        let mut logged_unparsed = 0usize;
        let mut suppressed_unparsed = 0usize;
        while let Ok(Some(outcome)) = read_event() {
            let relevant = match outcome {
                ReadEvent::Event(event) => layout_relevant(&event),
                ReadEvent::Unparsed(line) => {
                    // Unknown event variants may change the layout (this is
                    // exactly how niri unstable drifts); be conservative and
                    // save. The debounce collapses bursts, and byte-identical
                    // captures skip the write entirely.
                    if logged_unparsed < MAX_UNPARSED_LOG_PER_CONNECTION {
                        logged_unparsed = logged_unparsed.saturating_add(1);
                        warn!(
                            "Skipping unparsable niri event line (niri newer than the pinned niri-ipc?): {}",
                            truncate_for_log(&line)
                        );
                    } else {
                        suppressed_unparsed = suppressed_unparsed.saturating_add(1);
                    }
                    true
                }
            };
            if relevant && tx.blocking_send(()).is_err() {
                break;
            }
        }
        if suppressed_unparsed > 0 {
            warn!(
                "Suppressed logging of {suppressed_unparsed} further unparsable niri event lines"
            );
        }
    });

    'outer: loop {
        tokio::select! {
            () = shutdown_requested(&mut shutdown) => break,
            maybe = rx.recv() => {
                let Some(()) = maybe else { break };
            }
        }
        let settle = sleep(debounce);
        tokio::pin!(settle);
        loop {
            tokio::select! {
                () = &mut settle => break,
                () = shutdown_requested(&mut shutdown) => break 'outer,
                maybe = rx.recv() => match maybe {
                    Some(()) => {
                        let next = tokio::time::Instant::now()
                            .checked_add(debounce)
                            .unwrap_or_else(tokio::time::Instant::now);
                        settle.as_mut().reset(next);
                    }
                    // The stream died mid-debounce. Layout-relevant events
                    // WERE delivered — flush the pending save instead of
                    // dropping it, so a dying stream cannot lose the last
                    // activity (the reconnect path re-subscribes afterwards).
                    None => break,
                },
            }
        }
        if let Err(e) = save_session_with_backup(file_path, config, app_config).await {
            error!("Error saving session: {}", e);
        }
    }

    // Unblock a reader parked on a socket read (shutdown path); after a
    // natural stream death the reader has already exited on its own.
    let _ = socket_shutdown.shutdown(Shutdown::Both);
    reader.abort();
    let _ = reader.await;
}
pub const FINAL_SAVE_TIMEOUT_SECS: u64 = 5;
/// Deterministic shutdown: stop the reactive save task (gracefully via the
/// shutdown signal — the save loop closes its event connection so no reader
/// thread stays blocked on a socket read — with an abort as the deadline
/// fallback), then perform one final save under a timeout so a wedged niri
/// IPC cannot hang the exit.
pub async fn shutdown_with_final_save(
    mut save_task: JoinHandle<()>,
    shutdown_tx: watch::Sender<bool>,
    session_file: &Path,
    config: &Config,
    app_config: &AppConfig,
) {
    let _ = shutdown_tx.send(true);
    match tokio::time::timeout(SAVE_TASK_SHUTDOWN_GRACE, &mut save_task).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => warn!("Reactive save task ended abnormally during shutdown: {e}"),
        Err(_) => {
            warn!(
                "Reactive save task did not stop within {}s; aborting it",
                SAVE_TASK_SHUTDOWN_GRACE.as_secs()
            );
            save_task.abort();
            let _ = save_task.await;
        }
    }

    info!("Saving final session before shutdown");
    let final_save = save_session_with_backup(session_file, config, app_config);
    match tokio::time::timeout(Duration::from_secs(FINAL_SAVE_TIMEOUT_SECS), final_save).await {
        Ok(Ok(())) => info!("Final session saved"),
        Ok(Err(e)) => error!("Error saving final session: {}", e),
        Err(_) => warn!("Final save timed out"),
    }
}
