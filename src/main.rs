mod config;
mod ipc;
mod proc;
mod restore;
mod save;
mod session;
mod terminal;

#[cfg(test)]
mod fake_niri;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use clap::Parser;
use niri_ipc::{Reply, Request, Response};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::{
    select,
    signal::unix::{signal, SignalKind},
    spawn,
    sync::watch,
    task::spawn_blocking,
};
use tracing::{info, warn};

use crate::config::{load_app_config, AppConfig, Config, RunMode};
use crate::ipc::{niri_send, open_niri_socket, request_reply};
use crate::restore::{
    get_boot_id, get_restore_marker_path, run_boot_restore, should_restore_on_boot,
};
use crate::save::{reactive_save_session, shutdown_with_final_save};
use crate::session::{
    get_session_file_path, load_session_windows, run_export, run_import, save_session_with_backup,
};

async fn handle_shutdown_signals() -> Result<()> {
    let mut term_signal =
        signal(SignalKind::terminate()).context("Failed to listen for SIGTERM")?;
    let mut int_signal = signal(SignalKind::interrupt()).context("Failed to listen for SIGINT")?;
    let mut quit_signal = signal(SignalKind::quit()).context("Failed to listen for SIGQUIT")?;

    select! {
        _ = term_signal.recv() => {
            info!("Received SIGTERM signal");
        },
        _ = int_signal.recv() => {
            info!("Received SIGINT signal");
        },
        _ = quit_signal.recv() => {
            info!("Received SIGQUIT signal");
        },
    }
    Ok(())
}
/// How many multiples of the fallback save interval the session file may age
/// before the health check calls it stale.
pub const SESSION_STALENESS_INTERVALS: u32 = 2;

/// Staleness decision for [`run_health_check`], extracted so tests can pin
/// the threshold: `Some(warning)` when the session file is older than
/// [`SESSION_STALENESS_INTERVALS`] × the save interval. A stale file means
/// either an idle desktop (saves are event-driven) or a save loop that has
/// silently stopped saving — the exact F1-recurrence signature the health
/// check exists to surface.
pub fn session_staleness_warning(
    age: Option<Duration>,
    save_interval_minutes: u64,
) -> Option<String> {
    let age = age?;
    let interval = save_interval_minutes.max(1);
    let threshold_minutes = interval.saturating_mul(u64::from(SESSION_STALENESS_INTERVALS));
    if age <= Duration::from_secs(threshold_minutes.saturating_mul(60)) {
        return None;
    }
    Some(format!(
        "session file is stale: last written {}m{}s ago, more than {threshold_minutes} min \
         ({} × the {interval}-min save interval). Either the desktop has been idle \
         (saves are event-driven) or the save loop is unhealthy — check recent logs \
         for event-stream deaths or unparsable-event warnings",
        age.as_secs().div_euclid(60),
        age.as_secs().rem_euclid(60),
        SESSION_STALENESS_INTERVALS,
    ))
}

/// Reports service health to the log: niri reachability (+ version),
/// boot-gate state, and the session file's contents and age. Fails when niri
/// is unreachable — everything else is informational.
async fn run_health_check(session_file: &Path, config: &Config) -> Result<()> {
    let version = match niri_send(Request::Version).await {
        Ok(Response::Version(v)) => v,
        Ok(_) => anyhow::bail!("niri replied, but not with its version"),
        Err(e) => anyhow::bail!("niri IPC unreachable: {e}"),
    };
    info!("niri IPC reachable (version {version})");

    let marker_path = get_restore_marker_path(session_file);
    let boot_id = get_boot_id();
    if should_restore_on_boot(boot_id.as_deref(), &marker_path, session_file) {
        info!("restore marker: this boot has not been restored yet (or marker is absent/stale)");
    } else {
        info!("restore marker: this boot was already restored");
    }

    if let Some(windows) = load_session_windows(session_file)? {
        let elapsed = fs::metadata(session_file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok());
        let age = elapsed.map_or_else(
            || "unknown age".to_string(),
            |d| {
                format!(
                    "{}m{}s ago",
                    d.as_secs().div_euclid(60),
                    d.as_secs().rem_euclid(60)
                )
            },
        );
        let with_layout = windows.iter().filter(|w| w.layout.is_some()).count();
        info!(
            "session file: {} window(s), {with_layout} with captured layout, last written {age}",
            windows.len()
        );
        if let Some(warning) = session_staleness_warning(elapsed, config.save_interval) {
            warn!("{warning}");
        }
    } else {
        info!("session file: none yet (a restore or save creates it)");
    }

    info!("health check passed");
    Ok(())
}
/// How many event-stream lines the protocol probe inspects before summarizing.
pub const PROBE_LINES: usize = 20;
/// How long the protocol probe waits for further burst lines before stopping
/// (a fresh subscription always gets the up-front state-sync burst first, so
/// a healthy niri delivers lines immediately).
pub const PROBE_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// What [`run_protocol_probe`] found on the live niri IPC socket.
#[derive(Debug, Default)]
pub struct ProtocolProbeReport {
    pub parsed_lines: usize,
    pub unparsable_lines: usize,
    pub unparsable_samples: Vec<String>,
}

