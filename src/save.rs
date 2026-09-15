//! Reactive saving: event-driven with debounce, polling fallback with
//! capped reconnect backoff, and graceful shutdown with a final save.

use anyhow::{Context, Result};
use niri_ipc::{Event, Reply, Request, Response};
use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use tokio::select;
use tokio::sync::watch;
use tokio::task::spawn_blocking;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::config::{AppConfig, Config};
use crate::session::{capture_session_json, save_session_with_backup};

const SAVE_DEBOUNCE_SECS: u64 = 2;
/// Wait before the first reconnect attempt after a live event stream dies.
const RECONNECT_DELAY_INITIAL: Duration = Duration::from_secs(1);
/// Upper bound for the exponential reconnect backoff.
const RECONNECT_DELAY_MAX: Duration = Duration::from_secs(30);
/// How long shutdown waits for the reactive save task to stop gracefully
/// before falling back to an abort.
const SAVE_TASK_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// A stream that survived this long counts as healthy and resets the
/// reconnect backoff; quicker deaths count as niri flapping.
const RECONNECT_HEALTHY_STREAM: Duration = Duration::from_secs(5);

/// Doubles the reconnect delay, capped so a flapping niri cannot pin the save
/// loop into a hot reconnect cycle.
fn next_reconnect_delay(current: Duration) -> Duration {
    current.saturating_mul(2).min(RECONNECT_DELAY_MAX)
}

/// Resolves once a graceful shutdown has been requested (immediately if it
/// already was). A dropped sender also counts as a shutdown request.
async fn shutdown_requested(shutdown: &mut watch::Receiver<bool>) {
    if *shutdown.borrow_and_update() {
        return;
    }
    let _ = shutdown.changed().await;
}

/// Whether a niri event can change what a session save would capture.
const fn layout_relevant(event: &niri_ipc::Event) -> bool {
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

/// Long-lived save loop: subscribes to niri's event stream and saves shortly
/// after layout activity settles (debounced), instead of blind polling.
/// When the stream is unavailable or dies, falls back to saving at the
/// configured interval until niri accepts a subscription again.
async fn reactive_save_session(
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
async fn run_reactive_save_session(
    file_path: std::path::PathBuf,
    config: Config,
    app_config: AppConfig,
    fallback_interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    let debounce = Duration::from_secs(SAVE_DEBOUNCE_SECS);
    let mut reconnect_delay = RECONNECT_DELAY_INITIAL;
    info!(
        "Starting reactive save task (niri event stream, debounce {}s, fallback interval {} min)",
        SAVE_DEBOUNCE_SECS,
        config.save_interval.max(1)
    );

    'outer: loop {
        if *shutdown.borrow_and_update() {
            break;
        }
        let connection = match subscribe_event_stream().await {
            Ok(connection) => connection,
            Err(e) => {
                warn!(
                    "Niri event stream unavailable ({e}); falling back to periodic saves ({} min)",
                    config.save_interval.max(1)
                );
                loop {
                    tokio::select! {
                        () = sleep(fallback_interval) => {}
                        () = shutdown_requested(&mut shutdown) => break 'outer,
                    }
                    if *shutdown.borrow_and_update() {
                        break 'outer;
                    }
                    if let Err(save_err) =
                        save_session_with_backup(&file_path, &config, &app_config).await
                    {
                        error!("Error saving session: {}", save_err);
                    }
                    if let Ok(connection) = subscribe_event_stream().await {
                        break connection;
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
        reconnect_delay = if stream_started.elapsed() >= RECONNECT_HEALTHY_STREAM {
            RECONNECT_DELAY_INITIAL
        } else {
            next_reconnect_delay(reconnect_delay)
        };
        tokio::select! {
            () = sleep(reconnect_delay) => {}
            () = shutdown_requested(&mut shutdown) => break,
        }
    }
    info!("Reactive save task stopped");
}

/// A live niri event-stream connection: a blocking event reader plus a
/// duplicate of the socket handle, so the async side can shut the connection
/// down and unblock the reader even while niri is idle.
struct EventConnection<F> {
    read_event: F,
    socket_shutdown: UnixStream,
}

/// Opens a connection to the niri IPC socket, returning the stream and a
/// clone that can shut the connection down from another thread.
fn open_niri_socket() -> std::io::Result<(UnixStream, UnixStream)> {
    let socket_path = std::env::var_os(niri_ipc::socket::SOCKET_PATH_ENV).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "NIRI_SOCKET is not set, are you running this within niri?",
        )
    })?;
    let stream = UnixStream::connect(socket_path)?;
    let shutdown_handle = stream.try_clone()?;
    Ok((stream, shutdown_handle))
}

/// Sends one JSON-line request and reads the JSON-line reply.
fn request_reply(stream: &mut BufReader<UnixStream>, request: &Request) -> std::io::Result<Reply> {
    let mut buf = serde_json::to_string(&request).map_err(std::io::Error::other)?;
    buf.push('\n');
    stream.get_mut().write_all(buf.as_bytes())?;
    buf.clear();
    stream.read_line(&mut buf)?;
    serde_json::from_str(&buf).map_err(std::io::Error::other)
}

/// Blocking reader over an established event stream; returns an error once
/// the connection dies (or is shut down from the async side).
fn event_reader(stream: BufReader<UnixStream>) -> impl FnMut() -> std::io::Result<niri_ipc::Event> {
    let mut stream = stream;
    move || {
        let mut buf = String::new();
        stream.read_line(&mut buf)?;
        serde_json::from_str(&buf).map_err(std::io::Error::other)
    }
}

async fn subscribe_event_stream(
) -> Result<EventConnection<impl FnMut() -> std::io::Result<niri_ipc::Event> + Send + 'static>> {
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
        Ok(EventConnection {
            read_event: event_reader(stream),
            socket_shutdown,
        })
    })
    .await
    .context("Event stream task join error")?
}

/// Saves (debounced) whenever a layout-relevant event arrives; returns when
/// the event stream dies or a shutdown is requested.
async fn drive_event_driven_saves(
    connection: EventConnection<impl FnMut() -> std::io::Result<niri_ipc::Event> + Send + 'static>,
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
        while let Ok(event) = read_event() {
            if layout_relevant(&event) && tx.blocking_send(()).is_err() {
                break;
            }
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
                    None => break 'outer,
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
const FINAL_SAVE_TIMEOUT_SECS: u64 = 5;
/// Deterministic shutdown: stop the reactive save task (gracefully via the
/// shutdown signal — the save loop closes its event connection so no reader
/// thread stays blocked on a socket read — with an abort as the deadline
/// fallback), then perform one final save under a timeout so a wedged niri
/// IPC cannot hang the exit.
async fn shutdown_with_final_save(
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
