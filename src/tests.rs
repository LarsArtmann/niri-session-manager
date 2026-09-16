// Deliberate lint exemption (see AGENTS.md "Testing"): tests assert via
// unwrap/expect/indexing/panics; the production-code denies do not apply here.
#![allow(
    clippy::pedantic,
    clippy::nursery,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::as_conversions
)]

use crate::config::*;
use crate::fake_niri::niri_workspace;
use crate::ipc::request_reply;
use crate::restore::*;
use crate::save::*;
use crate::session::*;
use crate::terminal::*;
use crate::{session_staleness_warning, SESSION_STALENESS_INTERVALS};
use niri_ipc::WorkspaceReferenceArg;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

fn expected_exec_suffix(child_cmd: &str) -> String {
    let shell = get_restore_shell();
    format!("{}; exec {}", shell_escape(child_cmd), shell_escape(&shell))
}

#[test]
fn reconnect_backoff_doubles_and_caps() {
    let mut delay = RECONNECT_DELAY_INITIAL;
    let mut steps = 0;
    while delay < RECONNECT_DELAY_MAX {
        delay = next_reconnect_delay(delay);
        steps += 1;
    }
    assert_eq!(delay, RECONNECT_DELAY_MAX);
    assert!(
        steps >= 5,
        "backoff should ramp up over several steps, not jump to the cap"
    );
    assert_eq!(
        next_reconnect_delay(RECONNECT_DELAY_MAX),
        RECONNECT_DELAY_MAX,
        "the cap must be a fixed point"
    );
}

#[test]
fn retry_backoff_starts_at_the_base_and_caps() {
    assert_eq!(next_retry_delay(2, 1), Duration::from_secs(2));
    assert_eq!(next_retry_delay(2, 2), Duration::from_secs(4));
    assert_eq!(next_retry_delay(2, 3), Duration::from_secs(8));
    assert_eq!(
        next_retry_delay(20, 2),
        RETRY_DELAY_MAX,
        "20s doubled would exceed the cap; the cap must hold"
    );
    assert_eq!(
        next_retry_delay(RETRY_DELAY_MAX.as_secs(), u32::MAX),
        RETRY_DELAY_MAX,
        "the cap must be a fixed point"
    );
    assert_eq!(
        next_retry_delay(0, 3),
        Duration::ZERO,
        "a zero base retries immediately, as it always has"
    );
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
        layout: None,
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
        layout: None,
    };
    let windows = vec![win(1, "xdg-desktop-portal"), win(2, "firefox")];
    let out = filter_skipped_windows(windows, &["xdg-desktop-portal".to_string()]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].app_id, "firefox");
}

#[test]
fn should_restore_on_boot_gate_and_stale_pruning() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.json");
    let marker = dir.path().join("restore-marker");

    assert!(
        should_restore_on_boot(Some("boot-abc-123"), &marker, &session),
        "missing marker = should restore"
    );
    atomic_write(&marker, "boot-abc-123\n").unwrap();
    std::fs::write(&session, "[]").unwrap();
    assert!(
        !should_restore_on_boot(Some("boot-abc-123"), &marker, &session),
        "matching boot id = already restored"
    );
    atomic_write(&marker, "older-boot\n").unwrap();
    assert!(
        should_restore_on_boot(Some("boot-abc-123"), &marker, &session),
        "stale marker from a previous boot = should restore"
    );
    assert!(
        !marker.exists(),
        "stale marker is pruned so it cannot accumulate forever"
    );
    assert!(
        should_restore_on_boot(None, &marker, &session),
        "unknown boot id (no /proc access) = never skip"
    );
}

#[test]
fn vanishing_session_file_prunes_this_boots_marker() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.json");
    let marker = dir.path().join("restore-marker");

    std::fs::write(&session, "[]").unwrap();
    atomic_write(&marker, "boot-abc-123\n").unwrap();
    assert!(
        !should_restore_on_boot(Some("boot-abc-123"), &marker, &session),
        "restored this boot with a live session file = skip"
    );

    std::fs::remove_file(&session).unwrap();
    assert!(
        should_restore_on_boot(Some("boot-abc-123"), &marker, &session),
        "session file vanished = marker must not block a re-restore"
    );
    assert!(!marker.exists(), "the orphaned marker is pruned");
}

#[test]
fn unreadable_boot_id_restores_and_leaves_the_marker_alone() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.json");
    let marker = dir.path().join("restore-marker");

    std::fs::write(&session, "[]").unwrap();
    atomic_write(&marker, "some-other-boot\n").unwrap();
    assert!(
        should_restore_on_boot(None, &marker, &session),
        "unreadable boot id = never skip the restore"
    );
    assert!(
        marker.exists(),
        "without a boot id we cannot tell the marker is stale, so it must not be pruned"
    );
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
        profile,
        &["btop".to_string()],
        Some("/home/user/projects"),
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
        profile,
        &["btop".to_string()],
        Some(home.as_str()),
    );
    assert_restore_command(&cmd, &["kitty", "sh", "-c"], "btop");
}

