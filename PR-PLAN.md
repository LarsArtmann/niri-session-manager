# PR Plan: Terminal State Recovery via /proc PID Resolution

**Branch:** `feat/terminal-state-recovery`
**Base:** `main` (MTeaHead/niri-session-manager)

---

## Problem

When restoring a session, terminals are spawned by `app_id` (e.g. `kitty`). This opens a bare shell — losing whatever was running inside (btop, nvim, ssh, a build command).

The niri IPC `Window` struct already has a `pid` field, but it's **discarded** in `WindowWithoutTitle`.

## Solution

### 1. Keep PID in saved data

Replace `WindowWithoutTitle` with a richer struct:

```rust
#[derive(Serialize, Deserialize)]
struct SavedWindow {
    id: u64,
    app_id: String,
    workspace_id: Option<u64>,
    is_focused: bool,
    pid: Option<u32>,

    // Terminal state — populated only for terminal app_ids
    terminal_state: Option<TerminalState>,
}

#[derive(Serialize, Deserialize)]
struct TerminalState {
    child_command: Option<String>,  // e.g. "btop", "nvim /path/to/file"
    child_cwd: Option<String>,      // e.g. "/home/lars/projects/foo"
    original_args: Option<Vec<String>>,  // from /proc/<pid>/cmdline
}
```

### 2. New module: `src/proc.rs`

Linux-only `/proc` reading (cfg(target_os = "linux")):

```rust
/// Walk the process tree under `pid` to find the foreground child process.
/// Skips known shells (fish, bash, zsh, sh, dash).
/// Returns (command_string, cwd) if found.
fn resolve_child_process(pid: u32) -> Option<(String, String)>

/// Read /proc/<pid>/cmdline as Vec<String> (null-separated)
fn read_cmdline(pid: u32) -> Option<Vec<String>>

/// Read /proc/<pid>/cwd via symlink
fn read_cwd(pid: u32) -> Option<String>

/// Read /proc/<pid>/comm (process name)
fn read_comm(pid: u32) -> Option<String>

/// Get child PIDs of a process via /proc/<pid>/task/<tid>/children
fn get_children(pid: u32) -> Vec<u32>

/// Check if a process name is a known shell
fn is_shell(comm: &str) -> bool
```

**Algorithm:**
```
fn resolve_child_process(pid):
    args = read_cmdline(pid)
    current = pid
    for _ in 0..20:
        if !exists(/proc/current): break
        children = get_children(current)
        if children.is_empty():
            // Fallback: check tpgid from /proc/current/stat field 8
            tpgid = read_stat_field(current, 8)
            if tpgid > 0:
                fg_comm = read_comm(tpgid)
                if !is_shell(fg_comm) and fg_comm != "__atexit__":
                    return (read_cmdline(tpgid).join(" "), read_cwd(tpgid))
            break
        current = children[0]
        comm = read_comm(current)
        if is_shell(comm) or is_terminal_helper(comm): continue
        return (read_cmdline(current).join(" "), read_cwd(current))
    return None
```

### 3. Config: terminal app IDs

Add to TOML config:

```toml
[terminal_state]
enabled = true
terminal_app_ids = ["kitty", "foot", "org.wezfurlong.wezterm", "com.mitchellh.ghostty", "alacritty"]
shell_names = ["fish", "bash", "zsh", "sh", "dash", "-fish", "-bash", "-zsh", "kitten", "sudo", "doas"]
max_walk_depth = 20
```

Defaults:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TerminalStateConfig {
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default = "default_terminal_app_ids")]
    terminal_app_ids: Vec<String>,
    #[serde(default = "default_shell_names")]
    shell_names: Vec<String>,
    #[serde(default = "default_max_walk_depth")]
    max_walk_depth: u32,
}
```

### 4. Save flow change

In `save_session()`:

```rust
let terminal_config = &app_config.terminal_state;

