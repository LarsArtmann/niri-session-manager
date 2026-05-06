# Status Report: Terminal State Recovery Implementation

**Date:** 2026-05-06 05:32  
**Branch:** `feat/terminal-state-recovery`  
**Base:** `main` (MTeaHead/niri-session-manager)

---

## Executive Summary

Implemented the full **Terminal State Recovery** feature from PR-PLAN.md: terminals are now saved with their foreground child process (e.g. `btop`, `nvim`, `ssh`) and restored with the same command + working directory. The feature is Linux-only, backward-compatible, and configurable via TOML.

**Build status:** Compiles clean (`cargo check`), builds via `nix build` successfully.

---

## A) FULLY DONE

### 1. `src/proc.rs` — Linux-only /proc reading module
- `resolve_child_process(pid, shell_names, max_depth)` — walks the process tree under a PID
- Skips known shells (`fish`, `bash`, `zsh`, etc.) and terminal helpers (`kitten`)
- Falls back to `tpgid` from `/proc/<pid>/stat` field 8 when no children found
- Filters out `__atexit__` (fish artifact)
- `#[cfg(target_os = "linux")]` gated; compiles to no-op on non-Linux

### 2. `SavedWindow` / `TerminalState` structs
- Replaced `WindowWithoutTitle` with richer `SavedWindow` containing:
  - `pid: Option<i32>` — from niri IPC `Window.pid`
  - `terminal_state: Option<TerminalState>` — populated only for terminal app_ids
- `TerminalState` has `child_command`, `child_cwd`, `original_args`
- Backward compatible: missing fields deserialize as `None` via `#[serde(default)]`

### 3. `TerminalStateConfig` in TOML config
- New `[terminal_state]` section with:
  - `enabled` (default: true)
  - `terminal_app_ids` (default: kitty, foot, wezterm, ghostty, alacritty)
  - `shell_names` (default: fish, bash, zsh, sh, dash, and login-shell variants, kitten, sudo, doas)
  - `max_walk_depth` (default: 20)
- Added to `AppConfig` struct with `#[serde(default)]`
- Default config template updated with new section

### 4. Save flow — `save_session()`
- Loads `TerminalStateConfig` from app config
- For each window: if terminal app_id + has PID + enabled → calls `proc::resolve_child_process()`
- Populates `TerminalState` with `child_command` and `child_cwd`
- Non-terminal windows get `pid` and `terminal_state` as `None`

### 5. Restore flow — `restore_session_internal()`
- Deserializes `Vec<SavedWindow>` (backward compatible with old format)
- `build_spawn_command()` checks for terminal state:
  - If `child_command` exists → builds `kitty --directory /cwd -e sh -c "command; exec $SHELL"`
  - Otherwise → falls back to app_mappings or app_id
- `build_terminal_restore_command()` generates per-terminal spawn commands with:
  - `--directory` only if CWD differs from `$HOME`
  - `exec $SHELL` after child command exits (drops into shell instead of closing terminal)

### 6. niri-ipc upgrade: 0.1.10 → 26.4.0
- **Required** because 0.1.10 did not have `pid` field on `Window` struct
- Updated all IPC call sites for new API:
  - `socket.send()` now returns `io::Result<Reply>` where `Reply = Result<Response, String>` (was `(Reply, Response)` tuple)
  - `MoveWindowToWorkspace` now requires `focus: bool` field
  - Socket must be `mut` for `.send()`
- Removed unused `Reply` import (now a type alias)

### 7. `default.nix` — Source filter fix
- Changed from `src = ./.` (git-tracked-only) to explicit filter including `.rs`, `.toml`, `.lock`, `.nix` files
- Untracked files like `src/proc.rs` are now included in the nix build
- Added `lib` to function arguments

### 8. `Cargo.toml` cleanup
- Removed `cargo-features = ["edition2024"]` (stable since Rust 1.85, generates warning)

