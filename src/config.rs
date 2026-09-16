//! The two configuration surfaces: CLI flags (`Config`) and the TOML app
//! config (`AppConfig`), with validation for both.

use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_RESTORE_WINDOWS_DEFAULT: usize = 100;
pub const fn default_enabled() -> bool {
    true
}
pub fn default_terminal_app_ids() -> Vec<String> {
    vec![
        "kitty".into(),
        "foot".into(),
        "org.wezfurlong.wezterm".into(),
        "com.mitchellh.ghostty".into(),
        // Both spellings: niri reports alacritty's app_id as "Alacritty"
        // (capital A — verified live 2026-09-16); lowercase covers builds
        // that report it verbatim from the binary name.
        "alacritty".into(),
        "Alacritty".into(),
    ]
}
pub fn default_shell_names() -> Vec<String> {
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
pub fn default_helper_names() -> Vec<String> {
    vec!["kitten".into()]
}
pub const fn default_max_walk_depth() -> u32 {
    20
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalStateConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_terminal_app_ids")]
    pub terminal_app_ids: Vec<String>,
    #[serde(default = "default_shell_names")]
    pub shell_names: Vec<String>,
    #[serde(default = "default_helper_names")]
    pub helper_names: Vec<String>,
    #[serde(default = "default_max_walk_depth")]
    pub max_walk_depth: u32,
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
pub struct SingleInstanceAppsConfig {
    #[serde(default)]
    pub apps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkipAppsConfig {
    #[serde(default)]
    pub apps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub app_mappings: HashMap<String, Vec<String>>,
    #[serde(default, rename = "single_instance_apps")]
    pub single_instance: SingleInstanceAppsConfig,
    #[serde(default, rename = "skip_apps")]
    pub skip_apps: SkipAppsConfig,
    #[serde(default)]
    pub terminal_state: TerminalStateConfig,
}
pub const DEFAULT_APP_CONFIG_TOML: &str = r#"# Niri Session Manager Configuration

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
# niri reports alacritty's app_id capitalized; map both spellings to the binary
"Alacritty" = ["alacritty"]
"alacritty" = ["alacritty"]
"org.wezfurlong.wezterm" = ["wezterm"]

# Commands with arguments
"firefox-custom" = ["firefox", "--profile", "default-release"]

# Terminal state recovery — restore running commands inside terminals
[terminal_state]
enabled = true
terminal_app_ids = ["kitty", "foot", "org.wezfurlong.wezterm", "com.mitchellh.ghostty", "alacritty", "Alacritty"]
shell_names = ["fish", "bash", "zsh", "sh", "dash", "-fish", "-bash", "-zsh", "-sh", "sudo", "doas"]
helper_names = ["kitten"]
max_walk_depth = 20
"#;
pub fn default_app_config_path() -> Result<PathBuf> {
    let mut config_path = dirs::config_dir().context("Failed to locate config directory")?;
    config_path.push("niri-session-manager");
    config_path.push("config.toml");
    Ok(config_path)
}

/// Loads the app config. With the default path, a missing file is created
/// from the built-in template. With an explicit `--config-file`, a missing
/// file is an error: the user asked for that exact file.
/// Rejects config values that would cause silent misbehavior at restore
/// time.
pub fn validate_app_config(app_config: &AppConfig) -> Result<()> {
    if app_config.terminal_state.max_walk_depth == 0 {
        bail!(
            "terminal_state.max_walk_depth must be at least 1 (0 would never walk to any child process)"
        );
    }
    Ok(())
}
pub fn load_app_config(explicit_path: Option<&Path>) -> Result<AppConfig> {
    let config_path = if let Some(p) = explicit_path {
        p.to_path_buf()
    } else {
        let p = default_app_config_path()?;
        if !p.exists() {
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create config directory {}", parent.display())
                })?;
            }
            fs::write(&p, DEFAULT_APP_CONFIG_TOML)
                .with_context(|| format!("Failed to write default config to {}", p.display()))?;
            return Ok(AppConfig::default());
        }
        p
    };

    let config_str = fs::read_to_string(&config_path).context("Failed to read config file")?;

    let config: AppConfig = toml::from_str(&config_str).context("Failed to parse config file")?;
    validate_app_config(&config)?;
    Ok(config)
}
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
// The bools are clap flags, not state modeling: each is an independent,
// user-facing switch, which is exactly what a CLI struct is for.
#[allow(clippy::struct_excessive_bools)]
pub struct Config {
    /// Seconds between polling-fallback saves when the niri event stream is down
    #[arg(long, default_value = "15")]
    pub save_interval: u64,

