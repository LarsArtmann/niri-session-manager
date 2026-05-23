# Status Report: Terminal State Recovery — Production Hardening Complete

**Date:** 2026-05-23 22:37  
**Branch:** `feat/terminal-state-recovery`  
**Base:** `main` (MTeaHead/niri-session-manager)

---

## Executive Summary

The terminal state recovery feature has been implemented, reviewed, and hardened through three rounds of work. The branch now converges with PR #2's IPC version and workspace approach, has data-driven per-terminal flag handling, shell injection protection, async-safe `/proc` reads, and 27 passing unit tests. **Build: clean. Tests: 27/27 pass.**

---

## A) FULLY DONE

### Core Feature (commit `d74a64d`)
1. **`src/proc.rs`** (128 lines) — Linux-only `/proc` reading module
   - `resolve_child_process(pid, shell_names, helper_names, max_depth)` — walks process tree under a PID
   - Skips configurable shells and helper processes
   - Falls back to `tpgid` from `/proc/<pid>/stat` field 8 when no children found
   - Filters out `__atexit__` (fish artifact)
   - `#[cfg(target_os = "linux")]` gated; compiles to no-op on non-Linux

### Hardening Round 1 (commit `4887100` — initial review fixes)
2. **`TerminalKind` enum** → per-terminal flag handling (later refactored)
3. **Shell injection fix** — `shell_escape()` single-quote escapes `child_cmd`
4. **PID type** — `SavedWindow.pid` changed to `Option<u32>` with guarded conversion
5. **Configurable helper names** — `helper_names` field in `TerminalStateConfig`
6. **Dead code removed** — `original_args`, commented-out lines, unused `toml` import
7. **`Debug` derives** on `SavedWindow` and `TerminalState`
8. **`&Path`** instead of `&PathBuf` on all function signatures