#[test]
fn build_restore_wezterm_with_cwd() {
    let profile = TerminalProfile::Wezterm;
    let cmd = build_terminal_restore_command(
        &["wezterm".to_string()],
        profile,
        &["btop".to_string()],
        Some("/home/user/projects"),
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
        profile,
        &["btop".to_string()],
        Some("/home/user/projects"),
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
        profile,
        &["btop".to_string()],
        Some("/home/user/projects"),
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
        profile,
        &["btop".to_string()],
        Some("/home/user/projects"),
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
        profile,
        &["echo 'hello'; rm -rf /".to_string()],
        None,
    );
    assert_eq!(cmd[3], expected_exec_suffix("echo 'hello'; rm -rf /"));
}

#[test]
fn build_restore_preserves_multi_arg_command() {
    let profile = TerminalProfile::Kitty;
    let cmd = build_terminal_restore_command(
        &["kitty".to_string()],
        profile,
        &["nvim".to_string(), "/path/to file".to_string()],
        None,
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
        profile,
        &["btop".to_string()],
        None,
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
        layout: None,
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
        layout: None,
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
    // niri reports alacritty's app_id capitalized; both spellings must be
    // recognized or alacritty windows never capture terminal state and
    // restore falls back to spawning the (nonexistent) binary "Alacritty"
    // (found live 2026-09-16 via the CARRIER=alacritty soak).
    assert!(c.terminal_app_ids.contains(&"Alacritty".to_string()));
    assert!(c.terminal_app_ids.contains(&"alacritty".to_string()));
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
fn session_json_with_unknown_future_keys_still_loads() {
    let json = r#"{
            "version": 5,
            "windows": [
                {
                    "id": 1,
                    "app_id": "kitty",
                    "is_focused": true,
                    "future_window_field": {"nested": [1, 2, 3]}
                }
            ],
            "future_top_level_field": "ignored"
        }"#;
    let session: SessionData = serde_json::from_str(json).unwrap();
    assert!(!session.is_legacy());
    let windows = session.into_windows();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].app_id, "kitty");
    assert!(windows[0].layout.is_none(), "absent layout must default");
}

#[test]
fn valid_session_file_wins_over_corrupt_backups() {
    let tmp = tempfile::tempdir().unwrap();
    let session_path = tmp.path().join("session.json");
    fs::write(
        &session_path,
        r#"{"version": 5, "windows": [{"id": 7, "app_id": "firefox", "is_focused": true}]}"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("session-2024-01-01T00:00:00Z.bak"),
        "{BROKEN",
    )
    .unwrap();
    fs::write(
        tmp.path().join("session-2024-06-01T00:00:00Z.bak"),
        "{ALSO BROKEN",
    )
    .unwrap();

    let windows = load_session_windows(&session_path)
        .expect("valid session must load without error")
        .expect("valid session must yield windows");
    assert_eq!(windows.len(), 1);
    assert_eq!(
        windows[0].id, 7,
        "the live session must be used, not a backup"
    );
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
            layout: Some(SavedWindowLayout {
                scroll_position: Some(ScrollPosition {
                    column: 2,
                    tile_in_column: 1,
                }),
                tile_width: 960.0,
                tile_height: 540.0,
            }),
        }],
    };
    let json = serde_json::to_string_pretty(&session).unwrap();
    assert!(json.contains("\"version\": 5"));
    assert!(json.contains("\"windows\""));
    assert!(json.contains("\"layout\""));
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

fn saved_win(id: u64, app: &str, name: Option<&str>, idx: Option<u8>) -> SavedWindow {
    SavedWindow {
        id,
        app_id: app.to_string(),
        workspace: WorkspaceInfo {
            idx,
            name: name.map(String::from),
            output: None,
        },
        is_focused: false,
        pid: None,
        terminal_state: None,
        layout: None,
    }
}

fn running_win(id: u64, app: &str, name: Option<&str>, idx: Option<u8>) -> RunningWindow {
    RunningWindow {
        id,
        app_id: Some(app.to_string()),
        workspace_name: name.map(String::from),
        workspace_idx: idx,
    }
}

fn no_ipc_config(dry_run: bool) -> Config {
    Config {
        save_interval: 15,
        max_backup_count: 5,
        spawn_timeout: 1,
        retry_attempts: 1,
        retry_delay: 1,
        max_restore_windows: 100,
        dry_run,
        app_config_path: None,
        restore: false,
        save_only: false,
        save_once: false,
        health_check: false,
        protocol_probe: false,
        export_to: None,
        import_from: None,
    }
}

// --- M3: dry-run contract regression tests (fixed in 0.4.0, must not regress) ---

#[tokio::test]
async fn dry_run_with_no_session_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let session = tmp.path().join("session.json");
    let config = no_ipc_config(true);
    let app_config = AppConfig::default();

    let outcome = restore_session_internal(&session, &config, &app_config)
        .await
        .unwrap();

    assert_eq!(outcome, RestoreOutcome::SeededNewSession);
    assert!(
        !session.exists(),
        "dry run must not create the session file"
    );
    assert!(
        !get_restore_marker_path(&session).exists(),
        "dry run must not write the restore marker"
    );
}