### 9. README.md updated
- Added "Terminal state recovery" to features list
- Added `[terminal_state]` section to example config
- Added explanation of how restore works with example command
- Removed completed TODO item ("Use PID to fetch the actual process command")

---

## B) PARTIALLY DONE

### 1. `original_args` field in TerminalState
- **Struct exists** but is never populated (always `None`)
- PR-PLAN.md describes using it for terminals launched with `-e` flag
- The `build_spawn_command` logic checks `original_args` but it's a dead code path currently
- **Impact:** Low — `child_command` covers the primary use case

### 2. Testing
- **No automated tests exist** (pre-existing — the project has no test suite at all)
- Only verified via `cargo check` and `nix build`
- No manual testing on a running niri session has been done

---

## C) NOT STARTED

### 1. Manual testing on a live niri session
- Save with running terminals (kitty + btop, nvim, ssh)
- Restore and verify commands are re-launched correctly
- Verify CWD restoration works
- Verify non-terminal apps unaffected
- Verify backward compatibility with old session.json

### 2. Edge case validation
- `__atexit__` fish artifact filtering (coded but untested)
- Nested shells (kitty → fish → sudo → btop) — depth walk coded but untested
- Child exited between save and restore — stale command spawned (by design, best-effort)
- `/proc/<pid>/task/<tid>/children` not available on all kernels

### 3. Shell-agnostic `$SHELL` fallback
- Currently uses `exec $SHELL` which depends on the env var being set
- Could hardcode the user's shell from `getent passwd` as fallback

### 4. Terminal-specific flag handling
- `--directory` flag assumed for all terminals (kitty, foot support it; others may not)
- `-e` flag assumed for all terminals (wezterm uses `--` instead, ghostty uses `-e`)
- PR-PLAN.md mentions `build_terminal_restore_command` should be per-terminal

---

## D) TOTALLY FUCKED UP

### 1. niri-ipc 0.1.10 → 26.4.0 is a MASSIVE breaking change
- This is a **major concern** for the upstream PR
- The original project pins `niri-ipc = "=0.1.10"` — jumping to `26.4.0` changes the entire IPC API
- All IPC call sites had to be rewritten (send/recv pattern, Reply type, Action variants)
- **This will need discussion with the maintainer** — they may want to keep the old version or do a separate upgrade PR
- The `default.nix` change is also invasive — changes the source inclusion mechanism

### 2. No running niri compositor available for testing
- We can compile but cannot verify the feature actually works at runtime
- The entire proc.rs logic is untested against real /proc data

### 3. Pre-existing `flake.nix` overlay bug
- `overlay does not take an argument named 'final'` — existed before this branch
- Not caused by our changes but will fail `nix flake check`

---

## E) WHAT WE SHOULD IMPROVE

### Architecture / Code Quality
1. **`proc.rs` is completely untested** — needs unit tests with mocked `/proc` filesystem or integration tests
2. **`build_terminal_restore_command` assumes all terminals use `-e` and `--directory`** — should be a per-terminal strategy (wezterm uses different flags)
3. **`original_args` is dead code** — either implement it or remove it
4. **`load_app_config()` called inside `save_session()`** — previously it was only called in restore; now every save loads config. Could be cached or passed as parameter
5. **Error handling in `resolve_child_process`** — silently returns `None` on any error; could log warnings for debugging
6. **`is_terminal_helper` has hardcoded list** — should be configurable like `shell_names`
7. **`exec $SHELL` assumption** — not all systems have `$SHELL` set; could read from `/etc/passwd`
8. **Session JSON format not versioned** — backward compatibility relies on `#[serde(default)]` but has no explicit version field for future format changes

### Build / Infra
9. **`default.nix` source filter is fragile** — `lib.sources.cleanSourceWith` with a custom filter could break if new file types are added
10. **No CI for this branch** — GitHub Actions checks.yml exists but may not cover the new niri-ipc version
11. **`Cargo.lock` has changed significantly** — many transitive dependency updates from niri-ipc upgrade