### Hardening Round 2 (commit `4887100` — strategic improvements)
9. **niri-ipc converged** — downgraded from 26.4.0 to 25.11.0 (semver compatible with PR #2's 25.5.1). Both PRs now share the same IPC version family, reducing merge conflict for upstream.
10. **Workspace-aware restore** — adopted PR #2's approach:
    - `SavedWindow` now stores `workspace_idx: Option<u8>`, `workspace_name: Option<String>`, `workspace_output: Option<String>`
    - `workspace_id` dropped (ephemeral in niri, breaks on workspace create/delete)
    - `get_niri_workspaces()` added
    - `MoveWindowToMonitor` before `MoveWindowToWorkspace` in restore
    - Prefers workspace name over index for named workspaces
    - Guards against empty workspace name strings
    - Old session.json with `workspace_id` still deserializes via `#[serde(default)]`
11. **Data-driven `TerminalProfile`** — replaced 6-arm enum + 80 lines of duplicated match arms with a 4-field struct:
    - `needs_start_subcommand: bool` (wezterm needs `start`)
    - `cwd_flag: &'static str` (varies per terminal)
    - `cwd_flag_separator: bool` (ghostty uses `=` syntax)
    - `cmd_flag: &'static str` (empty for positional, `-e` or `--` for others)
    - Correct flags: kitty `--directory`, foot `--working-directory`, wezterm `start --cwd ... --`, ghostty `--working-directory=... -e`, alacritty `--working-directory -e`
    - Adding new terminals: one `const fn` constructor
12. **`spawn_blocking`** — `/proc` reads wrapped in `tokio::task::spawn_blocking` via new `resolve_terminal_state()` async helper. No longer blocks the tokio runtime.
13. **27 unit tests** covering:
    - `shell_escape` (7 tests): empty, simple, spaces, single quotes, semicolons, dollar, backticks
    - `TerminalProfile` variants (6 tests): kitty, foot, wezterm, ghostty, alacritty, generic
    - `build_terminal_restore_command` (7 tests): per-terminal with/without CWD, shell metacharacters
    - `build_spawn_command` (2 tests): fallback to mappings, terminal state usage
    - `SavedWindow` deserialization (3 tests): old format with `workspace_id`, new format with workspace fields, minimal
    - `resolve_executable_name` (1 test)
    - `TerminalStateConfig` defaults (1 test)
14. **README updated** — per-terminal flag documentation, helper_names in config example, future section updated
15. **Config template** — includes `helper_names`, updated `shell_names` (kitten removed from shell list)

### Supporting Work
16. **`default.nix`** — source filter fix to include untracked `src/proc.rs`
17. **`Cargo.toml`** — removed `cargo-features = ["edition2024"]` (stable since 1.85)
18. **PR-PLAN.md** — comprehensive plan document
19. **`docs/status/2026-05-06_05-32_terminal-state-recovery-implementation.md`** — initial status report

---

## B) PARTIALLY DONE

### 1. Manual testing on a live niri session
- Builds clean, all tests pass, but zero runtime testing has been done
- No niri compositor available in this environment
- The entire `/proc` walking logic is untested against real process trees
- The workspace restore logic is untested against real multi-monitor setups

### 2. Error handling in restore spawn loop
- `MoveWindowToMonitor` errors are silently ignored with `let _ =`
- This is intentional (monitor may not exist yet during restore) but should log warnings
- `MoveWindowToWorkspace` errors are propagated but `let _ =` discards the outer result

### 3. Workspace restore race condition
- Windows are spawned concurrently via `tokio::spawn`
- If window A needs workspace 3 and window B needs workspace 5, both move commands race
- PR #2 has the same issue and acknowledges it with a TODO
- Solution: sort windows by `workspace_idx` before spawning, batch sequentially per workspace

---

## C) NOT STARTED

### 1. Integration tests
- Unit tests cover pure functions only
- No integration test for save → file → restore round-trip
- Would require mocking niri IPC socket (significant effort)

### 2. `/proc` module tests
- `proc.rs` has zero tests — cannot easily test without mocking `/proc` filesystem
- Options: tempdir with fake `/proc` structure, or extract to trait for mocking

### 3. Edge case validation
- `__atexit__` fish artifact filtering (coded but untested at runtime)
- Nested shells (kitty → fish → sudo → btop) — depth walk coded but untested
- Child exited between save and restore — stale command spawned (by design, best-effort)
- `/proc/<pid>/task/<tid>/children` not available on all kernels
- Kernel without `CONFIG_PIDFD` or `CONFIG_PROC_CHILDREN`

### 4. Shell-agnostic `$SHELL` fallback
- Currently assumes `$SHELL` is set
- Could read from `/etc/passwd` as fallback
- Could add `restore_shell` config option

### 5. Session JSON format versioning
- No version field — relies on `#[serde(default)]` for backward compat
- Future format changes harder to manage

### 6. CLI overrides
- No `--terminal-state` flag to override config
- No `--dry-run` flag to preview restore without spawning
- No `--save-only` / `--restore-only` mode

### 7. Window title preservation
- Title is still discarded (was in original code)
- Could be useful for better matching or user identification

### 8. Config migration path
- Old config files without `[terminal_state]` section work via `#[serde(default)]`
- But old session.json files with `workspace_id` lose workspace info on restore
- Should log a warning when `workspace_id` is present but new fields are missing

---

## D) TOTALLY FUCKED UP

### 1. No runtime verification possible
- We compile, we pass 27 unit tests, but **zero runtime testing** has been done
- The entire `/proc` walking logic is untested against real process trees
- All `TerminalProfile` flag patterns are based on documentation, not verified execution
- The workspace restore logic is untested against real multi-monitor setups
- This is a fundamental limitation — we cannot fix it without a running niri compositor

### 2. Pre-existing `flake.nix` overlay bug
- `overlay does not take an argument named 'final'` — existed before this branch
- Not caused by our changes but will fail `nix flake check`
- Not our responsibility but worth noting

### 3. PR #2 coordination still unresolved
- Our branch and PR #2 touch overlapping code
- Both upgrade niri-ipc (now same version family)
- Both change the session file format (now compatible approach)
- Both rewrite IPC call sites (now both use `Reply` type alias pattern)
- But merge order and conflict resolution still need human coordination

---

## E) WHAT WE SHOULD IMPROVE

### Architecture / Code Quality
1. **`proc.rs` is completely untested at runtime** — needs integration tests with fake `/proc`
2. **`build_terminal_restore_command` still has repeated `sh -c` + `escaped_cmd` pattern** — could extract a builder closure
3. **`load_app_config()` called in both `save_session_with_backup` and `restore_session_internal`** — could be cached or passed as parameter
4. **Error handling in `resolve_child_process`** — silently returns `None` on any error; could log warnings
5. **`exec $SHELL` assumption** — not all systems have `$SHELL` set; could read from `/etc/passwd`
6. **Session JSON not versioned** — no explicit version field for future format changes
7. **`shell_escape` should be in a shared module** — currently inline in `main.rs`; if project grows, extract it
8. **`MoveWindowToMonitor` errors silently discarded** — should at minimum log warnings
9. **Workspace restore race condition** — concurrent spawning means workspace ordering is not guaranteed
10. **`SavedWindow` is a flat struct** — could benefit from splitting into `WindowLocation` (workspace fields) and `WindowIdentity` (id, app_id) for cleaner separation

### Build / Infra
11. **No CI for this branch** — GitHub Actions `checks.yml` exists but may not cover new niri-ipc version
12. **`default.nix` source filter is fragile** — custom filter could break if new file types are added
13. **No `nix flake check` passing** — pre-existing overlay bug

---

## F) Top #25 Things We Should Get Done Next

| # | Priority | Task | Effort |
|---|----------|------|--------|
| 1 | **P0** | Manual test on live niri session with running terminals (kitty + btop, nvim, ssh) | S |
| 2 | **P0** | Test backward compatibility with old session.json (no pid/terminal_state/workspace_idx) | S |
| 3 | **P0** | Test workspace restore on multi-monitor setup | M |
| 4 | **P0** | Discuss merge strategy with upstream maintainer (coordinate with PR #2 author) | S |
| 5 | **P1** | Add integration test for `proc.rs` with tempdir-based fake `/proc` | M |
| 6 | **P1** | Log warnings on `MoveWindowToMonitor` errors instead of silently discarding | S |
| 7 | **P1** | Sort windows by workspace_idx before spawning to fix race condition | S |
| 8 | **P1** | Log warning when old-format `workspace_id` detected in session JSON | S |
| 9 | **P1** | Test nested shell walking (kitty → fish → sudo → btop) | S |
| 10 | **P2** | Add session JSON format version field | S |
| 11 | **P2** | Add error logging/warnings in `proc.rs` when `/proc` reads fail | S |
| 12 | **P2** | Fix pre-existing `flake.nix` overlay bug | S |
| 13 | **P2** | Add `--dry-run` flag to preview restore without spawning | M |
| 14 | **P2** | Test on kernel without `/proc/<pid>/task/<tid>/children` support | M |
| 15 | **P2** | Cache `load_app_config()` result instead of calling per-save | S |
| 16 | **P2** | Add `--terminal-state` CLI flag to override config | S |
| 17 | **P3** | Handle `$SHELL` not being set (fallback to `/etc/passwd` entry) | S |
| 18 | **P3** | Add `terminal_state.restore_shell` config option | S |
| 19 | **P3** | Add a `--save-only` / `--restore-only` CLI mode | S |
| 20 | **P3** | Separate niri-ipc upgrade into its own commit/PR (if maintainer prefers) | M |
| 21 | **P3** | Extract `shell_escape` into a shared utility module | S |
| 22 | **P4** | Add window title to saved data for better matching | S |
| 23 | **P4** | Split `SavedWindow` into focused sub-structs (location, identity, terminal) | S |
| 24 | **P4** | Add logging verbosity levels (quiet/normal/verbose) | S |
| 25 | **P4** | Add a session diff viewer (show what changed between saves) | L |

---

## G) Top #1 Question I Cannot Figure Out Myself

**Should we rebase this branch on top of PR #2, or keep them separate for the maintainer to merge independently?**

Our branch now shares the same IPC version (25.x family) and workspace approach (idx/name/output + MoveWindowToMonitor) as PR #2. But there are three ways to proceed:

1. **Rebase on PR #2** — cleanest for the maintainer, but requires PR #2 to land first
2. **Keep separate** — maintainer merges PR #2 first, then ours (with conflicts resolved)
3. **Combined PR** — merge both into one mega-PR (simplest process, biggest blast radius)

The technical overlap is now minimal (we've aligned on the same patterns), but the git conflicts will still exist because both touch the same lines in `src/main.rs`. The maintainer and PR #2 author need to weigh in.

---

## File Change Summary

**Branch total (3 commits on branch):**
```
 Cargo.lock         |  43 +-
 Cargo.toml         |   4 +-
 PR-PLAN.md         | 309 +++++++++++++++
 README.md          |  32 +-
 default.nix        |   9 +-
 docs/status/...md  | 226 ++++++++++++++
 src/main.rs        | 781 +++++++++++++++++++++++++++++++++++++++-----
 src/proc.rs        | 128 +++++++++++
 8 files changed, +1389/-143
```

**Source code:** 1,159 lines total (`main.rs` 1,031 + `proc.rs` 128)  
**Tests:** 27 unit tests, all passing  
**Build:** `nix build` clean  
**IPC version:** niri-ipc 25.11.0 (compatible with PR #2's 25.5.1)

---

_Waiting for instructions._
