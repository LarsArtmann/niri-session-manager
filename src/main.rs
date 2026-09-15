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

use anyhow::Result;
use clap::Parser;
use niri_ipc::{Request, Response};
use tokio::signal::unix::{signal, SignalKind};
use tokio::spawn;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::{load_app_config, AppConfig, Config, RunMode};
use crate::ipc::niri_send;
use crate::restore::{
    get_boot_id, get_restore_marker_path, run_boot_restore, should_restore_on_boot,
};
use crate::save::shutdown_with_final_save;
use crate::session::{
    capture_session_json, get_session_file_path, load_session_windows, save_session_with_backup,
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
/// Reports service health to the log: niri reachability (+ version),
/// boot-gate state, and the session file's contents and age. Fails when niri
/// is unreachable — everything else is informational.
async fn run_health_check(session_file: &Path) -> Result<()> {
    let version = match niri_send(Request::Version).await {
        Ok(Response::Version(v)) => v,
        Ok(_) => anyhow::bail!("niri replied, but not with its version"),
        Err(e) => anyhow::bail!("niri IPC unreachable: {e}"),
    };
    info!("niri IPC reachable (version {version})");

    let marker_path = get_restore_marker_path(session_file);
    let boot_id = get_boot_id();
    if should_restore_on_boot(boot_id.as_deref(), &marker_path) {
        info!("restore marker: this boot has not been restored yet (or marker is absent/stale)");
    } else {
        info!("restore marker: this boot was already restored");
    }

    if let Some(windows) = load_session_windows(session_file)? {
        let age = fs::metadata(session_file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .map_or_else(
                || "unknown age".to_string(),
                |d| {
                    format!(
                        "{}m{}s ago",
                        d.as_secs().div_euclid(60),
                        d.as_secs().rem_euclid(60)
                    )
                },
            );
        info!(
            "session file: {} window(s), last written {age}",
            windows.len()
        );
    } else {
        info!("session file: none yet (a restore or save creates it)");
    }

    info!("health check passed");
    Ok(())
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
            run_health_check(session_file).await?;
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
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::parse();
    config.validate()?;

    info!("Starting niri-session-manager");
    let session_file_path = get_session_file_path()?;

    let app_config = match load_app_config(config.app_config_path.as_deref()) {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!("Failed to load app config, using defaults: {e}");
            AppConfig::default()
        }
    };

    run_service_loop(
        &session_file_path,
        &config,
        &app_config,
        handle_shutdown_signals(),
    )
    .await
}