for window in &windows {
    let mut saved = SavedWindow::from_window(window);

    if terminal_config.enabled {
        if let Some(pid) = window.pid {
            if terminal_config.terminal_app_ids.contains(&saved.app_id) {
                saved.terminal_state = resolve_terminal_state(
                    pid,
                    &terminal_config.shell_names,
                    terminal_config.max_walk_depth,
                );
            }
        }
    }

    saved_windows.push(saved);
}
```

### 5. Restore flow change

In `restore_session_internal()`, when building the spawn command:

```rust
let command = if let Some(ts) = &saved_window.terminal_state {
    if let Some(child_cmd) = &ts.child_command {
        // Build terminal-specific spawn command
        build_terminal_restore_command(
            &app_id,
            child_cmd,
            &ts.child_cwd,
            &ts.original_args,
        )
    } else if let Some(args) = &ts.original_args {
        // Check for -e flag in original args
        if let Some(e_idx) = args.iter().position(|a| a == "-e") {
            let exec_args = &args[e_idx + 1..];
            vec![app_id.clone(), "-e".to_string()]
                .into_iter()
                .chain(exec_args.iter().cloned())
                .collect()
        } else {
            app_config.app_mappings.get(&app_id)
                .cloned()
                .unwrap_or_else(|| vec![app_id.clone()])
        }
    } else {
        app_config.app_mappings.get(&app_id)
            .cloned()
            .unwrap_or_else(|| vec![app_id.clone()])
    }
} else {
    app_config.app_mappings.get(&app_id)
        .cloned()
        .unwrap_or_else(|| vec![app_id.clone()])
};
```

`build_terminal_restore_command` generates per-terminal spawn commands:

```rust
fn build_terminal_restore_command(
    app_id: &str,
    child_cmd: &str,
    child_cwd: &Option<String>,
    original_args: &Option<Vec<String>>,
) -> Vec<String> {
    let mut cmd = vec![app_id.to_string()];

    // Add --directory if CWD differs from HOME
    if let Some(cwd) = child_cwd {
        let home = std::env::var("HOME").unwrap_or_default();
        if cwd != home && !cwd.is_empty() {
            cmd.push("--directory".to_string());
            cmd.push(cwd.clone());
        }
    }

    // Spawn child command; on exit, drop into default shell
    cmd.push("-e".to_string());
    cmd.push("sh".to_string());
    cmd.push("-c".to_string());
    cmd.push(format!("{}; exec fish", child_cmd));

    cmd
}
```

### 6. File structure

```
src/
├── main.rs       # Existing — minimal changes to save/restore flow
└── proc.rs       # NEW — Linux-only /proc reading
```

`proc.rs` is gated behind `#[cfg(target_os = "linux")]`. On non-Linux, `resolve_terminal_state` returns `None`.

### 7. Session JSON format change

Before:
```json
[
  {"id": 42, "app_id": "kitty", "workspace_id": 1, "is_focused": true}
]
```

After:
```json
[
  {
    "id": 42,
    "app_id": "kitty",
    "workspace_id": 1,
    "is_focused": true,
    "pid": 12345,
    "terminal_state": {
      "child_command": "btop",
      "child_cwd": "/home/lars",
      "original_args": ["kitty"]
    }
  },
  {
    "id": 43,
    "app_id": "firefox",
    "workspace_id": 2,
    "is_focused": false,
    "pid": null,
    "terminal_state": null
  }
]
```

Backward compatible: missing `pid` and `terminal_state` fields deserialize as `None`.

---

## Implementation Order

1. **`src/proc.rs`** — all /proc reading functions, Linux-only
2. **New structs** — `SavedWindow`, `TerminalState`, `TerminalStateConfig`
3. **`save_session`** — populate terminal state for terminal app_ids
4. **`restore_session_internal`** — use terminal state to build spawn commands
5. **Config** — add `[terminal_state]` section to TOML
6. **Testing** — manual testing on evo-x2 (Linux), verify macOS still builds (no /proc calls)
7. **Docs** — update README with terminal state section

---

## Edge Cases

| Case | Behavior |
|------|----------|
| No child process (just shell) | `terminal_state = None` → spawn bare terminal |
| Child exited between save and restore | `terminal_state` has stale command → spawn it anyway (best effort) |
| `__atexit__` (fish artifact) | Filtered out in `resolve_child_process` |
| Non-Linux platform | `proc.rs` compiles to no-op, `terminal_state` always `None` |
| Old session JSON (no pid/terminal_state) | Backward compatible — `None` defaults |
| Nested shells (kitty → fish → sudo → btop) | Walk up to 20 levels, skip shells/sudo/doas |

---

## Shell Detection

```rust
fn is_shell(comm: &str) -> bool {
    const SHELLS: &[&str] = &[
        "fish", "bash", "zsh", "sh", "dash",
        "-fish", "-bash", "-zsh", "-sh",
        "kitten",  // kitty's internal helper
        "sudo", "doas",
    ];
    SHELLS.contains(&comm)
}
```

Configurable via `terminal_state.shell_names` in TOML.

---

## Working POC Reference

Bash implementation with full kitty /proc walking:
- Save: https://github.com/LarsArtmann/SystemNix/blob/master/scripts/niri-session-save.sh
- Restore: https://github.com/LarsArtmann/SystemNix/blob/master/scripts/niri-session-restore.sh