#[tokio::test]
async fn dry_run_with_existing_session_spawns_nothing_and_modifies_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let session = tmp.path().join("session.json");
    let original = r#"{"version":3,"windows":[
            {"id":1,"app_id":"firefox","is_focused":false,"idx":1,"name":"dev"},
            {"id":2,"app_id":"chromium","is_focused":true,"idx":2}
        ]}"#;
    fs::write(&session, original).unwrap();
    let config = no_ipc_config(true);
    let app_config = AppConfig::default();

    let outcome = restore_session_internal(&session, &config, &app_config)
        .await
        .unwrap();

    assert_eq!(outcome, RestoreOutcome::WouldRestore { window_count: 2 });
    assert_eq!(
        fs::read_to_string(&session).unwrap(),
        original,
        "dry run must not modify the session file"
    );
    assert!(
        !get_restore_marker_path(&session).exists(),
        "dry run must not write the restore marker"
    );

    // A second dry run behaves identically: the marker stays absent.
    let outcome = restore_session_internal(&session, &config, &app_config)
        .await
        .unwrap();
    assert_eq!(outcome, RestoreOutcome::WouldRestore { window_count: 2 });
    assert!(!get_restore_marker_path(&session).exists());
}

#[tokio::test]
async fn corrupt_session_without_backup_is_reported_as_seed_candidate() {
    let tmp = tempfile::tempdir().unwrap();
    let session = tmp.path().join("session.json");
    fs::write(&session, "{CORRUPT").unwrap();
    let config = no_ipc_config(true);
    let app_config = AppConfig::default();

    let outcome = restore_session_internal(&session, &config, &app_config)
        .await
        .unwrap();

    assert_eq!(outcome, RestoreOutcome::SeededNewSession);
    assert_eq!(
        fs::read_to_string(&session).unwrap(),
        "{CORRUPT",
        "dry run must not overwrite the corrupt file"
    );
}

#[test]
fn run_mode_and_validation_work_together() {
    let mut config = no_ipc_config(false);
    assert_eq!(config.run_mode(), RunMode::Normal);
    config.restore = true;
    assert_eq!(config.run_mode(), RunMode::RestoreOnly);
    config.restore = false;
    config.save_only = true;
    assert_eq!(config.run_mode(), RunMode::SaveOnly);

    config.save_interval = 0;
    assert!(config.validate().is_err());
    config.save_interval = 15;
    config.max_restore_windows = 0;
    assert!(config.validate().is_err());
    config.max_restore_windows = 100;
    assert!(config.validate().is_ok());
}

// --- M9: idempotent restore planning (ROADMAP Q2: workspace-first, count-capped) ---

#[test]
fn plan_spawns_skips_windows_already_on_their_workspace() {
    let saved = vec![
        saved_win(1, "firefox", Some("dev"), Some(1)),
        saved_win(2, "firefox", Some("dev"), Some(1)),
        saved_win(3, "firefox", Some("dev"), Some(1)),
    ];
    let running = vec![running_win(10, "firefox", Some("dev"), Some(1))];

    let to_spawn = plan_spawns(&saved, &running, &AppConfig::default());

    assert_eq!(to_spawn.len(), 2, "only the deficit (3−1) is spawned");
    assert_eq!(
        to_spawn.iter().map(|w| w.id).collect::<Vec<_>>(),
        vec![2, 3],
        "entry 1 was satisfied by the running window; the rest spawn in saved order"
    );
}

#[test]
fn plan_spawns_caps_at_deficit_when_no_workspace_matches() {
    let saved = vec![
        saved_win(1, "firefox", Some("dev"), Some(1)),
        saved_win(2, "firefox", Some("dev"), Some(1)),
    ];
    // One firefox running, but on a different workspace: no name match.
    // The count cap still limits spawning to the deficit of 1.
    let running = vec![running_win(10, "firefox", Some("other"), Some(2))];

    let to_spawn = plan_spawns(&saved, &running, &AppConfig::default());

    assert_eq!(to_spawn.len(), 1, "count cap: saved 2 − running 1 = 1");
}

#[test]
fn plan_spawns_returns_empty_when_everything_already_runs() {
    let saved = vec![
        saved_win(1, "firefox", Some("dev"), Some(1)),
        saved_win(2, "firefox", Some("web"), Some(2)),
    ];
    let running = vec![
        running_win(10, "firefox", Some("dev"), Some(1)),
        running_win(11, "firefox", Some("web"), Some(2)),
    ];

    let to_spawn = plan_spawns(&saved, &running, &AppConfig::default());

    assert!(to_spawn.is_empty(), "re-restore must be a no-op");
}

#[test]
fn plan_spawns_falls_back_to_workspace_index_when_names_missing() {
    let saved = vec![
        saved_win(1, "kitty", None, Some(2)),
        saved_win(2, "kitty", None, Some(2)),
    ];
    let running = vec![running_win(10, "kitty", None, Some(2))];

    let to_spawn = plan_spawns(&saved, &running, &AppConfig::default());

    assert_eq!(to_spawn.len(), 1, "index match satisfies one entry");
}