---

## F) Top 25 Things We Should Get Done Next

| # | Priority | Task | Effort |
|---|----------|------|--------|
| 1 | **P0** | Manual test on live niri session with running terminals | S |
| 2 | **P0** | Test backward compatibility with old session.json (no pid/terminal_state) | S |
| 3 | **P0** | Discuss niri-ipc major version upgrade with upstream maintainer | S |
| 4 | **P1** | Add per-terminal flag handling (wezterm `--`, ghostty `-e`, foot `-e`) | M |
| 5 | **P1** | Add unit tests for `proc.rs` with mocked `/proc` | M |
| 6 | **P1** | Populate `original_args` in TerminalState or remove the dead field | S |
| 7 | **P1** | Make `is_terminal_helper` configurable via config | S |
| 8 | **P1** | Test nested shell walking (kitty → fish → sudo → btop) | S |
| 9 | **P2** | Add session JSON format version field | S |
| 10 | **P2** | Add error logging/warnings in proc.rs when /proc reads fail | S |
| 11 | **P2** | Fix pre-existing flake.nix overlay bug | S |
| 12 | **P2** | Add `--terminal-state` CLI flag to override config | S |
| 13 | **P2** | Test on kernel without `/proc/<pid>/task/<tid>/children` support | M |
| 14 | **P2** | Add integration test that saves and restores a session file | M |
| 15 | **P2** | Cache `load_app_config()` result instead of calling per-save | S |
| 16 | **P3** | Handle `$SHELL` not being set (fallback to `/etc/passwd` entry) | S |
| 17 | **P3** | Add `terminal_state.restore_shell` config option (instead of hardcoded `$SHELL`) | S |
| 18 | **P3** | Add `--dry-run` flag to show what would be restored without spawning | M |
| 19 | **P3** | Add logging verbosity levels (quiet/normal/verbose) | S |
| 20 | **P3** | Separate niri-ipc upgrade into its own commit/PR | M |
| 21 | **P3** | Add a `--save-only` / `--restore-only` CLI mode | S |
| 22 | **P3** | Verify `--directory` flag works for all configured terminal_app_ids | S |
| 23 | **P4** | Add window title to saved data (currently discarded) for better matching | S |
| 24 | **P4** | Consider saving environment variables from the terminal | M |
| 25 | **P4** | Add a session diff viewer (show what changed between saves) | L |

---

## G) Top #1 Question I Cannot Figure Out Myself

**Will the upstream maintainer accept the `niri-ipc` 0.1.10 → 26.4.0 upgrade as part of this PR, or should it be a separate PR?**

This upgrade was **mandatory** because the old version (0.1.10) does not expose the `pid` field on the `Window` struct — the entire feature depends on it. However, it's a massive API break:
- All IPC call sites rewritten (Reply type, send signature, Action variants)
- `MoveWindowToWorkspace` gained a `focus` field
- Socket requires `mut` binding
- Transitive dependencies shifted significantly

The maintainer may prefer to:
1. Accept it all in one PR (simplest, but large blast radius)
2. Split into two PRs: first upgrade niri-ipc, then add terminal state
3. Pin a specific newer version that has `pid` but fewer API changes (if such a version exists)

This is a **people/process decision**, not a technical one. I cannot resolve it without talking to the maintainer.

---

## File Change Summary

```
 Cargo.lock  |  43 +++++++-----
 Cargo.toml  |   4 +-
 README.md   |  22 +++++-
 default.nix |   9 ++-
 src/main.rs | 224 ++++++++++++++++++++++++++++++++++++++++++++++++------------
 src/proc.rs | 127 +++++++++++++++++++++++++++++++++++++++
 6 files changed, 360 insertions(+), 69 deletions(-)
```

**Files changed:** 5 modified, 1 new  
**Lines added:** ~360  
**Lines removed:** ~69  
**Net change:** +291 lines
