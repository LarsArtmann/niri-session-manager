mod proc;

use anyhow::{Context, Result, bail};
use chrono::{Local, SecondsFormat};
use clap::Parser;
use niri_ipc::{
    socket::Socket, Action, Reply, Request, Response, Window, Workspace, WorkspaceReferenceArg,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Write;
use std::time::UNIX_EPOCH;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::{
    select,
    signal::unix::{signal, SignalKind},
    spawn,
    sync::{OwnedSemaphorePermit, Semaphore},
    task::{JoinHandle, spawn_blocking},
    time::sleep,
    time::Duration,
};
use tracing::{error, info, warn};

async fn niri_send(request: Request) -> Result<Response> {
    spawn_blocking(move || {
        let mut socket = Socket::connect().context("Failed to connect to Niri IPC socket")?;
        let reply = socket
            .send(request)
            .context("Failed to communicate with Niri IPC")?;
        match reply {
            Reply::Ok(response) => Ok(response),
            Reply::Err(error_msg) => anyhow::bail!("Niri IPC returned an error: {}", error_msg),
        }
    })
    .await
    .context("Niri IPC task join error")?
}

async fn get_niri_windows() -> Result<Vec<Window>> {
    match niri_send(Request::Windows).await? {
        Response::Windows(windows) => Ok(windows),
        _ => anyhow::bail!("Expected Windows response from Niri"),
    }
}

async fn get_niri_workspaces() -> Result<Vec<Workspace>> {
    match niri_send(Request::Workspaces).await? {
        Response::Workspaces(workspaces) => Ok(workspaces),
        _ => anyhow::bail!("Expected Workspaces response from Niri"),
    }
}

fn get_session_file_path() -> Result<std::path::PathBuf> {
    let mut session_dir =
        dirs::data_dir().context("Failed to locate data directory (XDG_DATA_HOME)")?;
    session_dir.push("niri-session-manager");
    fs::create_dir_all(&session_dir).context("Failed to create session directory")?;
    Ok(session_dir.join("session.json"))
}

const MAX_RESTORE_WINDOWS_DEFAULT: usize = 100;
const SAME_APP_RESTORE_WARN_THRESHOLD: usize = 10;

fn get_boot_id() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn get_restore_marker_path(session_file: &Path) -> PathBuf {
    session_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("restore-marker")
}

/// Decides whether the boot restore should run.
///
/// Without a readable `boot_id` we can never prove a restore already
/// happened, so we always restore. A marker from a *previous* boot is stale
/// and gets pruned so it cannot accumulate forever.
fn should_restore_on_boot(boot_id: Option<&str>, marker_path: &Path) -> bool {
    let Some(id) = boot_id else {
        return true;
    };
    match fs::read_to_string(marker_path) {
        Ok(contents) if contents.trim() == id => false,
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

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct WorkspaceInfo {
    #[serde(default, alias = "workspace_idx")]
    idx: Option<u8>,
    #[serde(default, alias = "workspace_name")]
    name: Option<String>,
    #[serde(default, alias = "workspace_output")]
    output: Option<String>,
}

impl WorkspaceInfo {
    fn from_workspace(ws: Option<&Workspace>) -> Self {
        ws.map_or_else(Self::default, |w| Self {
            idx: Some(w.idx),
            name: w.name.clone(),
            output: w.output.clone(),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedWindow {
    id: u64,
    app_id: String,
    #[serde(default, flatten)]
    workspace: WorkspaceInfo,
    is_focused: bool,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    terminal_state: Option<TerminalState>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
enum ChildCommand {
    Args(Vec<String>),
    Legacy(String),
}

impl ChildCommand {
    fn to_args(&self) -> Vec<String> {
        match self {
            Self::Args(args) => args.clone(),
            Self::Legacy(s) => s.split_whitespace().map(String::from).collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TerminalState {
    child_command: Option<ChildCommand>,
    child_cwd: Option<String>,
}

const SESSION_FORMAT_VERSION: u32 = 3;
const MAX_SPAWN_CONCURRENCY: usize = 5;

#[derive(Debug, Serialize, Deserialize)]
struct VersionedSession {
    version: u32,
    windows: Vec<SavedWindow>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum SessionData {
    Versioned(VersionedSession),
    Legacy(Vec<SavedWindow>),
}

impl SessionData {
    fn into_windows(self) -> Vec<SavedWindow> {
        match self {
            Self::Versioned(v) => v.windows,
            Self::Legacy(windows) => windows,
        }
    }

    const fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy(_))
    }
}

/// What a restore pass actually decided to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestoreOutcome {
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

async fn restore_session(
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
                    warn!(
                        "Attempt {} failed: {}. Retrying in {} seconds...",
                        attempt, e, config.retry_delay
                    );
                    sleep(Duration::from_secs(config.retry_delay)).await;
                }
                last_error = Some(e);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("restore failed without a specific error")))
}

const fn default_enabled() -> bool {
    true
}
fn default_terminal_app_ids() -> Vec<String> {
    vec![
        "kitty".into(),
        "foot".into(),
        "org.wezfurlong.wezterm".into(),
        "com.mitchellh.ghostty".into(),
        "alacritty".into(),
    ]
}
fn default_shell_names() -> Vec<String> {
    vec![
        "fish".into(),
        "bash".into(),
        "zsh".into(),
        "sh".into(),
        "dash".into(),
        "-fish".into(),
        "-bash".into(),
        "-zsh".into(),
        "-sh".into(),
        "sudo".into(),
        "doas".into(),
    ]
}
fn default_helper_names() -> Vec<String> {
    vec!["kitten".into()]
}
const fn default_max_walk_depth() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TerminalStateConfig {
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default = "default_terminal_app_ids")]
    terminal_app_ids: Vec<String>,
    #[serde(default = "default_shell_names")]
    shell_names: Vec<String>,
    #[serde(default = "default_helper_names")]
    helper_names: Vec<String>,
    #[serde(default = "default_max_walk_depth")]
    max_walk_depth: u32,
}

impl Default for TerminalStateConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            terminal_app_ids: default_terminal_app_ids(),
            shell_names: default_shell_names(),
            helper_names: default_helper_names(),
            max_walk_depth: default_max_walk_depth(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SingleInstanceAppsConfig {
    #[serde(default)]
    apps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SkipAppsConfig {
    #[serde(default)]
    apps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppConfig {
    #[serde(default)]
    app_mappings: HashMap<String, Vec<String>>,
    #[serde(default, rename = "single_instance_apps")]
    single_instance: SingleInstanceAppsConfig,
    #[serde(default, rename = "skip_apps")]
    skip_apps: SkipAppsConfig,
    #[serde(default)]
    terminal_state: TerminalStateConfig,
}

fn load_app_config() -> Result<AppConfig> {
    let mut config_path = dirs::config_dir().context("Failed to locate config directory")?;
    config_path.push("niri-session-manager");
    config_path.push("config.toml");

    if !config_path.exists() {
        fs::create_dir_all(config_path.parent().unwrap())?;
        fs::write(
            &config_path,
            r#"# Niri Session Manager Configuration

# Apps that should only have one instance
[single_instance_apps] 
apps = [
    "firefox",
    "zen"
]

#Application remapping
[app_mappings]

# flatpak remapping
"vesktop" = ["flatpak", "run", "dev.vencord.Vesktop"]
"discord" = ["flatpak", "run", "com.discordapp.Discord"]
"slack" = ["flatpak", "run", "com.slack.Slack"]
"obs" = ["flatpak", "run", "com.obsproject.Studio"]

# Simple command remapping
"com.mitchellh.ghostty" = ["ghostty"]
"org.wezfurlong.wezterm" = ["wezterm"]

# Commands with arguments
"firefox-custom" = ["firefox", "--profile", "default-release"]

# Terminal state recovery — restore running commands inside terminals
[terminal_state]
enabled = true
terminal_app_ids = ["kitty", "foot", "org.wezfurlong.wezterm", "com.mitchellh.ghostty", "alacritty"]
shell_names = ["fish", "bash", "zsh", "sh", "dash", "-fish", "-bash", "-zsh", "-sh", "sudo", "doas"]
helper_names = ["kitten"]
max_walk_depth = 20
"#,
        )?;
        return Ok(AppConfig::default());
    }

    let config_str = fs::read_to_string(&config_path).context("Failed to read config file")?;

    let config: AppConfig = toml::from_str(&config_str).context("Failed to parse config file")?;
    Ok(config)
}

fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(target_os = "linux")]
fn get_shell_from_passwd() -> Option<String> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let uid = status
        .lines()
        .find(|l| l.starts_with("Uid:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u32>().ok())?;

    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.get(2).and_then(|f| f.parse::<u32>().ok()) == Some(uid) {
            let shell = fields.get(6).map(|s| s.trim()).unwrap_or_default();
            if !shell.is_empty() {
                return Some(shell.to_string());
            }
        }
    }
    None
}

fn get_restore_shell() -> String {
    if let Ok(shell) = std::env::var("SHELL") {
        if !shell.is_empty() {
            return shell;
        }
    }
    #[cfg(target_os = "linux")]
    if let Some(shell) = get_shell_from_passwd() {
        return shell;
    }
    "/bin/sh".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CwdFlag {
    Separated(&'static str),
    Joined(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalProfile {
    Kitty,
    Foot,
    Wezterm,
    Ghostty,
    Alacritty,
    Generic,
}

impl TerminalProfile {
    fn from_executable(name: &str) -> Self {
        let lower = name.to_lowercase();
        let last_segment = lower.rsplit('.').next().unwrap_or(&lower);
        match last_segment {
            "kitty" => Self::Kitty,
            "foot" => Self::Foot,
            "wezterm" => Self::Wezterm,
            "ghostty" => Self::Ghostty,
            "alacritty" => Self::Alacritty,
            _ => Self::Generic,
        }
    }

    fn from_args(args: &[String]) -> Self {
        args.iter()
            .rev()
            .map(|arg| Self::from_executable(arg))
            .find(|profile| *profile != Self::Generic)
            .unwrap_or(Self::Generic)
    }

    const fn needs_start_subcommand(self) -> bool {
        matches!(self, Self::Wezterm)
    }

    const fn cwd_flag(self) -> CwdFlag {
        match self {
            Self::Kitty => CwdFlag::Separated("--directory"),
            Self::Foot | Self::Alacritty | Self::Generic => {
                CwdFlag::Separated("--working-directory")
            }
            Self::Wezterm => CwdFlag::Separated("--cwd"),
            Self::Ghostty => CwdFlag::Joined("--working-directory="),
        }
    }

    const fn cmd_flag(self) -> Option<&'static str> {
        match self {
            Self::Kitty | Self::Foot => None,
            Self::Wezterm => Some("--"),
            Self::Ghostty | Self::Alacritty | Self::Generic => Some("-e"),
        }
    }
}

fn build_terminal_restore_command(
    launch_prefix: &[String],
    profile: TerminalProfile,
    child_cmd: &[String],
    child_cwd: Option<&str>,
) -> Vec<String> {
    let mut cmd: Vec<String> = launch_prefix.to_vec();

    if profile.needs_start_subcommand() {
        cmd.push("start".to_string());
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let cwd_to_use = child_cwd.filter(|cwd| !cwd.is_empty() && *cwd != home);

    if let Some(cwd) = cwd_to_use {
        match profile.cwd_flag() {
            CwdFlag::Separated(flag) => {
                cmd.push(flag.to_string());
                cmd.push(cwd.to_string());
            }
            CwdFlag::Joined(flag) => {
                cmd.push(format!("{flag}{cwd}"));
            }
        }
    }

    if let Some(flag) = profile.cmd_flag() {
        cmd.push(flag.to_string());
    }

    let escaped_cmd: String = child_cmd
        .iter()
        .map(|arg| shell_escape(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let restore_shell = get_restore_shell();
    cmd.push("sh".to_string());
    cmd.push("-c".to_string());
    cmd.push(format!(
        "{}; exec {}",
        escaped_cmd,
        shell_escape(&restore_shell)
    ));

    cmd
}

fn build_spawn_command(
    app_id: &str,
    saved_window: &SavedWindow,
    app_mappings: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mapped = app_mappings
        .get(app_id)
        .cloned()
        .unwrap_or_else(|| vec![app_id.to_string()]);

    if let Some(ts) = &saved_window.terminal_state {
        if let Some(child_cmd) = &ts.child_command {
            let args = child_cmd.to_args();
            if !args.is_empty() {
                let profile = TerminalProfile::from_args(&mapped);
                return build_terminal_restore_command(&mapped, profile, &args, ts.child_cwd.as_deref());
            }
        }
    }

    mapped
}

async fn resolve_terminal_state(
    pid: u32,
    config: &TerminalStateConfig,
) -> Option<(Vec<String>, String)> {
    let shell_names = config.shell_names.clone();
    let helper_names = config.helper_names.clone();
    let max_depth = config.max_walk_depth;
    spawn_blocking(move || proc::resolve_child_process(pid, &shell_names, &helper_names, max_depth))
        .await
        .ok()
        .flatten()
}

/// Atomic file write: temp file + fsync of file contents + rename + fsync of
/// the parent directory. The parent-dir fsync is what makes the rename itself
/// durable across a power loss; without it the machine can come back with the
/// previous session file (or no file at all).
fn atomic_write(file_path: &Path, data: &str) -> Result<()> {
    let tmp_path = file_path.with_extension("json.tmp");

    let mut file =
        fs::File::create(&tmp_path).context("Failed to create temporary session file")?;
    file.write_all(data.as_bytes())
        .context("Failed to write session data")?;
    file.sync_all()
        .context("Failed to sync session data to disk")?;
    drop(file);

    fs::rename(&tmp_path, file_path).context("Failed to atomically replace session file")?;

    if let Some(parent) = file_path.parent() {
        let dir = fs::File::open(parent)
            .with_context(|| format!("Failed to open parent directory {} for fsync", parent.display()))?;
        dir.sync_all()
            .context("Failed to fsync parent directory after rename")?;
    }

    Ok(())
}

fn dedupe_single_instance_windows(
    windows: Vec<SavedWindow>,
    single_instance_apps: &[String],
) -> Vec<SavedWindow> {
    let single: std::collections::HashSet<&String> = single_instance_apps.iter().collect();
    let mut seen_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    windows
        .into_iter()
        .filter(|w| {
            if !single.contains(&w.app_id) {
                return true;
            }
            w.pid.map_or(true, |pid| seen_pids.insert(pid))
        })
        .collect()
}

fn filter_skipped_windows(windows: Vec<SavedWindow>, skip_apps: &[String]) -> Vec<SavedWindow> {
    windows
        .into_iter()
        .filter(|w| !skip_apps.iter().any(|s| s == &w.app_id))
        .collect()
}

async fn save_session_with_terminal_state(file_path: &Path, app_config: &AppConfig) -> Result<()> {
    let windows = get_niri_windows().await?;
    let workspaces = get_niri_workspaces().await?;
    let terminal_config = &app_config.terminal_state;

    let mut saved_windows = Vec::with_capacity(windows.len());

    for window in &windows {
        let ws = workspaces
            .iter()
            .find(|w| window.workspace_id == Some(w.id));
        let app_id = window.app_id.clone().unwrap_or_default();
        let pid = window
            .pid
            .and_then(|p| u32::try_from(p).ok())
            .filter(|p| *p > 0);

        let terminal_state = if terminal_config.enabled {
            match pid {
                Some(pid) if terminal_config.terminal_app_ids.contains(&app_id) => {
                    resolve_terminal_state(pid, terminal_config).await.map(
                        |(child_command, child_cwd)| TerminalState {
                            child_command: Some(ChildCommand::Args(child_command)),
                            child_cwd: Some(child_cwd),
                        },
                    )
                }
                _ => None,
            }
        } else {
            None
        };

        saved_windows.push(SavedWindow {
            id: window.id,
            app_id: app_id.clone(),
            workspace: WorkspaceInfo::from_workspace(ws),
            is_focused: window.is_focused,
            pid,
            terminal_state,
        });
    }

    let skipped = saved_windows.len();
    let saved_windows = filter_skipped_windows(saved_windows, &app_config.skip_apps.apps);
    if saved_windows.len() < skipped {
        info!(
            "Not saving {} window(s) of skip-listed apps",
            skipped.saturating_sub(saved_windows.len())
        );
    }

    let before_dedupe = saved_windows.len();
    let saved_windows =
        dedupe_single_instance_windows(saved_windows, &app_config.single_instance.apps);
    if saved_windows.len() < before_dedupe {
        info!(
            "Deduped {} extra surface(s) of single-instance apps sharing one process",
            before_dedupe.saturating_sub(saved_windows.len())
        );
    }

    let session = VersionedSession {
        version: SESSION_FORMAT_VERSION,
        windows: saved_windows,
    };
    let json_data =
        serde_json::to_string_pretty(&session).context("Failed to serialize window data")?;

    atomic_write(file_path, &json_data).context("Failed to write session file")?;
    info!("Session saved to {}", file_path.display());
    Ok(())
}

/// Reads and parses the session file.
///
/// Returns `Ok(None)` when there is no usable session data (missing file, or
/// a corrupt file with no valid backup): a fresh session should then be
/// seeded from the current niri state.
fn load_session_windows(file_path: &Path) -> Result<Option<Vec<SavedWindow>>> {
    if !file_path.exists() {
        info!("No previous session found at {}", file_path.display());
        return Ok(None);
    }
    let session_data = fs::read_to_string(file_path).context("Failed to read session file")?;
    if session_data.trim().is_empty() {
        info!("Session file at {} is empty", file_path.display());
        return Ok(Some(Vec::new()));
    }
    match serde_json::from_str::<SessionData>(&session_data) {
        Ok(session) => {
            if session.is_legacy() {
                warn!("Session file uses legacy format (no version field). Consider re-saving to upgrade.");
            }
            Ok(Some(session.into_windows()))
        }
        Err(e) => {
            warn!(
                "Session file at {} is corrupt ({}). Attempting backup recovery...",
                file_path.display(),
                e
            );
            if let Some((backup_path, backup_data)) = find_latest_valid_backup(file_path) {
                info!("Recovered session from backup: {}", backup_path.display());
                Ok(Some(backup_data.into_windows()))
            } else {
                warn!("No valid backup found.");
                Ok(None)
            }
        }
    }
}

/// Applies the restore-time filters and caps, with the same warnings as
/// before: drop terminal windows without captured state, warn about
/// suspicious per-app counts, cap at `--max-restore-windows`.
fn prepare_saved_windows(
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
struct RunningWindow {
    id: u64,
    app_id: Option<String>,
    workspace_name: Option<String>,
    workspace_idx: Option<u8>,
}

async fn snapshot_running_windows() -> Result<Vec<RunningWindow>> {
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
fn workspace_matches(saved: &WorkspaceInfo, running: &RunningWindow) -> bool {
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
fn plan_spawns(
    saved: &[SavedWindow],
    running: &[RunningWindow],
    config: &Config,
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
struct SpawnLimiter {
    global: Arc<Semaphore>,
    per_app: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl SpawnLimiter {
    fn new(max_global_concurrency: usize) -> Self {
        Self {
            global: Arc::new(Semaphore::new(max_global_concurrency)),
            per_app: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn acquire(
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

async fn restore_session_internal(
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
    let to_spawn = plan_spawns(&prepared, &running, config, app_config);
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
async fn spawn_windows(
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
        let command =
            build_spawn_command(&saved_window.app_id, &saved_window, &app_config.app_mappings);
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
async fn spawn_single_window(
    saved_window: &SavedWindow,
    command: &[String],
    spawn_timeout: u64,
    claimed: &Mutex<HashSet<u64>>,
    workspaces: &[Workspace],
) -> Result<usize> {
    let mut spawn_socket = Socket::connect().context("Failed to connect to Niri IPC socket")?;
    let reply = spawn_socket
        .send(Request::Action(Action::Spawn {
            command: command.to_vec(),
        }))
        .context("Failed to send spawn request")?;

    if !matches!(reply, Reply::Ok(Response::Handled)) {
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
async fn wait_for_new_window(
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

/// Best-effort placement: pin to the saved output if it still exists (with a
/// workspace-based fallback), move to the saved workspace, never steal focus
/// with the move itself.
async fn apply_window_placement(win_id: u64, saved_window: &SavedWindow, workspaces: &[Workspace]) {
    if let Some(output) = resolve_target_output(&saved_window.workspace, workspaces) {
        let connect = Socket::connect().context("Failed to connect to Niri IPC socket");
        if let Ok(mut move_socket) = connect {
            let result = move_socket.send(Request::Action(Action::MoveWindowToMonitor {
                id: Some(win_id),
                output,
            }));
            if let Err(e) = &result {
                warn!("Warning: failed to move window {win_id} to monitor: {e:?}");
            }
        }
    }

    let workspace_reference = saved_window
        .workspace
        .name
        .as_ref()
        .filter(|n| !n.is_empty())
        .cloned()
        .map(WorkspaceReferenceArg::Name)
        .or_else(|| {
            saved_window
                .workspace
                .idx
                .filter(|i| *i > 0)
                .map(WorkspaceReferenceArg::Index)
        });

    match workspace_reference {
        Some(reference) => {
            let connect = Socket::connect().context("Failed to connect to Niri IPC socket");
            if let Ok(mut move_socket) = connect {
                if let Err(e) = move_socket.send(Request::Action(Action::MoveWindowToWorkspace {
                    window_id: Some(win_id),
                    reference,
                    focus: false,
                })) {
                    warn!("Warning: failed to move window {win_id} to workspace: {e:?}");
                }
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
fn resolve_target_output(saved: &WorkspaceInfo, workspaces: &[Workspace]) -> Option<String> {
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
async fn focus_window(win_id: u64, app_id: &str) {
    match niri_send(Request::Action(Action::FocusWindow { id: win_id })).await {
        Ok(_) => info!("Restored focus to window {win_id} of app {app_id}"),
        Err(e) => warn!("Warning: failed to focus window {win_id}: {e}"),
    }
}

async fn handle_shutdown_signals() {
    let mut term_signal = signal(SignalKind::terminate()).expect("Failed to listen for SIGTERM");
    let mut int_signal = signal(SignalKind::interrupt()).expect("Failed to listen for SIGINT");
    let mut quit_signal = signal(SignalKind::quit()).expect("Failed to listen for SIGQUIT");

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
}

async fn periodic_save_session(
    file_path: std::path::PathBuf,
    config: Config,
    app_config: AppConfig,
) {
    let interval_secs = config.save_interval.max(1) * 60;
    let interval = Duration::from_secs(interval_secs);

    info!(
        "Starting periodic save task (interval: {} minutes)",
        config.save_interval.max(1)
    );

    loop {
        sleep(interval).await;
        if let Err(e) = save_session_with_backup(&file_path, &config, &app_config).await {
            error!("Error saving session: {}", e);
        }
    }
}

async fn save_session_with_backup(
    file_path: &Path,
    config: &Config,
    app_config: &AppConfig,
) -> Result<()> {
    create_backup(file_path)?;

    if let Some(session_dir) = file_path.parent() {
        cleanup_old_backups(session_dir, config.max_backup_count)?;
    }

    save_session_with_terminal_state(file_path, app_config).await
}

fn create_backup(file_path: &Path) -> Result<()> {
    if file_path.exists() {
        let contents =
            fs::read_to_string(file_path).context("Failed to read session file for backup")?;
        if serde_json::from_str::<SessionData>(&contents).is_err() {
            warn!(
                "Existing session file is corrupt; not backing it up (a corrupt backup would evict valid ones from rotation)"
            );
            return Ok(());
        }
        let timestamp = Local::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let backup_file_name = format!(
            "{}-{}.bak",
            file_path.file_stem().unwrap_or_default().to_string_lossy(),
            timestamp
        );
        let mut backup_path = file_path.to_path_buf();
        backup_path.set_file_name(backup_file_name);
        fs::copy(file_path, &backup_path).context("Failed to create backup file")?;
        info!("Backup created at {}", backup_path.display());
    }
    Ok(())
}

/// Attempts to find and parse the most recent valid `.bak` file alongside the session file.
/// Returns the backup path and parsed session data if a valid backup exists.
fn find_latest_valid_backup(file_path: &Path) -> Option<(std::path::PathBuf, SessionData)> {
    let dir = file_path.parent()?;

    let mut backups: Vec<_> = fs::read_dir(dir)
        .ok()?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "bak"))
        .collect();

    backups.sort_by(|a, b| {
        b.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH)
            .cmp(
                &a.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(UNIX_EPOCH),
            )
    });

    for backup in backups {
        let path = backup.path();
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(session) = serde_json::from_str::<SessionData>(&data) {
                return Some((path, session));
            }
        }
    }

    None
}

fn cleanup_old_backups(session_dir: &Path, keep_count: usize) -> Result<()> {
    let mut backups: Vec<_> = fs::read_dir(session_dir)?
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry
                .path()
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".bak"))
        })
        .collect();

    if backups.len() <= keep_count {
        return Ok(());
    }

    backups.sort_by(|a, b| {
        b.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH)
            .cmp(
                &a.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(UNIX_EPOCH),
            )
    });

    for backup in backups.iter().skip(keep_count) {
        if let Err(e) = fs::remove_file(backup.path()) {
            warn!(
                "Failed to remove old backup {}: {}",
                backup.path().display(),
                e
            );
        } else {
            info!("Removed old backup: {}", backup.path().display());
        }
    }

    Ok(())
}

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Config {
    #[arg(long, default_value = "15")]
    save_interval: u64,

    #[arg(long, default_value = "5")]
    max_backup_count: usize,

    #[arg(long, default_value = "5")]
    spawn_timeout: u64,

    #[arg(long, default_value = "3")]
    retry_attempts: u32,

    #[arg(long, default_value = "2")]
    retry_delay: u64,

    /// Sanity cap on how many windows a single restore may spawn
    #[arg(long, default_value_t = MAX_RESTORE_WINDOWS_DEFAULT)]
    max_restore_windows: usize,

    /// Preview what would be restored without actually spawning windows or modifying files
    #[arg(long, default_value = "false")]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::parse();

    // Validate: reject nonsensical CLI values that would cause silent misbehavior
    if config.save_interval == 0 {
        anyhow::bail!("--save-interval must be at least 1 minute");
    }
    if config.max_backup_count == 0 {
        anyhow::bail!("--max-backup-count must be at least 1");
    }
    if config.spawn_timeout == 0 {
        anyhow::bail!("--spawn-timeout must be at least 1 second");
    }
    if config.max_restore_windows == 0 {
        anyhow::bail!("--max-restore-windows must be at least 1");
    }

    info!("Starting niri-session-manager");
    let session_file_path = get_session_file_path()?;

    let app_config = match load_app_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!("Failed to load app config, using defaults: {e}");
            AppConfig::default()
        }
    };

    let boot_id = get_boot_id();
    let marker_path = get_restore_marker_path(&session_file_path);
    if already_restored_this_boot(&boot_id, &marker_path) {
        info!(
            "Session already restored for this boot; skipping restore (marker: {})",
            marker_path.display()
        );
    } else {
        info!("Restoring previous session");
        match restore_session(&session_file_path, &config, &app_config).await {
            Ok(()) => {
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

    if config.dry_run {
        info!("Dry run complete — exiting without starting periodic save.");
        return Ok(());
    }

    let save_task = spawn(periodic_save_session(
        session_file_path.clone(),
        config.clone(),
        app_config.clone(),
    ));

    let signal_task = spawn(handle_shutdown_signals());

    let _ = signal_task.await;
    save_task.abort();
    let _ = save_task.await;

    info!("Saving final session before shutdown");
    let final_save = save_session_with_backup(&session_file_path, &config, &app_config);
    match tokio::time::timeout(Duration::from_secs(5), final_save).await {
        Ok(Ok(())) => info!("Final session saved"),
        Ok(Err(e)) => error!("Error saving final session: {}", e),
        Err(_) => warn!("Final save timed out"),
    }

    info!("Shutdown complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected_exec_suffix(child_cmd: &str) -> String {
        let shell = get_restore_shell();
        format!("{}; exec {}", shell_escape(child_cmd), shell_escape(&shell))
    }

    #[test]
    fn dedupe_single_instance_keeps_one_window_per_pid() {
        let win = |id: u64, app: &str, pid: Option<u32>| SavedWindow {
            id,
            app_id: app.to_string(),
            workspace: WorkspaceInfo::default(),
            is_focused: false,
            pid,
            terminal_state: None,
        };
        let windows = vec![
            win(1, "com.mitchellh.ghostty", Some(42)),
            win(2, "com.mitchellh.ghostty", Some(42)),
            win(3, "com.mitchellh.ghostty", Some(42)),
            win(4, "firefox", Some(7)),
            win(5, "firefox", Some(8)),
            win(6, "unknown-app", None),
        ];
        let out = dedupe_single_instance_windows(windows, &["com.mitchellh.ghostty".to_string()]);
        assert_eq!(out.len(), 4, "3 same-pid ghostty surfaces collapse to 1; firefox pids differ so both stay; no-pid window stays");
        assert_eq!(out[0].id, 1);
        assert_eq!(out[1].id, 4);
        assert_eq!(out[2].id, 5);
        assert_eq!(out[3].id, 6);
    }

    #[test]
    fn filter_skipped_windows_removes_only_skipped_apps() {
        let win = |id: u64, app: &str| SavedWindow {
            id,
            app_id: app.to_string(),
            workspace: WorkspaceInfo::default(),
            is_focused: false,
            pid: None,
            terminal_state: None,
        };
        let windows = vec![win(1, "xdg-desktop-portal"), win(2, "firefox")];
        let out = filter_skipped_windows(windows, &["xdg-desktop-portal".to_string()]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].app_id, "firefox");
    }

    #[test]
    fn already_restored_this_boot_matches_trimmed_marker() {
        let dir = std::env::temp_dir().join(format!("nsm-marker-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("restore-marker");
        let boot_id = Some("boot-abc-123".to_string());
        assert!(
            !already_restored_this_boot(&boot_id, &marker),
            "missing marker = not restored"
        );
        atomic_write(&marker, "boot-abc-123\n").unwrap();
        assert!(
            already_restored_this_boot(&boot_id, &marker),
            "matching boot id = restored"
        );
        assert!(
            !already_restored_this_boot(&Some("other-boot".to_string()), &marker),
            "different boot id = not restored"
        );
        assert!(
            !already_restored_this_boot(&None, &marker),
            "unknown boot id (no /proc access) = never skip"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn shell_escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn shell_escape_simple() {
        assert_eq!(shell_escape("btop"), "'btop'");
    }

    #[test]
    fn shell_escape_with_spaces() {
        assert_eq!(shell_escape("nvim /path/to file"), "'nvim /path/to file'");
    }

    #[test]
    fn shell_escape_with_single_quotes() {
        assert_eq!(shell_escape("echo 'hello'"), "'echo '\\''hello'\\'''");
    }

    #[test]
    fn shell_escape_with_semicolons() {
        assert_eq!(shell_escape("cmd; rm -rf /"), "'cmd; rm -rf /'");
    }

    #[test]
    fn shell_escape_with_dollar() {
        assert_eq!(shell_escape("echo $HOME"), "'echo $HOME'");
    }

    #[test]
    fn shell_escape_with_backticks() {
        assert_eq!(shell_escape("echo `whoami`"), "'echo `whoami`'");
    }

    #[test]
    fn terminal_profile_from_args_finds_flatpak_wrapped_terminal() {
        let mapped = vec![
            "flatpak".to_string(),
            "run".to_string(),
            "org.wezfurlong.wezterm".to_string(),
        ];
        let profile = TerminalProfile::from_args(&mapped);
        assert_eq!(profile, TerminalProfile::Wezterm);
    }

    #[test]
    fn terminal_profile_from_args_plain_app_id() {
        let mapped = vec!["kitty".to_string()];
        assert_eq!(TerminalProfile::from_args(&mapped), TerminalProfile::Kitty);
        let unknown = vec!["my-terminal-wrapper".to_string()];
        assert_eq!(
            TerminalProfile::from_args(&unknown),
            TerminalProfile::Generic
        );
    }

    #[test]
    fn terminal_profile_kitty() {
        let p = TerminalProfile::from_executable("kitty");
        assert!(!p.needs_start_subcommand());
        assert_eq!(p.cwd_flag(), CwdFlag::Separated("--directory"));
        assert_eq!(p.cmd_flag(), None);
    }

    #[test]
    fn terminal_profile_foot() {
        let p = TerminalProfile::from_executable("foot");
        assert!(!p.needs_start_subcommand());
        assert_eq!(p.cwd_flag(), CwdFlag::Separated("--working-directory"));
        assert_eq!(p.cmd_flag(), None);
    }

    #[test]
    fn terminal_profile_wezterm() {
        let p = TerminalProfile::from_executable("wezterm");
        assert!(p.needs_start_subcommand());
        assert_eq!(p.cwd_flag(), CwdFlag::Separated("--cwd"));
        assert_eq!(p.cmd_flag(), Some("--"));
    }

    #[test]
    fn terminal_profile_ghostty() {
        let p = TerminalProfile::from_executable("ghostty");
        assert!(!p.needs_start_subcommand());
        assert_eq!(p.cwd_flag(), CwdFlag::Joined("--working-directory="));
        assert_eq!(p.cmd_flag(), Some("-e"));
    }

    #[test]
    fn terminal_profile_alacritty() {
        let p = TerminalProfile::from_executable("alacritty");
        assert!(!p.needs_start_subcommand());
        assert_eq!(p.cwd_flag(), CwdFlag::Separated("--working-directory"));
        assert_eq!(p.cmd_flag(), Some("-e"));
    }

    #[test]
    fn terminal_profile_generic() {
        let p = TerminalProfile::from_executable("unknown-terminal");
        assert!(!p.needs_start_subcommand());
        assert_eq!(p.cwd_flag(), CwdFlag::Separated("--working-directory"));
        assert_eq!(p.cmd_flag(), Some("-e"));
    }

    fn assert_restore_command(cmd: &[String], expected_prefix: &[&str], child_cmd: &str) {
        for (i, expected) in expected_prefix.iter().enumerate() {
            assert_eq!(cmd[i], *expected);
        }
        assert_eq!(cmd[expected_prefix.len()], expected_exec_suffix(child_cmd));
    }

    #[test]
    fn build_restore_kitty_with_cwd() {
        let profile = TerminalProfile::Kitty;
        let cmd = build_terminal_restore_command(
            &["kitty".to_string()],
            &profile,
            &["btop".to_string()],
            &Some("/home/user/projects".to_string()),
        );
        assert_restore_command(
            &cmd,
            &["kitty", "--directory", "/home/user/projects", "sh", "-c"],
            "btop",
        );
    }

    #[test]
    fn build_restore_kitty_without_cwd() {
        let profile = TerminalProfile::Kitty;
        let home = std::env::var("HOME").unwrap_or_default();
        let cmd = build_terminal_restore_command(
            &["kitty".to_string()],
            &profile,
            &["btop".to_string()],
            &Some(home),
        );
        assert_restore_command(&cmd, &["kitty", "sh", "-c"], "btop");
    }

    #[test]
    fn build_restore_wezterm_with_cwd() {
        let profile = TerminalProfile::Wezterm;
        let cmd = build_terminal_restore_command(
            &["wezterm".to_string()],
            &profile,
            &["btop".to_string()],
            &Some("/home/user/projects".to_string()),
        );
        assert_restore_command(
            &cmd,
            &[
                "wezterm",
                "start",
                "--cwd",
                "/home/user/projects",
                "--",
                "sh",
                "-c",
            ],
            "btop",
        );
    }

    #[test]
    fn build_restore_ghostty_with_cwd() {
        let profile = TerminalProfile::Ghostty;
        let cmd = build_terminal_restore_command(
            &["ghostty".to_string()],
            &profile,
            &["btop".to_string()],
            &Some("/home/user/projects".to_string()),
        );
        assert_restore_command(
            &cmd,
            &[
                "ghostty",
                "--working-directory=/home/user/projects",
                "-e",
                "sh",
                "-c",
            ],
            "btop",
        );
    }

    #[test]
    fn build_restore_foot_with_cwd() {
        let profile = TerminalProfile::Foot;
        let cmd = build_terminal_restore_command(
            &["foot".to_string()],
            &profile,
            &["btop".to_string()],
            &Some("/home/user/projects".to_string()),
        );
        assert_restore_command(
            &cmd,
            &[
                "foot",
                "--working-directory",
                "/home/user/projects",
                "sh",
                "-c",
            ],
            "btop",
        );
    }

    #[test]
    fn build_restore_alacritty_with_cwd() {
        let profile = TerminalProfile::Alacritty;
        let cmd = build_terminal_restore_command(
            &["alacritty".to_string()],
            &profile,
            &["btop".to_string()],
            &Some("/home/user/projects".to_string()),
        );
        assert_restore_command(
            &cmd,
            &[
                "alacritty",
                "--working-directory",
                "/home/user/projects",
                "-e",
                "sh",
                "-c",
            ],
            "btop",
        );
    }

    #[test]
    fn build_restore_with_shell_metacharacters() {
        let profile = TerminalProfile::Kitty;
        let cmd = build_terminal_restore_command(
            &["kitty".to_string()],
            &profile,
            &["echo 'hello'; rm -rf /".to_string()],
            &None,
        );
        assert_eq!(cmd[3], expected_exec_suffix("echo 'hello'; rm -rf /"));
    }

    #[test]
    fn build_restore_preserves_multi_arg_command() {
        let profile = TerminalProfile::Kitty;
        let cmd = build_terminal_restore_command(
            &["kitty".to_string()],
            &profile,
            &["nvim".to_string(), "/path/to file".to_string()],
            &None,
        );
        let expected_suffix = {
            let shell = get_restore_shell();
            format!(
                "{} {}; exec {}",
                shell_escape("nvim"),
                shell_escape("/path/to file"),
                shell_escape(&shell)
            )
        };
        assert_eq!(cmd[3], expected_suffix);
    }

    #[test]
    fn build_restore_preserves_mapped_launch_prefix() {
        let profile = TerminalProfile::Generic;
        let cmd = build_terminal_restore_command(
            &[
                "flatpak".to_string(),
                "run".to_string(),
                "org.myterm".to_string(),
            ],
            &profile,
            &["btop".to_string()],
            &None,
        );
        assert_eq!(cmd[0], "flatpak");
        assert_eq!(cmd[1], "run");
        assert_eq!(cmd[2], "org.myterm");
    }

    #[test]
    fn build_spawn_command_falls_back_to_mappings() {
        let mut mappings = HashMap::new();
        mappings.insert(
            "com.mitchellh.ghostty".to_string(),
            vec!["ghostty".to_string()],
        );

        let window = SavedWindow {
            id: 1,
            app_id: "com.mitchellh.ghostty".to_string(),
            workspace: WorkspaceInfo::default(),
            is_focused: false,
            pid: None,
            terminal_state: None,
        };

        let cmd = build_spawn_command("com.mitchellh.ghostty", &window, &mappings);
        assert_eq!(cmd, vec!["ghostty"]);
    }

    #[test]
    fn build_spawn_command_uses_terminal_state() {
        let mappings = HashMap::new();
        let window = SavedWindow {
            id: 1,
            app_id: "kitty".to_string(),
            workspace: WorkspaceInfo {
                idx: Some(0),
                ..Default::default()
            },
            is_focused: true,
            pid: Some(1234),
            terminal_state: Some(TerminalState {
                child_command: Some(ChildCommand::Args(vec!["btop".to_string()])),
                child_cwd: Some("/home/user".to_string()),
            }),
        };

        let cmd = build_spawn_command("kitty", &window, &mappings);
        assert_eq!(cmd[0], "kitty");
        assert!(cmd.contains(&expected_exec_suffix("btop")));
    }

    #[test]
    fn saved_window_deserializes_old_format_with_workspace_id() {
        // workspace_id is silently ignored by serde (no deny_unknown_fields)
        let json = r#"{
            "id": 42,
            "app_id": "kitty",
            "workspace_id": 3,
            "is_focused": true
        }"#;
        let w: SavedWindow = serde_json::from_str(json).unwrap();
        assert_eq!(w.id, 42);
        assert_eq!(w.app_id, "kitty");
        assert_eq!(w.workspace.idx, None);
        assert_eq!(w.workspace.name, None);
        assert_eq!(w.workspace.output, None);
        assert!(w.terminal_state.is_none());
        assert!(w.pid.is_none());
    }

    #[test]
    fn saved_window_deserializes_new_format_with_workspace_fields() {
        let json = r#"{
            "id": 42,
            "app_id": "kitty",
            "workspace_idx": 2,
            "workspace_name": "dev",
            "workspace_output": "eDP-1",
            "is_focused": true,
            "pid": 1234,
            "terminal_state": {
                "child_command": "btop",
                "child_cwd": "/home/user"
            }
        }"#;
        let w: SavedWindow = serde_json::from_str(json).unwrap();
        assert_eq!(w.workspace.idx, Some(2));
        assert_eq!(w.workspace.name, Some("dev".to_string()));
        assert_eq!(w.workspace.output, Some("eDP-1".to_string()));
        assert_eq!(w.pid, Some(1234));
        let ts = w.terminal_state.unwrap();
        assert_eq!(
            ts.child_command,
            Some(ChildCommand::Legacy("btop".to_string()))
        );
        assert_eq!(ts.child_cwd, Some("/home/user".to_string()));
    }

    #[test]
    fn saved_window_deserializes_v3_array_child_command() {
        let json = r#"{
            "id": 42,
            "app_id": "kitty",
            "is_focused": true,
            "pid": 1234,
            "terminal_state": {
                "child_command": ["nvim", "/path/to/file"],
                "child_cwd": "/home/user"
            }
        }"#;
        let w: SavedWindow = serde_json::from_str(json).unwrap();
        let ts = w.terminal_state.unwrap();
        assert_eq!(
            ts.child_command,
            Some(ChildCommand::Args(vec![
                "nvim".to_string(),
                "/path/to/file".to_string()
            ]))
        );
    }

    #[test]
    fn saved_window_deserializes_minimal() {
        let json = r#"{"id": 1, "app_id": "firefox", "is_focused": false}"#;
        let w: SavedWindow = serde_json::from_str(json).unwrap();
        assert_eq!(w.app_id, "firefox");
        assert_eq!(w.workspace.idx, None);
        assert!(w.terminal_state.is_none());
        assert!(w.pid.is_none());
    }

    #[test]
    fn saved_window_silently_ignores_legacy_workspace_id() {
        let json = r#"{
            "id": 42,
            "app_id": "kitty",
            "workspace_id": 3,
            "is_focused": true
        }"#;
        let w: SavedWindow = serde_json::from_str(json).unwrap();
        assert_eq!(w.workspace.idx, None);
    }

    #[test]
    fn config_default_values() {
        let c = TerminalStateConfig::default();
        assert!(c.enabled);
        assert!(c.terminal_app_ids.contains(&"kitty".to_string()));
        assert!(c.shell_names.contains(&"fish".to_string()));
        assert!(c.helper_names.contains(&"kitten".to_string()));
        assert_eq!(c.max_walk_depth, 20);
    }

    #[test]
    fn session_data_parses_versioned_format() {
        let json = r#"{
            "version": 2,
            "windows": [
                {"id": 1, "app_id": "kitty", "is_focused": false}
            ]
        }"#;
        let session: SessionData = serde_json::from_str(json).unwrap();
        assert!(!session.is_legacy());
        let windows = session.into_windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].app_id, "kitty");
    }

    #[test]
    fn session_data_parses_legacy_array_format() {
        let json = r#"[
            {"id": 1, "app_id": "kitty", "is_focused": false},
            {"id": 2, "app_id": "firefox", "is_focused": true}
        ]"#;
        let session: SessionData = serde_json::from_str(json).unwrap();
        assert!(session.is_legacy());
        let windows = session.into_windows();
        assert_eq!(windows.len(), 2);
    }

    #[test]
    fn versioned_session_serializes_correctly() {
        let session = VersionedSession {
            version: SESSION_FORMAT_VERSION,
            windows: vec![SavedWindow {
                id: 42,
                app_id: "kitty".to_string(),
                workspace: WorkspaceInfo {
                    idx: Some(1),
                    ..Default::default()
                },
                is_focused: false,
                pid: Some(1234),
                terminal_state: Some(TerminalState {
                    child_command: Some(ChildCommand::Args(vec!["btop".to_string()])),
                    child_cwd: Some("/home/user".to_string()),
                }),
            }],
        };
        let json = serde_json::to_string_pretty(&session).unwrap();
        assert!(json.contains("\"version\": 3"));
        assert!(json.contains("\"windows\""));
        let parsed: SessionData = serde_json::from_str(&json).unwrap();
        assert!(!parsed.is_legacy());
    }

    #[test]
    fn get_restore_shell_returns_non_empty() {
        let shell = get_restore_shell();
        assert!(!shell.is_empty());
        assert!(shell.contains('/') || shell == "/bin/sh");
    }

    #[test]
    fn get_restore_shell_prefers_env_var() {
        let shell = get_restore_shell();
        if let Ok(env_shell) = std::env::var("SHELL") {
            if !env_shell.is_empty() {
                assert_eq!(shell, env_shell);
            }
        }
    }

    #[test]
    fn create_backup_skips_corrupt_session_file() {
        let tmp = tempfile::tempdir().unwrap();
        let session_path = tmp.path().join("session.json");
        fs::write(&session_path, "{CORRUPT").unwrap();

        create_backup(&session_path).unwrap();

        let backups: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "bak"))
            .collect();
        assert!(
            backups.is_empty(),
            "corrupt session must not enter the backup rotation"
        );
    }

    #[test]
    fn create_backup_copies_valid_session_file() {
        let tmp = tempfile::tempdir().unwrap();
        let session_path = tmp.path().join("session.json");
        fs::write(&session_path, r#"{"version":3,"windows":[]}"#).unwrap();

        create_backup(&session_path).unwrap();

        let backups: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "bak"))
            .collect();
        assert_eq!(backups.len(), 1, "valid session gets exactly one backup");
    }

    #[test]
    fn find_latest_valid_backup_returns_most_recent() {
        let tmp = tempfile::tempdir().unwrap();
        let session_path = tmp.path().join("session.json");

        let old_bak = tmp.path().join("session-2024-01-01T00:00:00Z.bak");
        let new_bak = tmp.path().join("session-2024-06-01T00:00:00Z.bak");
        // Write old backup first, then new one later so it has a newer mtime
        fs::write(&old_bak, r#"{"version":3,"windows":[]}"#).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(
            &new_bak,
            r#"{"version":3,"windows":[{"id":1,"app_id":"firefox","is_focused":false}]}"#,
        )
        .unwrap();

        let result = find_latest_valid_backup(&session_path);
        assert!(result.is_some());
        let (path, data) = result.unwrap();
        assert_eq!(path, new_bak);
        assert_eq!(data.into_windows().len(), 1);
    }

    #[test]
    fn find_latest_valid_backup_skips_corrupt() {
        let tmp = tempfile::tempdir().unwrap();
        let session_path = tmp.path().join("session.json");

        let corrupt_bak = tmp.path().join("session-corrupt.bak");
        let good_bak = tmp.path().join("session-good.bak");
        fs::write(&corrupt_bak, "{NOT VALID JSON}").unwrap();
        fs::write(&good_bak, r#"{"version":3,"windows":[]}"#).unwrap();

        let result = find_latest_valid_backup(&session_path);
        assert!(result.is_some());
        let (_, data) = result.unwrap();
        assert_eq!(data.into_windows().len(), 0);
    }

    #[test]
    fn find_latest_valid_backup_returns_none_when_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let session_path = tmp.path().join("session.json");
        let result = find_latest_valid_backup(&session_path);
        assert!(result.is_none());
    }

    #[test]
    fn atomic_write_creates_file_with_correct_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.json");
        atomic_write(&path, "{\"test\":true}").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "{\"test\":true}");
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.json");
        fs::write(&path, "OLD CONTENT").unwrap();
        atomic_write(&path, "NEW CONTENT").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "NEW CONTENT");
    }

    #[test]
    fn atomic_write_leaves_no_temp_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.json");
        atomic_write(&path, "data").unwrap();
        let tmp_path = path.with_extension("json.tmp");
        assert!(!tmp_path.exists(), "temp file should not exist after write");
    }

    #[test]
    fn atomic_write_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a/b/c/session.json");
        let result = atomic_write(&nested, "data");
        assert!(result.is_err(), "should fail without parent dirs");
    }

    #[test]
    fn app_config_parses_full_toml() {
        let toml = r#"
[app_mappings]
"vesktop" = ["flatpak", "run", "dev.vencord.Vesktop"]
"com.mitchellh.ghostty" = ["ghostty"]

[single_instance_apps]
apps = ["firefox", "zen"]

[skip_apps]
apps = ["discord"]

[terminal_state]
enabled = true
terminal_app_ids = ["kitty", "foot"]
shell_names = ["fish", "bash"]
helper_names = ["kitten"]
max_walk_depth = 15
"#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.app_mappings.len(), 2);
        assert_eq!(
            config.app_mappings.get("vesktop"),
            Some(&vec![
                "flatpak".into(),
                "run".into(),
                "dev.vencord.Vesktop".into()
            ])
        );
        assert_eq!(config.single_instance.apps, vec!["firefox", "zen"]);
        assert_eq!(config.skip_apps.apps, vec!["discord"]);
        assert!(config.terminal_state.enabled);
        assert_eq!(config.terminal_state.max_walk_depth, 15);
    }

    #[test]
    fn app_config_parses_empty_toml() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert!(config.app_mappings.is_empty());
        assert!(config.single_instance.apps.is_empty());
        assert!(config.skip_apps.apps.is_empty());
        assert!(config.terminal_state.enabled); // defaults to true
    }

    #[test]
    fn app_config_parses_partial_toml() {
        let toml = r#"
[app_mappings]
"firefox" = ["firefox"]
"#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.app_mappings.len(), 1);
        assert!(config.single_instance.apps.is_empty());
        assert!(config.terminal_state.enabled);
    }

    #[test]
    fn terminal_state_config_defaults() {
        let config: TerminalStateConfig = toml::from_str("").unwrap();
        assert!(config.enabled);
        assert!(config.terminal_app_ids.contains(&"kitty".to_string()));
        assert!(config.shell_names.contains(&"fish".to_string()));
        assert_eq!(config.max_walk_depth, 20);
    }
}