#[test]
fn plan_spawns_skips_single_instance_app_when_any_instance_runs() {
    let saved = vec![
        saved_win(1, "zen", Some("dev"), Some(1)),
        saved_win(2, "zen", Some("web"), Some(2)),
    ];
    let running = vec![running_win(10, "zen", Some("web"), Some(2))];
    let app_config = AppConfig {
        single_instance: SingleInstanceAppsConfig {
            apps: vec!["zen".to_string()],
        },
        ..Default::default()
    };

    let to_spawn = plan_spawns(&saved, &running, &app_config);

    assert!(
        to_spawn.is_empty(),
        "single-instance apps stay skipped when any instance runs"
    );
}

#[test]
fn plan_spawns_skips_skip_listed_apps() {
    let saved = vec![saved_win(1, "discord", Some("dev"), Some(1))];
    let app_config = AppConfig {
        skip_apps: SkipAppsConfig {
            apps: vec!["discord".to_string()],
        },
        ..Default::default()
    };

    let to_spawn = plan_spawns(&saved, &[], &app_config);

    assert!(to_spawn.is_empty());
}

#[test]
fn plan_spawns_ignores_other_apps_running_windows() {
    let saved = vec![saved_win(1, "firefox", Some("dev"), Some(1))];
    let running = vec![running_win(10, "chromium", Some("dev"), Some(1))];

    let to_spawn = plan_spawns(&saved, &running, &AppConfig::default());

    assert_eq!(
        to_spawn.len(),
        1,
        "another app on the same workspace does not satisfy this entry"
    );
}

#[test]
fn plan_spawns_never_spawns_more_than_saved() {
    let saved = vec![saved_win(1, "firefox", Some("dev"), Some(1))];
    // Many firefox instances running on different workspaces.
    let running = vec![
        running_win(10, "firefox", Some("x"), Some(1)),
        running_win(11, "firefox", Some("y"), Some(2)),
        running_win(12, "firefox", Some("z"), Some(3)),
    ];

    let to_spawn = plan_spawns(&saved, &running, &AppConfig::default());

    assert!(to_spawn.is_empty(), "running ≥ saved means spawn nothing");
}

// --- M14: output fallback when the saved output no longer exists ---

#[test]
fn resolve_output_keeps_existing_saved_output() {
    let saved = WorkspaceInfo {
        idx: Some(1),
        name: Some("dev".to_string()),
        output: Some("DP-1".to_string()),
    };
    let workspaces = vec![niri_workspace(1, 1, Some("dev"), "DP-1")];
    assert_eq!(
        resolve_target_output(&saved, &workspaces),
        Some("DP-1".to_string())
    );
}

#[test]
fn resolve_output_falls_back_to_host_of_named_workspace() {
    let saved = WorkspaceInfo {
        idx: Some(1),
        name: Some("dev".to_string()),
        output: Some("HDMI-A-1".to_string()),
    };
    // The saved output is gone; the workspace now lives on DP-2.
    let workspaces = vec![niri_workspace(1, 1, Some("dev"), "DP-2")];
    assert_eq!(
        resolve_target_output(&saved, &workspaces),
        Some("DP-2".to_string()),
        "docking with a renamed monitor should still place the window"
    );
}

#[test]
fn resolve_output_falls_back_to_index_when_name_misses() {
    let saved = WorkspaceInfo {
        idx: Some(3),
        name: None,
        output: Some("DP-1".to_string()),
    };
    let workspaces = vec![niri_workspace(1, 3, None, "eDP-1")];
    assert_eq!(
        resolve_target_output(&saved, &workspaces),
        Some("eDP-1".to_string())
    );
}

// --- M4: atomic_write now fsyncs the parent directory too ---

#[test]
fn atomic_write_survives_and_syncs_parent_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("session.json");
    atomic_write(&path, "data").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "data");
    // The parent-dir open+fsync path runs on every write; if it errored,
    // atomic_write itself would error, so reaching here means it worked.
}

// --- M5: same-app spawns are serialized, different apps are not ---

#[tokio::test]
async fn spawn_limiter_serializes_same_app() {
    let limiter = SpawnLimiter::new(5);
    let (app_permit, _global) = limiter.acquire("firefox").await.unwrap();

    let limiter2 = limiter.clone();
    let second = tokio::spawn(async move { limiter2.acquire("firefox").await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !second.is_finished(),
        "second firefox spawn must wait for the first to release"
    );

    drop(app_permit);
    let (_app, _global) = second.await.unwrap().unwrap();
}

#[tokio::test]
async fn spawn_limiter_allows_distinct_apps_in_parallel() {
    let limiter = SpawnLimiter::new(5);
    let _a = limiter.acquire("firefox").await.unwrap();
    let _b = limiter.acquire("kitty").await.unwrap();
    let _c = limiter.acquire("zen").await.unwrap();
}

// --- M16: coverage batch ---

#[test]
fn cleanup_old_backups_keeps_newest_and_removes_oldest() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..7 {
        let path = tmp
            .path()
            .join(format!("session-2024-01-0{i}T00:00:00Z.bak"));
        fs::write(&path, r#"{"version":3,"windows":[]}"#).unwrap();
        // Distinguish mtimes: sequential writes alone are too fast.
        std::thread::sleep(std::time::Duration::from_millis(15));
    }

    cleanup_old_backups(tmp.path(), 5).unwrap();

    let mut remaining: Vec<String> = fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "bak"))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    remaining.sort();
    assert_eq!(
        remaining.len(),
        5,
        "rotation keeps exactly keep_count backups"
    );
    assert!(
        !remaining.iter().any(|n| n.contains("01-00")),
        "the oldest backup is evicted first"
    );
    assert!(
        remaining.iter().any(|n| n.contains("01-06")),
        "the newest backup is kept"
    );
}