/// Self-diagnosis for the protocol-drift failure class (F1): round-trips a
/// Version request, then reads the head of a fresh event-stream subscription
/// and reports which lines the pinned niri-ipc cannot parse. Drift is a
/// finding, not a failure — the tolerant reader keeps saving regardless — so
/// the probe only fails when niri itself is unreachable.
async fn run_protocol_probe() -> Result<ProtocolProbeReport> {
    let version = match niri_send(Request::Version).await {
        Ok(Response::Version(v)) => v,
        Ok(_) => anyhow::bail!("niri replied, but not with its version"),
        Err(e) => anyhow::bail!("niri IPC unreachable: {e}"),
    };
    info!("protocol probe: version round-trip OK (niri {version})");

    let report = spawn_blocking(probe_event_stream_head)
        .await
        .context("protocol probe task join error")??;
    let total = report.parsed_lines.saturating_add(report.unparsable_lines);
    if report.unparsable_lines == 0 {
        info!(
            "protocol probe: {total} burst line(s), all parsed by the pinned niri-ipc — no drift"
        );
    } else {
        warn!(
            "protocol probe: PROTOCOL DRIFT — {} of {total} line(s) unparsable by the pinned \
             niri-ipc; saving still works via the tolerant reader, but the pin should be bumped",
            report.unparsable_lines
        );
        for sample in &report.unparsable_samples {
            warn!("protocol probe: unparsable: {sample}");
        }
    }
    Ok(report)
}

/// Reads up to [`PROBE_LINES`] event-stream lines under
/// [`PROBE_READ_TIMEOUT`], classifying each as parsed or unparsable. A read
/// timeout or stream end just ends the head read — the burst head is what
/// the probe is after.
fn probe_event_stream_head() -> Result<ProtocolProbeReport> {
    use crate::save::{event_reader, truncate_for_log, ReadEvent};
    use std::io::BufReader;

    let (stream, _shutdown_handle) =
        open_niri_socket().context("Failed to connect to Niri IPC socket")?;
    let mut stream = BufReader::new(stream);
    match request_reply(&mut stream, &Request::EventStream)
        .context("Failed to request event stream")?
    {
        Reply::Ok(_) => {}
        Reply::Err(msg) => anyhow::bail!("Niri refused the event stream: {msg}"),
        _ => anyhow::bail!("Unexpected reply to event-stream request"),
    }
    stream
        .get_mut()
        .set_read_timeout(Some(PROBE_READ_TIMEOUT))
        .context("Failed to set the probe read timeout")?;
    let mut read_event = event_reader(stream);
    let mut report = ProtocolProbeReport::default();
    for _ in 0..PROBE_LINES {
        match read_event() {
            Ok(Some(ReadEvent::Event(_))) => {
                report.parsed_lines = report.parsed_lines.saturating_add(1);
            }
            Ok(Some(ReadEvent::Unparsed(line))) => {
                report.unparsable_lines = report.unparsable_lines.saturating_add(1);
                report.unparsable_samples.push(truncate_for_log(&line));
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::UnexpectedEof
                ) =>
            {
                break;
            }
            Err(e) => return Err(e).context("reading the event-stream head"),
        }
    }
    Ok(report)
}

/// Mode dispatch plus the long-running service loop. Split out of `main` so
/// the fake-IPC harness can drive it end-to-end with an injected shutdown
/// signal.
pub(crate) async fn run_service_loop(
    session_file: &Path,
    config: &Config,
    app_config: &AppConfig,
    shutdown_signal: impl std::future::Future<Output = Result<()>>,
) -> Result<()> {
    if let Some(dest) = &config.export_to {
        run_export(session_file, dest)?;
        return Ok(());
    }
    if let Some(source) = &config.import_from {
        run_import(source, session_file)?;
        return Ok(());
    }

    match config.run_mode() {
        RunMode::HealthCheck => {
            run_health_check(session_file, config).await?;
            return Ok(());
        }
        RunMode::ProtocolProbe => {
            run_protocol_probe().await?;
            return Ok(());
        }
        RunMode::SaveOnce => {
            info!("--save-once: saving current session, then exiting");
            save_session_with_backup(session_file, config, app_config).await?;
            return Ok(());
        }
        RunMode::SaveOnly => info!("--save-only: skipping boot restore"),
        RunMode::Normal | RunMode::RestoreOnly => {
            run_boot_restore(session_file, config, app_config).await;
        }
    }

    if config.dry_run {
        info!("Dry run complete — exiting without starting the save loop.");
        return Ok(());
    }
    if config.run_mode() == RunMode::RestoreOnly {
        info!("--restore: restore complete — exiting without starting the save loop.");
        return Ok(());
    }

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let save_task = spawn(reactive_save_session(
        session_file.to_path_buf(),
        config.clone(),
        app_config.clone(),
        shutdown_rx,
    ));

    shutdown_signal.await?;
    shutdown_with_final_save(save_task, shutdown_tx, session_file, config, app_config).await;

    info!("Shutdown complete");
    Ok(())
}
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = Config::parse();
    if let Err(e) = config.validate() {
        eprintln!("Error: {e:#}");
        #[allow(clippy::exit)]
        std::process::exit(2);
    }

    info!("Starting niri-session-manager");
    let session_file_path = match get_session_file_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Error: {e:#}");
            #[allow(clippy::exit)]
            std::process::exit(2);
        }
    };

    let app_config = match load_app_config(config.app_config_path.as_deref()) {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!("Failed to load app config, using defaults: {e}");
            AppConfig::default()
        }
    };

    let result = run_service_loop(
        &session_file_path,
        &config,
        &app_config,
        handle_shutdown_signals(),
    )
    .await;

    // Exit explicitly instead of unwinding: a blocking-pool IPC task parked on
    // a wedged niri socket would otherwise hang the process at runtime drop,
    // leaving the systemd service stuck "stopping" until the kill timeout
    // (observed live 2026-09-15).
    let code = match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e:#}");
            1
        }
    };
    #[allow(clippy::exit)]
    std::process::exit(code);
}
