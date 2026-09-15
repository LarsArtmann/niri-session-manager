//! Turning saved windows back into commands, including terminal-specific
//! restarts that re-open the foreground child in its working directory.

use std::collections::HashMap;
use std::fs;
use tokio::task::spawn_blocking;

use crate::config::TerminalStateConfig;
use crate::proc;
use crate::session::SavedWindow;

pub fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(target_os = "linux")]
pub fn get_shell_from_passwd() -> Option<String> {
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

pub fn get_restore_shell() -> String {
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
pub enum CwdFlag {
    Separated(&'static str),
    Joined(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalProfile {
    Kitty,
    Foot,
    Wezterm,
    Ghostty,
    Alacritty,
    Generic,
}

impl TerminalProfile {
    pub fn from_executable(name: &str) -> Self {
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

    pub fn from_args(args: &[String]) -> Self {
        args.iter()
            .rev()
            .map(|arg| Self::from_executable(arg))
            .find(|profile| *profile != Self::Generic)
            .unwrap_or(Self::Generic)
    }

    pub const fn needs_start_subcommand(self) -> bool {
        matches!(self, Self::Wezterm)
    }

    pub const fn cwd_flag(self) -> CwdFlag {
        match self {
            Self::Kitty => CwdFlag::Separated("--directory"),
            Self::Foot | Self::Alacritty | Self::Generic => {
                CwdFlag::Separated("--working-directory")
            }
            Self::Wezterm => CwdFlag::Separated("--cwd"),
            Self::Ghostty => CwdFlag::Joined("--working-directory="),
        }
    }

    pub const fn cmd_flag(self) -> Option<&'static str> {
        match self {
            Self::Kitty | Self::Foot => None,
            Self::Wezterm => Some("--"),
            Self::Ghostty | Self::Alacritty | Self::Generic => Some("-e"),
        }
    }
}

pub fn build_terminal_restore_command(
    launch_prefix: &[String],
    profile: TerminalProfile,
    child_cmd: &[String],
    working_dir: Option<&str>,
) -> Vec<String> {
    let mut cmd: Vec<String> = launch_prefix.to_vec();

    if profile.needs_start_subcommand() {
        cmd.push("start".to_string());
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let effective_cwd = working_dir.filter(|cwd| !cwd.is_empty() && *cwd != home);

    if let Some(cwd) = effective_cwd {
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

pub fn build_spawn_command(
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
                return build_terminal_restore_command(
                    &mapped,
                    profile,
                    &args,
                    ts.child_cwd.as_deref(),
                );
            }
        }
    }

    mapped
}

pub async fn resolve_terminal_state(
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