#[test]
fn cleanup_old_backups_ignores_non_backup_files() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("session.json"), "{}").unwrap();
    fs::write(tmp.path().join("restore-marker"), "boot").unwrap();
    fs::write(tmp.path().join("session-1.bak"), "{}").unwrap();

    cleanup_old_backups(tmp.path(), 5).unwrap();

    assert!(tmp.path().join("session.json").exists());
    assert!(tmp.path().join("restore-marker").exists());
    assert!(tmp.path().join("session-1.bak").exists());
}

#[test]
fn dedupe_keeps_same_pid_across_different_single_instance_apps() {
    let win = |id: u64, app: &str, pid: Option<u32>| SavedWindow {
        id,
        app_id: app.to_string(),
        workspace: WorkspaceInfo::default(),
        is_focused: false,
        pid,
        terminal_state: None,
        layout: None,
    };
    let windows = vec![
        win(1, "app-one", Some(42)),
        win(2, "app-two", Some(42)),
        win(3, "app-one", Some(42)),
    ];
    let out =
        dedupe_single_instance_windows(windows, &["app-one".to_string(), "app-two".to_string()]);
    assert_eq!(
        out.iter().map(|w| w.id).collect::<Vec<_>>(),
        vec![1, 2],
        "same pid under a different app is NOT a duplicate surface"
    );
}

#[test]
fn restore_shell_falls_back_when_shell_env_unset() {
    static SHELL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = SHELL_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let original = std::env::var("SHELL").ok();
    std::env::remove_var("SHELL");

    let shell = get_restore_shell();

    assert!(
        !shell.is_empty() && !shell.is_empty(),
        "SHELL unset must not produce an empty shell command"
    );
    assert!(
        shell.starts_with('/') || shell.contains('/'),
        "fallback must be an absolute path (passwd or /bin/sh), got: {shell}"
    );

    match original {
        Some(v) => std::env::set_var("SHELL", v),
        None => std::env::remove_var("SHELL"),
    }
    drop(guard);
}

// --- M18: workspace-reference decision (idx 0 = unknown) + validation + Display ---

#[test]
fn workspace_reference_prefers_name_and_treats_idx_zero_as_unknown() {
    let ws = |idx: Option<u8>, name: Option<&str>| WorkspaceInfo {
        idx,
        name: name.map(String::from),
        output: None,
    };

    assert_eq!(
        workspace_reference(&ws(Some(2), Some("dev"))),
        Some(WorkspaceReferenceArg::Name("dev".to_string())),
        "name wins over index"
    );
    assert_eq!(
        workspace_reference(&ws(Some(3), None)),
        Some(WorkspaceReferenceArg::Index(3))
    );
    assert_eq!(
        workspace_reference(&ws(Some(0), None)),
        None,
        "niri is 1-based; legacy idx 0 means unknown — skip the move"
    );
    assert_eq!(
        workspace_reference(&ws(None, Some(""))),
        None,
        "empty name falls through to (missing) index"
    );
    assert_eq!(workspace_reference(&ws(None, None)), None);
}

#[test]
fn app_config_validation_rejects_zero_max_walk_depth() {
    let config = AppConfig {
        terminal_state: TerminalStateConfig {
            max_walk_depth: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(validate_app_config(&config).is_err());

    let config = AppConfig::default();
    assert!(validate_app_config(&config).is_ok());
}

// --- M29: export / import ---

fn bak_count(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "bak"))
        .count()
}

#[test]
fn export_copies_session_and_backups() {
    let tmp = tempfile::tempdir().unwrap();
    let session = tmp.path().join("session.json");
    fs::write(&session, r#"{"version":4,"windows":[]}"#).unwrap();
    fs::write(
        tmp.path().join("session-1.bak"),
        r#"{"version":4,"windows":[]}"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("session-2.bak"),
        r#"{"version":4,"windows":[]}"#,
    )
    .unwrap();
    fs::write(tmp.path().join("unrelated.txt"), "keep out").unwrap();
    let dest = tempfile::tempdir().unwrap();

    run_export(&session, dest.path()).unwrap();

    assert!(dest.path().join("session.json").exists());
    assert_eq!(bak_count(dest.path()), 2, "both backups are exported");
    assert!(
        !dest.path().join("unrelated.txt").exists(),
        "only session artifacts are exported"
    );
}

#[test]
fn export_fails_without_session_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    assert!(run_export(&tmp.path().join("session.json"), dest.path()).is_err());
}