    /// How many timestamped session backups to keep before pruning the oldest
    #[arg(long, default_value = "5")]
    pub max_backup_count: usize,

    /// Seconds to wait for a spawned window to appear before the spawn counts as failed
    #[arg(long, default_value = "5")]
    pub spawn_timeout: u64,

    /// Restore attempts per window before giving up (retries happen within one restore)
    #[arg(long, default_value = "3")]
    pub retry_attempts: u32,

    /// Base delay in seconds between restore retries; doubles per attempt, capped at 30 (0 retries immediately)
    #[arg(long, default_value = "2")]
    pub retry_delay: u64,

    /// Sanity cap on how many windows a single restore may spawn
    #[arg(long, default_value_t = MAX_RESTORE_WINDOWS_DEFAULT)]
    pub max_restore_windows: usize,

    /// Preview what would be restored without actually spawning windows or modifying files
    #[arg(long, default_value = "false")]
    pub dry_run: bool,

    /// Override the app-config path (default: XDG config dir, config.toml)
    #[arg(long = "config-file", value_name = "PATH")]
    pub app_config_path: Option<PathBuf>,

    /// Restore the saved session, then exit (no periodic saving)
    #[arg(long, conflicts_with = "save_only")]
    pub restore: bool,

    /// Skip the boot restore and only run periodic saving
    #[arg(long, conflicts_with = "restore")]
    pub save_only: bool,

    /// Save the current session once, then exit (used by the suspend hook)
    #[arg(
        long,
        conflicts_with = "restore",
        conflicts_with = "save_only",
        conflicts_with = "dry_run"
    )]
    pub save_once: bool,

    /// Check service health (niri reachable, session file, restore marker) and exit
    #[arg(
        long,
        conflicts_with = "restore",
        conflicts_with = "save_only",
        conflicts_with = "save_once",
        conflicts_with = "dry_run"
    )]
    pub health_check: bool,

    /// Probe the live IPC protocol: version round-trip, then inspect the first
    /// event-stream burst lines for unparsable (protocol-drift) events, and exit
    #[arg(
        long,
        conflicts_with = "restore",
        conflicts_with = "save_only",
        conflicts_with = "save_once",
        conflicts_with = "dry_run",
        conflicts_with = "health_check"
    )]
    pub protocol_probe: bool,

    /// Copy session.json plus all backups into DIR (created if missing), then exit
    #[arg(long, value_name = "DIR")]
    pub export_to: Option<PathBuf>,

    /// Validate and import session.json (plus backups) from DIR, backing up the
    /// current session first, then exit
    #[arg(long, value_name = "DIR", conflicts_with = "export_to")]
    pub import_from: Option<PathBuf>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Boot restore (marker-gated), then reactive saving until shutdown.
    Normal,
    /// Only restore, then exit.
    RestoreOnly,
    /// Skip restore, only run saving until shutdown.
    SaveOnly,
    /// Save once, then exit.
    SaveOnce,
    /// Check health, then exit.
    HealthCheck,
    /// Inspect the live IPC protocol for drift, then exit.
    ProtocolProbe,
}
impl Config {
    pub const fn run_mode(&self) -> RunMode {
        if self.restore {
            RunMode::RestoreOnly
        } else if self.save_only {
            RunMode::SaveOnly
        } else if self.save_once {
            RunMode::SaveOnce
        } else if self.health_check {
            RunMode::HealthCheck
        } else if self.protocol_probe {
            RunMode::ProtocolProbe
        } else {
            RunMode::Normal
        }
    }

    /// Rejects nonsensical CLI values that would cause silent misbehavior.
    pub fn validate(&self) -> Result<()> {
        if self.save_interval == 0 {
            bail!("--save-interval must be at least 1 minute");
        }
        if self.max_backup_count == 0 {
            bail!("--max-backup-count must be at least 1");
        }
        if self.spawn_timeout == 0 {
            bail!("--spawn-timeout must be at least 1 second");
        }
        if self.max_restore_windows == 0 {
            bail!("--max-restore-windows must be at least 1");
        }
        Ok(())
    }
}
