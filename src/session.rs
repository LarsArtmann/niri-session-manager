//! The session data model plus everything that reads or writes session
//! files: capture from niri, atomic writes, backups, and export/import.

use anyhow::{bail, Context, Result};
use chrono::{Local, SecondsFormat};
use niri_ipc::Workspace;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::UNIX_EPOCH;
use tracing::{info, warn};

use crate::config::{AppConfig, Config};
use crate::ipc::{get_niri_windows, get_niri_workspaces};
use crate::terminal::resolve_terminal_state;

pub fn get_session_file_path() -> Result<std::path::PathBuf> {
    let mut session_dir =
        dirs::data_dir().context("Failed to locate data directory (XDG_DATA_HOME)")?;
    session_dir.push("niri-session-manager");
    fs::create_dir_all(&session_dir).context("Failed to create session directory")?;
    Ok(session_dir.join("session.json"))
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkspaceInfo {
    #[serde(default, alias = "workspace_idx")]
    pub idx: Option<u8>,
    #[serde(default, alias = "workspace_name")]
    pub name: Option<String>,
    #[serde(default, alias = "workspace_output")]
    pub output: Option<String>,
}

impl WorkspaceInfo {
    pub fn from_workspace(ws: Option<&Workspace>) -> Self {
        ws.map_or_else(Self::default, |w| Self {
            idx: Some(w.idx),
            name: w.name.clone(),
            output: w.output.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub id: u64,
    pub app_id: String,
    #[serde(default, flatten)]
    pub workspace: WorkspaceInfo,
    pub is_focused: bool,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub terminal_state: Option<TerminalState>,
    /// Geometry at save time (format v5); `None` for pre-v5 files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<SavedWindowLayout>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum ChildCommand {
    Args(Vec<String>),
    Legacy(String),
}

impl ChildCommand {
    pub fn to_args(&self) -> Vec<String> {
        match self {
            Self::Args(args) => args.clone(),
            Self::Legacy(s) => s.split_whitespace().map(String::from).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalState {
    pub child_command: Option<ChildCommand>,
    pub child_cwd: Option<String>,
}

/// A window's slot in its workspace's scrolling layout.
///
/// Both indices are 1-based, matching niri's own reporting; they travel as a
/// pair because one without the other is meaningless.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrollPosition {
    pub column: u64,
    pub tile_in_column: u64,
}

/// The on-screen geometry a window had when the session was saved
/// (session format v5), captured from niri's `WindowLayout`.
///
/// Only the durable parts are kept: the scrolling-layout slot and the visible
/// tile size. Viewport-relative positions and Wayland-internal sizes change
/// with the monitor setup or carry no restore meaning, so they are dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWindowLayout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_position: Option<ScrollPosition>,
    /// Visible tile size in logical pixels, including borders.
    pub tile_width: f64,
    pub tile_height: f64,
}

impl SavedWindowLayout {
    /// Maps niri's `WindowLayout` onto the subset we keep in the session file.
    pub fn from_niri(layout: &niri_ipc::WindowLayout) -> Self {
        Self {
            scroll_position: layout
                .pos_in_scrolling_layout
                .and_then(|(column, tile_in_column)| {
                    Some(ScrollPosition {
                        column: u64::try_from(column).ok()?,
                        tile_in_column: u64::try_from(tile_in_column).ok()?,
                    })
                }),
            tile_width: layout.tile_size.0,
            tile_height: layout.tile_size.1,
        }
    }
}

/// Session file format version.
///
/// 5 = current: each window carries `layout` (scrolling-layout slot + tile
/// size). Files from versions 1-4 still load (missing keys deserialize to
/// their defaults), so this constant is descriptive, not enforced: it stamps
/// what a file was written with; nothing is rejected based on it.
pub const SESSION_FORMAT_VERSION: u32 = 5;

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionedSession {
    pub version: u32,
    pub windows: Vec<SavedWindow>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionData {
    Versioned(VersionedSession),
    Legacy(Vec<SavedWindow>),
}

impl SessionData {
    pub fn into_windows(self) -> Vec<SavedWindow> {
        match self {
            Self::Versioned(v) => v.windows,
            Self::Legacy(windows) => windows,
        }
    }

    pub const fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy(_))
    }
}

/// Atomic file write: temp file + fsync of file contents + rename + fsync of
/// the parent directory. The parent-dir fsync is what makes the rename itself
/// durable across a power loss; without it the machine can come back with the
/// previous session file (or no file at all).
pub fn atomic_write(file_path: &Path, data: &str) -> Result<()> {
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
        let dir = fs::File::open(parent).with_context(|| {
            format!(
                "Failed to open parent directory {} for fsync",
                parent.display()
            )
        })?;
        dir.sync_all()
            .context("Failed to fsync parent directory after rename")?;
    }

    Ok(())
}
pub fn dedupe_single_instance_windows(
    windows: Vec<SavedWindow>,
    single_instance_apps: &[String],
) -> Vec<SavedWindow> {
    let single: std::collections::HashSet<&String> = single_instance_apps.iter().collect();
    // Per (app, pid): two different single-instance apps can legitimately
    // share a process id (e.g. wrappers); only duplicate surfaces of the
    // SAME app collapse.
    let mut seen_pids: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();
    windows
        .into_iter()
        .filter(|w| {
            if !single.contains(&w.app_id) {
                return true;
            }
            w.pid
                .is_none_or(|pid| seen_pids.insert((w.app_id.clone(), pid)))
        })
        .collect()
}

pub fn filter_skipped_windows(windows: Vec<SavedWindow>, skip_apps: &[String]) -> Vec<SavedWindow> {
    windows
        .into_iter()
        .filter(|w| !skip_apps.iter().any(|s| s == &w.app_id))
        .collect()
}

/// Captures the current niri state into the serialized session JSON.
pub async fn capture_session_json(app_config: &AppConfig) -> Result<String> {
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
            layout: Some(SavedWindowLayout::from_niri(&window.layout)),
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

    if app_config.terminal_state.enabled {
        let terminals_matched = saved_windows.iter().any(|w| {
            app_config
                .terminal_state
                .terminal_app_ids
                .contains(&w.app_id)
        });
        if !terminals_matched {
            warn!(
                "terminal_state is enabled but no terminal windows matched terminal_app_ids — nothing to recover inside terminals"
            );
        }
    }

    let session = VersionedSession {
        version: SESSION_FORMAT_VERSION,
        windows: saved_windows,
    };
    serde_json::to_string_pretty(&session).context("Failed to serialize window data")
}

pub async fn save_session_with_terminal_state(
    file_path: &Path,
    app_config: &AppConfig,
) -> Result<()> {
    let json_data = capture_session_json(app_config).await?;
    atomic_write(file_path, &json_data).context("Failed to write session file")?;
    info!("Session saved to {}", file_path.display());
    Ok(())
}

/// Reads and parses the session file.
///
/// Returns `Ok(None)` when there is no usable session data (missing file, or
/// a corrupt file with no valid backup): a fresh session should then be
/// seeded from the current niri state.
pub fn load_session_windows(file_path: &Path) -> Result<Option<Vec<SavedWindow>>> {
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

pub async fn save_session_with_backup(
    file_path: &Path,
    config: &Config,
    app_config: &AppConfig,
) -> Result<()> {
    let json_data = capture_session_json(app_config).await?;

    // Layout-hash throttling: when the captured state is byte-identical to
    // what is already on disk, skip the backup rotation and the write. An
    // unchanged desktop then produces neither backup churn nor journal
    // noise.
    if fs::read_to_string(file_path).is_ok_and(|existing| existing == json_data) {
        return Ok(());
    }

    create_backup(file_path)?;

    if let Some(session_dir) = file_path.parent() {
        cleanup_old_backups(session_dir, config.max_backup_count)?;
    }

    atomic_write(file_path, &json_data).context("Failed to write session file")?;
    info!("Session saved to {}", file_path.display());
    Ok(())
}

pub fn create_backup(file_path: &Path) -> Result<()> {
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
pub fn find_latest_valid_backup(file_path: &Path) -> Option<(std::path::PathBuf, SessionData)> {
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

pub fn cleanup_old_backups(session_dir: &Path, keep_count: usize) -> Result<()> {
    let mut backups: Vec<_> = fs::read_dir(session_dir)?
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("bak"))
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
/// Copies the session file and every backup into `dest_dir` for safekeeping.
pub fn run_export(session_file: &Path, dest_dir: &Path) -> Result<()> {
    if !session_file.exists() {
        bail!(
            "nothing to export: no session file at {}",
            session_file.display()
        );
    }
    fs::create_dir_all(dest_dir)
        .with_context(|| format!("Failed to create export directory {}", dest_dir.display()))?;
    fs::copy(session_file, dest_dir.join("session.json"))
        .context("Failed to export session file")?;

    let mut backups = 0usize;
    if let Some(src_dir) = session_file.parent() {
        for entry in fs::read_dir(src_dir)
            .with_context(|| format!("Failed to read {}", src_dir.display()))?
        {
            let Ok(entry) = entry else { continue };
            let is_bak = entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("bak"));
            if !is_bak {
                continue;
            }
            let dest = dest_dir.join(entry.file_name());
            if fs::copy(entry.path(), dest).is_ok() {
                backups = backups.saturating_add(1);
            }
        }
    }
    info!(
        "Exported session + {backups} backup(s) to {}",
        dest_dir.display()
    );
    Ok(())
}

/// Validates a previously exported directory and installs its session file
/// as the current one. Refuses invalid session data, and backs up whatever
/// is currently on disk first.
pub fn run_import(archive_dir: &Path, session_file: &Path) -> Result<()> {
    let source = archive_dir.join("session.json");
    let contents = fs::read_to_string(&source)
        .with_context(|| format!("Failed to read exported session {}", source.display()))?;
    let windows = serde_json::from_str::<SessionData>(&contents)
        .with_context(|| {
            format!(
                "{} is not valid session data — refusing to import it over the live session",
                source.display()
            )
        })?
        .into_windows();
    info!(
        "Importing {} window(s) from {}",
        windows.len(),
        source.display()
    );

    create_backup(session_file)?;
    atomic_write(session_file, &contents)?;

    let mut backups = 0usize;
    if let Some(dest_dir) = session_file.parent() {
        for entry in fs::read_dir(archive_dir)
            .with_context(|| format!("Failed to read {}", archive_dir.display()))?
        {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let is_bak = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("bak"));
            if !is_bak {
                continue;
            }
            if let Some(name) = path.file_name() {
                if fs::copy(&path, dest_dir.join(name)).is_ok() {
                    backups = backups.saturating_add(1);
                }
            }
        }
    }
    info!(
        "Imported session from {} (plus {backups} backup(s))",
        archive_dir.display()
    );
    Ok(())
}