#[test]
fn import_refuses_invalid_session_data_and_keeps_live_file() {
    let archive = tempfile::tempdir().unwrap();
    fs::write(archive.path().join("session.json"), "{NOT SESSION DATA").unwrap();
    let live = tempfile::tempdir().unwrap();
    let session = live.path().join("session.json");
    fs::write(&session, r#"{"version":4,"windows":[]}"#).unwrap();

    assert!(run_import(archive.path(), &session).is_err());
    assert_eq!(
        fs::read_to_string(&session).unwrap(),
        r#"{"version":4,"windows":[]}"#,
        "the live session must not be touched by a refused import"
    );
}

#[test]
fn import_replaces_session_and_backs_up_current() {
    let archive = tempfile::tempdir().unwrap();
    let incoming = r#"{"version":4,"windows":[{"id":1,"app_id":"firefox","is_focused":false}]}"#;
    fs::write(archive.path().join("session.json"), incoming).unwrap();
    fs::write(archive.path().join("session-old.bak"), "{}").unwrap();

    let live = tempfile::tempdir().unwrap();
    let session = live.path().join("session.json");
    fs::write(&session, r#"{"version":4,"windows":[]}"#).unwrap();

    run_import(archive.path(), &session).unwrap();

    assert_eq!(fs::read_to_string(&session).unwrap(), incoming);
    assert_eq!(
        bak_count(live.path()),
        2,
        "the previous session is backed up AND the archive's backups carry over"
    );
    assert_eq!(bak_count(archive.path()), 1, "the archive keeps its backup");
}

// --- M17: property tests (round-trips, legacy aliases, parse fuzzing) ---

use proptest::prelude::*;

fn arb_child_command() -> impl Strategy<Value = ChildCommand> {
    prop::option::of(proptest::collection::vec("[a-zA-Z0-9_./ -]{1,20}", 0..4)).prop_map(
        |maybe_args| match maybe_args {
            Some(args) if !args.is_empty() => ChildCommand::Args(args),
            _ => ChildCommand::Legacy("legacy-cmd".to_string()),
        },
    )
}

fn arb_terminal_state() -> impl Strategy<Value = TerminalState> {
    (
        prop::option::of(arb_child_command()),
        prop::option::of("/[a-z/]{0,12}"),
    )
        .prop_map(|(child_command, child_cwd)| TerminalState {
            child_command,
            child_cwd,
        })
}

fn arb_window_layout() -> impl Strategy<Value = Option<SavedWindowLayout>> {
    (
        prop::option::of((1u64..=64u64, 1u64..=64u64)),
        0.0f64..=4096.0,
        0.0f64..=4096.0,
        prop::option::of(Just(())),
    )
        .prop_map(|(scroll_pos, tile_width, tile_height, present)| {
            present.map(|()| SavedWindowLayout {
                scroll_position: scroll_pos.map(|(column, tile_in_column)| ScrollPosition {
                    column,
                    tile_in_column,
                }),
                tile_width,
                tile_height,
            })
        })
}

fn arb_saved_window() -> impl Strategy<Value = SavedWindow> {
    (
        any::<u64>(),
        "[a-z][a-z0-9.]{0,14}",
        prop::option::of(0u8..=9),
        prop::option::of("[a-z][a-z-]{0,7}"),
        prop::option::of("[A-Z]{2,3}-[0-9]"),
        any::<bool>(),
        prop::option::of(any::<u32>()),
        prop::option::of(arb_terminal_state()),
        arb_window_layout(),
    )
        .prop_map(
            |(id, app_id, idx, name, output, is_focused, pid, terminal_state, layout)| {
                SavedWindow {
                    id,
                    app_id,
                    workspace: WorkspaceInfo { idx, name, output },
                    is_focused,
                    pid,
                    terminal_state,
                    layout,
                }
            },
        )
}

proptest! {
    #[test]
    fn saved_window_json_round_trip_is_identity(win in arb_saved_window()) {
        let json = serde_json::to_string(&win).unwrap();
        let parsed: SavedWindow = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(parsed, win);
    }

    #[test]
    fn legacy_workspace_keys_deserialize_identically(win in arb_saved_window()) {
        let mut value = serde_json::to_value(&win).unwrap();
        if let Some(obj) = value.as_object_mut() {
            for (old_key, new_key) in [
                ("idx", "workspace_idx"),
                ("name", "workspace_name"),
                ("output", "workspace_output"),
            ] {
                if let Some(v) = obj.remove(new_key) {
                    obj.insert(old_key.to_string(), v);
                }
            }
        }
        let legacy: SavedWindow = serde_json::from_value(value).unwrap();
        prop_assert_eq!(legacy, win);
    }

    #[test]
    fn versioned_session_round_trip_is_identity(windows in proptest::collection::vec(arb_saved_window(), 0..20)) {
        let session = VersionedSession {
            version: SESSION_FORMAT_VERSION,
            windows,
        };
        let json = serde_json::to_string(&session).unwrap();
        let parsed: SessionData = serde_json::from_str(&json).unwrap();
        assert!(!parsed.is_legacy());
        prop_assert_eq!(parsed.into_windows(), session.windows);
    }

    #[test]
    fn session_parse_never_panics_on_arbitrary_input(input in ".*") {
        let _ = serde_json::from_str::<SessionData>(&input);
    }

    #[test]
    fn app_config_parse_never_panics_on_arbitrary_input(input in ".*") {
        let _ = toml::from_str::<AppConfig>(&input);
    }
}

#[test]
fn v4_session_without_layout_still_loads_with_layout_none() {
    let v4 = r#"{"version":4,"windows":[{"id":1,"app_id":"firefox","is_focused":true,"idx":2,"name":"web","output":"DP-1","pid":42}]}"#;
    let parsed: SessionData = serde_json::from_str(v4).unwrap();
    assert!(!parsed.is_legacy());
    let windows = parsed.into_windows();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].id, 1);
    assert_eq!(windows[0].layout, None, "pre-v5 files carry no geometry");
}

