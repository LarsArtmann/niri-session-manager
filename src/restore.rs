//! Idempotent restore: boot gating, spawn planning, spawning with per-app
//! serialization, placement, focus, and retry backoff.

use anyhow::{Context, Result};
use niri_ipc::{Action, Request, Response, Workspace, WorkspaceReferenceArg};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::spawn;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::config::{AppConfig, Config, TerminalStateConfig};
use crate::ipc::{get_niri_windows, get_niri_workspaces, niri_send};
use crate::session::{
    atomic_write, load_session_windows, save_session_with_terminal_state, SavedWindow,
    WorkspaceInfo,
};
use crate::terminal::build_spawn_command;
pub const MAX_SPAWN_CONCURRENCY: usize = 5;

pub const SAME_APP_RESTORE_WARN_THRESHOLD: usize = 10;
pub fn get_boot_id() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn get_restore_marker_path(session_file: &Path) -> PathBuf {
    session_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("restore-marker")
}

/// Decides whether the boot restore should run.
///
/// Without a readable `boot_id` we can never prove a restore already
/// happened, so we always restore. A marker from a *previous* boot is stale
/// and gets pruned so it cannot accumulate forever. A marker from *this*
/// boot whose session file has vanished is also stale — the restore that
/// wrote it can no longer be reused (a fresh restore just seeds a new
/// session from the current state), so the marker is pruned and we restore.
pub fn should_restore_on_boot(
    boot_id: Option<&str>,
    marker_path: &Path,
    session_file: &Path,
) -> bool {
    let Some(id) = boot_id else {
        return true;
    };
    match fs::read_to_string(marker_path) {
        Ok(contents) if contents.trim() == id => {
            if session_file.exists() {
                return false;
            }
            if let Err(e) = fs::remove_file(marker_path) {
                warn!(
                    "Failed to prune restore marker whose session file vanished {}: {e}",
                    marker_path.display()
                );
            } else {
                info!("Session file vanished; pruned this boot's restore marker so restore can re-run");
            }
            true
        }
        Ok(_) => {
            if let Err(e) = fs::remove_file(marker_path) {
                warn!(
                    "Failed to prune stale restore marker {}: {e}",
                    marker_path.display()
                );
            } else {
                info!("Pruned stale restore marker from a previous boot");
            }
            true
        }
        Err(_) => true,
    }
}

/// What a restore pass actually decided to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// No usable session data existed; a fresh session file was created from
    /// the current niri state. In dry-run mode nothing was written.
    SeededNewSession,
    /// The session file existed but held no restorable windows.
    NothingToRestore,
    /// Dry run: no windows were spawned and no files were written.
    WouldRestore { window_count: usize },
    /// Restore ran for real; the count is how many spawned windows were
    /// confirmed visible in niri within their spawn timeout.
    Restored { spawned: usize },
}

impl fmt::Display for RestoreOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SeededNewSession => {
                write!(f, "Seeded a new session file from the current state")
            }
            Self::NothingToRestore => write!(f, "Session file held nothing to restore"),
            Self::WouldRestore { window_count } => {
                write!(f, "DRY RUN: would restore {window_count} window(s)")
            }
            Self::Restored { spawned } => write!(f, "Restored {spawned} window(s)"),
        }
    }
}

/// Upper bound for the exponential backoff between restore retry attempts.
pub const RETRY_DELAY_MAX: Duration = Duration::from_secs(30);