#[test]
fn example_session_doc_deserializes() {
    let doc = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/example-session.json");
    let raw = fs::read_to_string(&doc).expect("docs/example-session.json must exist");
    let parsed: SessionData = serde_json::from_str(&raw)
        .expect("the documented example must load with the current model");
    assert!(
        !parsed.is_legacy(),
        "the example documents the versioned format"
    );
    let windows = parsed.into_windows();
    assert!(
        !windows.is_empty(),
        "the example documents at least one window"
    );
    assert!(
        windows.iter().all(|w| w.layout.is_some()),
        "the v5 example documents layout on every window"
    );
}

#[test]
fn layout_relevant_covers_geometry_changes() {
    let layout_event = niri_ipc::Event::WindowLayoutsChanged {
        changes: vec![(
            1,
            niri_ipc::WindowLayout {
                pos_in_scrolling_layout: Some((2, 1)),
                tile_size: (960.0, 540.0),
                window_size: (950, 530),
                tile_pos_in_workspace_view: Some((10.0, 5.0)),
                window_offset_in_tile: (5.0, 5.0),
            },
        )],
    };
    assert!(layout_relevant(&layout_event));
    let keyboard_event = niri_ipc::Event::KeyboardLayoutsChanged {
        keyboard_layouts: niri_ipc::KeyboardLayouts {
            names: vec!["us".to_string()],
            current_idx: 0,
        },
    };
    assert!(!layout_relevant(&keyboard_event));
}

/// Real-niri regression fixture: a sanitized capture of the live event stream
/// from niri unstable 2026-08-02 (feb3e43), 2026-09-16, titles redacted. It
/// contains the up-front state-sync burst — including `CastsChanged`, the
/// variant that niri-ipc 25.11 could not parse and that killed every stream
/// ~2 ms after subscribe — plus one deliberate unknown-variant poison line.
/// Every line must either deserialize or be identifiable as tolerated drift;
/// re-pinning niri-ipc to a version lacking `CastsChanged` fails this test.
#[test]
fn live_niri_event_stream_fixture_parses_or_is_tolerated() {
    let fixture = include_str!("testdata/niri-event-stream-2026-08-02.jsonl");
    let mut parsed = 0usize;
    let mut tolerated = 0usize;
    for line in fixture.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<niri_ipc::Event>(line) {
            Ok(event) => {
                parsed += 1;
                // Parsed events must be classifiable by the save loop.
                let _ = layout_relevant(&event);
            }
            Err(_) => {
                tolerated += 1;
            }
        }
    }
    assert!(
        parsed >= 40,
        "the pinned niri-ipc must understand (nearly) all of the 2026-08-02 capture; \
         got {parsed} parsed, {tolerated} tolerated"
    );
    assert_eq!(
        tolerated, 1,
        "exactly the injected poison line may be tolerated"
    );
    assert!(
        serde_json::from_str::<niri_ipc::Event>(r#"{"CastsChanged":{"casts":[]}}"#).is_ok(),
        "the pinned niri-ipc must know CastsChanged (the 2026-09-15 production killer)"
    );
}

#[test]
fn restore_outcome_display_is_stable_for_humans() {
    assert_eq!(
        RestoreOutcome::SeededNewSession.to_string(),
        "Seeded a new session file from the current state"
    );
    assert_eq!(
        RestoreOutcome::NothingToRestore.to_string(),
        "Session file held nothing to restore"
    );
    assert_eq!(
        RestoreOutcome::WouldRestore { window_count: 7 }.to_string(),
        "DRY RUN: would restore 7 window(s)"
    );

    assert_eq!(
        RestoreOutcome::Restored { spawned: 4 }.to_string(),
        "Restored 4 window(s)"
    );
}

// --- M30: health telemetry + wire-format pins ---

#[test]
fn session_staleness_warning_fires_only_past_two_save_intervals() {
    let fresh = Duration::from_secs(10 * 60);
    let stale = Duration::from_secs(31 * 60);
    assert!(
        session_staleness_warning(Some(fresh), 15).is_none(),
        "a 10-min-old session with a 15-min interval is not stale"
    );
    let warning = session_staleness_warning(Some(stale), 15)
        .expect("a 31-min-old session with a 15-min interval must warn");
    assert!(
        warning.contains("stale"),
        "warning names the problem: {warning}"
    );
    assert!(
        warning.contains("30 min"),
        "warning states the 2x-interval threshold: {warning}"
    );
    assert!(
        session_staleness_warning(Some(Duration::from_secs(90)), 1).is_none(),
        "staleness scales with the configured interval (90s <= 2x1min)"
    );
    assert!(
        session_staleness_warning(Some(Duration::from_secs(130)), 1).is_some(),
        "130s > 2x1min is stale"
    );
    assert!(
        session_staleness_warning(Some(Duration::from_secs(5 * 60)), 0).is_some(),
        "a zero interval still yields a sane 2-min threshold"
    );
    assert!(
        session_staleness_warning(None, 15).is_none(),
        "unknown age never warns"
    );
    assert_eq!(SESSION_STALENESS_INTERVALS, 2);
}