/// Doubles the retry delay after each failed attempt so a persistently
/// failing restore backs off instead of hammering niri at a fixed interval.
/// `--retry-delay` is the base (waited after the first failure); a base of 0
/// retries immediately, as that configuration always has.
pub fn next_retry_delay(base_secs: u64, failed_attempts: u32) -> Duration {
    let factor = 2u32.saturating_pow(failed_attempts.saturating_sub(1));
    Duration::from_secs(base_secs)
        .saturating_mul(factor)
        .min(RETRY_DELAY_MAX)
}
pub async fn restore_session(
    file_path: &Path,
    config: &Config,
    app_config: &AppConfig,
) -> Result<RestoreOutcome> {
    let attempts = config.retry_attempts.max(1);
    let mut last_error: Option<anyhow::Error> = None;
    for attempt in 1..=attempts {
        match restore_session_internal(file_path, config, app_config).await {
            Ok(outcome) => return Ok(outcome),
            Err(e) => {
                if attempt < attempts {
                    let delay = next_retry_delay(config.retry_delay, attempt);
                    warn!(
                        "Attempt {} failed: {e}. Retrying in {}s...",
                        attempt,
                        delay.as_secs()
                    );
                    sleep(delay).await;
                }
                last_error = Some(e);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("restore failed without a specific error")))
}
/// Applies the restore-time filters and caps, with the same warnings as
/// before: drop terminal windows without captured state, warn about
/// suspicious per-app counts, cap at `--max-restore-windows`.
pub fn prepare_saved_windows(
    mut windows: Vec<SavedWindow>,
    config: &Config,
    terminal_cfg: &TerminalStateConfig,
) -> Vec<SavedWindow> {
    windows.sort_by_key(|w| w.workspace.idx.unwrap_or(0));

    let before_terminal_filter = windows.len();
    windows.retain(|w| {
        let is_terminal = terminal_cfg.enabled && terminal_cfg.terminal_app_ids.contains(&w.app_id);
        !(is_terminal && w.terminal_state.is_none())
    });
    if windows.len() < before_terminal_filter {
        warn!(
            "Dropped {} terminal window(s) without captured state: restoring them would spawn empty shells",
            before_terminal_filter.saturating_sub(windows.len())
        );
    }

    let mut per_app: HashMap<&str, usize> = HashMap::new();
    for w in &windows {
        per_app
            .entry(w.app_id.as_str())
            .and_modify(|c| *c = c.saturating_add(1))
            .or_insert(1);
    }
    for (app, count) in per_app
        .iter()
        .filter(|(_, c)| **c > SAME_APP_RESTORE_WARN_THRESHOLD)
    {
        warn!(
            "Session file holds {} windows for app '{}' (threshold {}): possible single-instance save leak or poisoned session",
            count, app, SAME_APP_RESTORE_WARN_THRESHOLD
        );
    }

    if windows.len() > config.max_restore_windows {
        warn!(
            "Session holds {} windows; capping restore to {} (--max-restore-windows)",
            windows.len(),
            config.max_restore_windows
        );
        windows.truncate(config.max_restore_windows);
    }

    windows
}
/// A window currently running in niri, joined with its workspace so saved
/// windows can be matched against it (idempotent restore).
#[derive(Debug, Clone)]
pub struct RunningWindow {
    pub id: u64,
    pub app_id: Option<String>,
    pub workspace_name: Option<String>,
    pub workspace_idx: Option<u8>,
}

pub async fn snapshot_running_windows() -> Result<Vec<RunningWindow>> {
    let windows = get_niri_windows().await?;
    let workspaces = get_niri_workspaces().await?;
    Ok(windows
        .iter()
        .map(|w| {
            let ws = workspaces.iter().find(|s| w.workspace_id == Some(s.id));
            RunningWindow {
                id: w.id,
                app_id: w.app_id.clone(),
                workspace_name: ws.and_then(|s| s.name.clone()),
                workspace_idx: ws.map(|s| s.idx),
            }
        })
        .collect())
}

/// Whether a running window sits on the workspace a saved window was saved
/// on. Names are matched first (stable across reorders); index is the
/// fallback.
pub fn workspace_matches(saved: &WorkspaceInfo, running: &RunningWindow) -> bool {
    if let Some(name) = saved.name.as_deref().filter(|n| !n.is_empty()) {
        return running.workspace_name.as_deref() == Some(name);
    }
    match (saved.idx, running.workspace_idx) {
        (Some(saved_idx), Some(running_idx)) => saved_idx == running_idx,
        _ => false,
    }
}

/// Decides which saved windows need spawning.
///
/// Idempotent-restore semantics (ROADMAP Q2, resolved): per app, match saved
/// entries to already-running windows **by workspace first**; then cap the
/// spawn list at `saved − running` so re-running a restore never spawns more
/// than the deficit. Single-instance apps keep their stronger rule: skipped
/// entirely when any instance is already running.
pub fn plan_spawns(
    saved: &[SavedWindow],
    running: &[RunningWindow],
    app_config: &AppConfig,
) -> Vec<SavedWindow> {
    let mut running_by_app: HashMap<&str, Vec<&RunningWindow>> = HashMap::new();
    for w in running {
        if let Some(app) = w.app_id.as_deref() {
            running_by_app.entry(app).or_default().push(w);
        }
    }

    let mut saved_count: HashMap<&str, usize> = HashMap::new();
    for w in saved {
        saved_count
            .entry(w.app_id.as_str())
            .and_modify(|c| *c = c.saturating_add(1))
            .or_insert(1);
    }

    let mut matched_ids: HashSet<u64> = HashSet::new();
    let mut spawned_count: HashMap<&str, usize> = HashMap::new();
    let mut single_instance_spawned: HashSet<&str> = HashSet::new();
    let mut to_spawn = Vec::new();

    for window in saved {
        let app_id = window.app_id.as_str();

        if app_config.skip_apps.apps.iter().any(|s| s == app_id) {
            info!("Skipping app: {app_id}");
            continue;
        }

        let running_of_app: Vec<&RunningWindow> =
            running_by_app.get(app_id).map_or(Vec::new(), Vec::clone);

        if app_config.single_instance.apps.iter().any(|s| s == app_id) {
            if !running_of_app.is_empty() || single_instance_spawned.contains(app_id) {
                info!("Skipping single-instance app: {app_id}");
                continue;
            }
            single_instance_spawned.insert(app_id);
            to_spawn.push(window.clone());
            continue;
        }

        // Workspace-first match: an already-running window on the saved
        // workspace satisfies this saved entry without spawning.
        let match_hit = running_of_app
            .iter()
            .find(|r| !matched_ids.contains(&r.id) && workspace_matches(&window.workspace, r));
        if let Some(matched) = match_hit {
            matched_ids.insert(matched.id);
            continue;
        }

        // Count-based cap: never spawn more than the per-app deficit.
        let saved_total = saved_count.get(app_id).copied().unwrap_or(0);
        let running_total = running_of_app.len();
        let spawned_so_far = spawned_count.get(app_id).copied().unwrap_or(0);
        let deficit = saved_total
            .saturating_sub(running_total)
            .saturating_sub(spawned_so_far);
        if deficit == 0 {
            continue;
        }

        spawned_count.insert(app_id, spawned_so_far.saturating_add(1));
        to_spawn.push(window.clone());
    }

    to_spawn
}

/// Caps concurrent window spawns globally (niri IPC rate limit) and
/// serializes spawns of the same app so two instances of one app cannot
/// claim each other's new windows and land on swapped workspaces.
#[derive(Clone)]
pub struct SpawnLimiter {
    pub global: Arc<Semaphore>,
    pub per_app: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl SpawnLimiter {
    pub fn new(max_global_concurrency: usize) -> Self {
        Self {
            global: Arc::new(Semaphore::new(max_global_concurrency)),
            per_app: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn acquire(
        &self,
        app_id: &str,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit)> {
        let app_semaphore = {
            let mut per_app = self
                .per_app
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Arc::clone(
                per_app
                    .entry(app_id.to_string())
                    .or_insert_with(|| Arc::new(Semaphore::new(1))),
            )
        };
        let app_permit = app_semaphore
            .acquire_owned()
            .await
            .map_err(|_| anyhow::anyhow!("per-app spawn semaphore for '{app_id}' closed"))?;
        let global_permit = Arc::clone(&self.global)
            .acquire_owned()
            .await
            .map_err(|_| anyhow::anyhow!("global spawn semaphore closed"))?;
        Ok((app_permit, global_permit))
    }
}
pub async fn restore_session_internal(
    file_path: &Path,
    config: &Config,
    app_config: &AppConfig,
) -> Result<RestoreOutcome> {
    let Some(saved_windows) = load_session_windows(file_path)? else {
        if config.dry_run {
            info!(
                "DRY RUN: no usable session at {} — a real run would build a new session file",
                file_path.display()
            );
            return Ok(RestoreOutcome::SeededNewSession);
        }
        info!("Building new session file");
        save_session_with_terminal_state(file_path, app_config).await?;
        return Ok(RestoreOutcome::SeededNewSession);
    };

    let prepared = prepare_saved_windows(saved_windows, config, &app_config.terminal_state);
    if prepared.is_empty() {
        return Ok(RestoreOutcome::NothingToRestore);
    }

    if config.dry_run {
        info!("DRY RUN: {} windows would be restored:", prepared.len());
        for w in &prepared {
            let ws_name = w.workspace.name.clone().unwrap_or_else(|| {
                w.workspace
                    .idx
                    .map_or_else(|| "?".to_string(), |i| i.to_string())
            });
            let cmd = build_spawn_command(&w.app_id, w, &app_config.app_mappings);
            info!("  {} -> workspace [{}]: {:?}", w.app_id, ws_name, cmd);
        }
        return Ok(RestoreOutcome::WouldRestore {
            window_count: prepared.len(),
        });
    }

    let running = snapshot_running_windows().await?;
    let to_spawn = plan_spawns(&prepared, &running, app_config);
    if to_spawn.len() < prepared.len() {
        info!(
            "{} window(s) already running — idempotent restore spawns only the remaining {}",
            prepared.len().saturating_sub(to_spawn.len()),
            to_spawn.len()
        );
    }
    if to_spawn.is_empty() {
        return Ok(RestoreOutcome::Restored { spawned: 0 });
    }

    let spawned = spawn_windows(to_spawn, &running, config, app_config).await?;
    Ok(RestoreOutcome::Restored { spawned })
}

/// Spawns the planned windows (concurrency-limited) and returns how many
/// were confirmed visible in niri.
pub async fn spawn_windows(
    to_spawn: Vec<SavedWindow>,
    running: &[RunningWindow],
    config: &Config,
    app_config: &AppConfig,
) -> Result<usize> {
    let claimed: Arc<Mutex<HashSet<u64>>> =
        Arc::new(Mutex::new(running.iter().map(|w| w.id).collect()));
    let workspaces = get_niri_workspaces().await?;
    let limiter = SpawnLimiter::new(MAX_SPAWN_CONCURRENCY);

    let mut handles: Vec<JoinHandle<Result<usize>>> = Vec::new();
    for saved_window in to_spawn {
        let command = build_spawn_command(
            &saved_window.app_id,
            &saved_window,
            &app_config.app_mappings,
        );
        let limiter = limiter.clone();
        let claimed = Arc::clone(&claimed);
        let workspaces = workspaces.clone();
        let spawn_timeout = config.spawn_timeout;
        handles.push(spawn(async move {
            let _permits = limiter.acquire(&saved_window.app_id).await?;
            spawn_single_window(
                &saved_window,
                &command,
                spawn_timeout,
                &claimed,
                &workspaces,
            )
            .await
        }));
    }

    let mut spawned = 0usize;
    for handle in handles {
        let confirmed = handle
            .await
            .context("Window spawn task panicked")?
            .unwrap_or_else(|e| {
                warn!("Window spawn failed: {e}");
                0
            });
        spawned = spawned.saturating_add(confirmed);
    }
    Ok(spawned)
}

/// Spawns one window and waits for it to appear, then applies placement and
/// focus. Returns 1 if the window was confirmed visible, 0 otherwise.
pub async fn spawn_single_window(
    saved_window: &SavedWindow,
    command: &[String],
    spawn_timeout: u64,
    claimed: &Mutex<HashSet<u64>>,
    workspaces: &[Workspace],
) -> Result<usize> {
    let response = niri_send(Request::Action(Action::Spawn {
        command: command.to_vec(),
    }))
    .await?;

    if !matches!(response, Response::Handled) {
        warn!(
            "Failed to spawn app: {} using command: {:?}",
            saved_window.app_id, command
        );
        return Ok(0);
    }

    let Some(win_id) = wait_for_new_window(saved_window, spawn_timeout, claimed).await else {
        warn!(
            "Window for app {} did not appear within {}s (spawn timeout)",
            saved_window.app_id, spawn_timeout
        );
        return Ok(0);
    };

    apply_window_placement(win_id, saved_window, workspaces).await;

    if saved_window.is_focused {
        focus_window(win_id, &saved_window.app_id).await;
    }

    Ok(1)
}

/// Polls niri for a newly-opened window of the saved app that no other spawn
/// task has claimed yet. `None` when nothing appeared within the timeout.
pub async fn wait_for_new_window(
    saved_window: &SavedWindow,
    spawn_timeout: u64,
    claimed: &Mutex<HashSet<u64>>,
) -> Option<u64> {
    let polls = spawn_timeout.saturating_mul(2);
    for _ in 0..polls {
        sleep(Duration::from_millis(500)).await;
        let new_windows = get_niri_windows().await.ok()?;
        let win_id = {
            let mut claimed = claimed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            new_windows
                .iter()
                .find(|w| w.app_id == Some(saved_window.app_id.clone()) && !claimed.contains(&w.id))
                .map(|w| {
                    claimed.insert(w.id);
                    w.id
                })
        };
        if let Some(win_id) = win_id {
            return Some(win_id);
        }
    }
    None
}

/// Picks the niri workspace reference to move a restored window to.
///
/// Names win over indices (they survive workspace reordering). Index 0 is
/// treated as "no workspace": niri workspaces are 1-based, and legacy files
/// saved `idx: 0` for unknown workspaces — moving to index 0 would fail on
/// every spawn, so we leave the window on the active workspace instead.
pub fn workspace_reference(workspace: &WorkspaceInfo) -> Option<WorkspaceReferenceArg> {
    workspace
        .name
        .as_ref()
        .filter(|n| !n.is_empty())
        .cloned()
        .map(WorkspaceReferenceArg::Name)
        .or_else(|| {
            workspace
                .idx
                .filter(|i| *i > 0)
                .map(WorkspaceReferenceArg::Index)
        })
}

/// Best-effort placement: pin to the saved output if it still exists (with a
/// workspace-based fallback), move to the saved workspace, never steal focus
/// with the move itself.
pub async fn apply_window_placement(
    win_id: u64,
    saved_window: &SavedWindow,
    workspaces: &[Workspace],
) {
    if let Some(output) = resolve_target_output(&saved_window.workspace, workspaces) {
        if let Err(e) = niri_send(Request::Action(Action::MoveWindowToMonitor {
            id: Some(win_id),
            output,
        }))
        .await
        {
            warn!("Warning: failed to move window {win_id} to monitor: {e:?}");
        }
    }

    match workspace_reference(&saved_window.workspace) {
        Some(reference) => {
            if let Err(e) = niri_send(Request::Action(Action::MoveWindowToWorkspace {
                window_id: Some(win_id),
                reference,
                focus: false,
            }))
            .await
            {
                warn!("Warning: failed to move window {win_id} to workspace: {e:?}");
            }
        }
        None => {
            info!("Window {win_id} has no saved workspace; leaving it on the active workspace");
        }
    }
}

/// Picks the output to pin a window to: the saved output if it still exists,
/// otherwise the output that currently hosts a workspace with the saved name
/// (or index). Monitors get renamed or reordered between boots; the saved
/// workspace survives on *some* output. (True position/EDID matching is not
/// possible today: niri's IPC does not expose output positions.)
pub fn resolve_target_output(saved: &WorkspaceInfo, workspaces: &[Workspace]) -> Option<String> {
    let saved_output = saved.output.as_deref().filter(|o| !o.is_empty());
    if let Some(out) = saved_output {
        let output_exists = workspaces.iter().any(|w| w.output.as_deref() == Some(out));
        if output_exists {
            return Some(out.to_string());
        }
        warn!(
            "Saved output '{out}' no longer exists; falling back to the output hosting the saved workspace"
        );
    }

    let by_name = saved
        .name
        .as_deref()
        .filter(|n| !n.is_empty())
        .and_then(|n| workspaces.iter().find(|w| w.name.as_deref() == Some(n)));
    let by_idx = saved
        .idx
        .and_then(|i| workspaces.iter().find(|w| w.idx == i));
    by_name.or(by_idx).and_then(|w| w.output.clone())
}

/// Restores focus to the saved focused window, best-effort.
pub async fn focus_window(win_id: u64, app_id: &str) {
    match niri_send(Request::Action(Action::FocusWindow { id: win_id })).await {
        Ok(_) => info!("Restored focus to window {win_id} of app {app_id}"),
        Err(e) => warn!("Warning: failed to focus window {win_id}: {e}"),
    }
}
/// One-shot boot restore behind the boot-scoped marker gate. Writes the
/// marker only after a successful non-dry-run restore.
pub async fn run_boot_restore(session_file: &Path, config: &Config, app_config: &AppConfig) {
    let boot_id = get_boot_id();
    let marker_path = get_restore_marker_path(session_file);
    if !should_restore_on_boot(boot_id.as_deref(), &marker_path, session_file) {
        info!(
            "Session already restored for this boot; skipping restore (marker: {})",
            marker_path.display()
        );
        return;
    }
    info!("Restoring previous session");
    match restore_session(session_file, config, app_config).await {
        Ok(outcome) => {
            info!("{outcome}");
            if config.dry_run {
                info!("DRY RUN: restore marker not written — a real run would restore again");
            } else if let Some(id) = &boot_id {
                if let Err(e) = atomic_write(&marker_path, id) {
                    warn!("Failed to write restore marker: {e}");
                }
            }
        }
        Err(e) => warn!(
            "Session restore failed (a real restore will be attempted again on next service start): {e}"
        ),
    }
}