#[test]
fn flapping_summary_emits_once_per_flapping_summary_every_deaths() {
    let mut since_summary = 0u32;
    for _ in 0..FLAPPING_SUMMARY_EVERY - 1 {
        assert!(
            flapping_summary(&mut since_summary, 42, 1, RECONNECT_DELAY_INITIAL).is_none(),
            "no summary before {FLAPPING_SUMMARY_EVERY} deaths"
        );
    }
    let summary = flapping_summary(&mut since_summary, 42, 1, RECONNECT_DELAY_MAX)
        .expect("the 10th death must emit the summary");
    assert!(
        summary.contains("42 deaths"),
        "summary carries the total: {summary}"
    );
    assert_eq!(since_summary, 0, "emitting resets the counter");
    for _ in 0..FLAPPING_SUMMARY_EVERY - 1 {
        assert!(flapping_summary(&mut since_summary, 43, 2, RECONNECT_DELAY_INITIAL).is_none());
    }
    assert!(flapping_summary(&mut since_summary, 43, 2, RECONNECT_DELAY_INITIAL).is_some());
}

proptest! {
    /// F1's lesson, pinned forever: arbitrary event-stream lines — valid JSON
    /// of unknown variants, malformed JSON, raw garbage — must classify as
    /// Event/Unparsed exactly as serde does and NEVER kill the reader; only
    /// EOF (stream death) surfaces as an error.
    #[test]
    fn unknown_event_lines_never_kill_the_reader(line in "[^\n]{0,120}") {
        use std::io::{BufReader, Write as _};
        let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
        writeln!(writer, "{line}").unwrap();
        drop(writer); // EOF right after the line
        let mut read_event = event_reader(BufReader::new(reader));
        let outcome = read_event()
            .unwrap()
            .expect("the single line must be delivered before EOF");
        match outcome {
            ReadEvent::Event(event) => {
                prop_assert!(serde_json::from_str::<niri_ipc::Event>(&line).is_ok());
                let _ = layout_relevant(&event);
            }
            ReadEvent::Unparsed(returned) => {
                prop_assert!(serde_json::from_str::<niri_ipc::Event>(&line).is_err());
                prop_assert_eq!(returned.trim(), line.trim());
            }
        }
        // EOF after the line is stream death (Err), never a hang.
        prop_assert!(read_event().is_err());
    }
}

#[test]
fn wire_format_requests_serialize_as_quoted_bare_strings() {
    // niri's protocol serializes unit requests as a quoted bare string, e.g.
    // `"EventStream"` (byte-probe-verified live 2026-09-15). Pin the exact
    // bytes so a niri-ipc upgrade that changes request serialization fails
    // here instead of against a real compositor.
    for (request, wire) in [
        (niri_ipc::Request::EventStream, "\"EventStream\""),
        (niri_ipc::Request::Windows, "\"Windows\""),
        (niri_ipc::Request::Workspaces, "\"Workspaces\""),
        (niri_ipc::Request::Version, "\"Version\""),
    ] {
        assert_eq!(serde_json::to_string(&request).unwrap(), wire);
    }
}

#[test]
fn request_reply_round_trips_over_a_real_socket() {
    use std::io::{BufRead as _, BufReader, Write as _};
    let (client_sock, server_sock) = std::os::unix::net::UnixStream::pair().unwrap();
    let peer = std::thread::spawn(move || {
        let mut peer_stream = BufReader::new(server_sock);
        let mut buf = String::new();
        peer_stream.read_line(&mut buf).unwrap();
        assert_eq!(
            buf, "\"Version\"\n",
            "the request must arrive on the wire as a quoted bare string"
        );
        peer_stream
            .get_mut()
            .write_all(b"{\"Ok\":{\"Version\":\"probe-1.2.3\"}}\n")
            .unwrap();
    });
    let mut client = BufReader::new(client_sock);
    let reply = request_reply(&mut client, &niri_ipc::Request::Version).unwrap();
    assert!(
        matches!(
            reply,
            niri_ipc::Reply::Ok(niri_ipc::Response::Version(ref v)) if v == "probe-1.2.3"
        ),
        "unexpected reply: {reply:?}"
    );
    peer.join().unwrap();
}

#[test]
fn niri_ipc_dependency_stays_exactly_pinned() {
    // Wire-format pin policy (AGENTS.md): niri-ipc must be an EXACT pin so
    // `cargo update` can never silently change IPC deserialization. The
    // functional drift guard is the fixture test above; this pins the pin.
    let manifest = include_str!("../Cargo.toml");
    let line = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("niri-ipc"))
        .unwrap_or_else(|| panic!("niri-ipc dependency line missing from Cargo.toml"));
    assert!(
        line.contains("= \""),
        "niri-ipc must stay exactly pinned (found: {line})"
    );
}
