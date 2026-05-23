# Status Report: Terminal State Recovery — Production Hardening Round 3

**Date:** 2026-05-23 23:07  
**Branch:** `feat/terminal-state-recovery`  
**Base:** `main` (MTeaHead/niri-session-manager)

---

## Executive Summary

Nine hardening items from the previous status reports have been resolved. The branch now has versioned session JSON, `/proc` module tests with fake filesystems, resolved-shell restore commands, workspace race condition fix, proper error logging throughout, and a fixed `flake.nix` overlay. **Build: clean. Tests: 42/42 pass. Clippy: zero warnings. `nix flake check`: all checks pass.**

---

## A) FULLY DONE

### Previous Rounds (commits `d74a64d`, `4887100`)
1. **`src/proc.rs`** — Linux-only `/proc` reading module (now 385 lines)
2. **`TerminalProfile`** — data-driven per-terminal flag handling
3. **Shell injection fix** — `shell_escape()` single-quote escapes
4. **`PID` type safety** — `Option<u32>` with guarded conversion
5. **Configurable helper names** — `helper_names` field
6. **niri-ipc converged** — semver compatible with PR #2
7. **Workspace-aware restore** — `workspace_idx`, `workspace_name`, `workspace_output`
8. **`spawn_blocking`** — async-safe `/proc` reads
9. **27 original unit tests**
10. **README updated** — per-terminal flag docs, config examples

### This Round (uncommitted — 9 hardening items)

11. **Error logging on IPC moves** — `MoveWindowToMonitor` and `MoveWindowToWorkspace` errors now log warnings via `log_error()` instead of `let _ =` silent discard. The monitor move is intentionally best-effort (monitor may not exist yet), but now debuggable.

12. **Workspace restore race condition fixed** — Windows are sorted by `workspace_idx` before spawning via `saved_windows.sort_by_key(|w| w.workspace_idx.unwrap_or(0))`. Windows targeting the same workspace now spawn in deterministic order, preventing workspace assignment races.

13. **`/proc` error logging** — `resolve_child_process` now logs:
    - Warning when root PID doesn't exist in `/proc` (process vanished before save)
    - Warning when a child PID's `comm` file is unreadable (race condition with process exit)
    - Uses `eprintln` (safe in `spawn_blocking` context)

14. **Session JSON format versioning** — New `VersionedSession` struct with `version: 2` and `windows` array. The `SessionData` enum uses `#[serde(untagged)]` to transparently parse both:
    - **New format**: `{"version": 2, "windows": [...]}` — detected, parsed, version validated
    - **Legacy format**: `[{...}, {...}]` — detected, parsed, warning logged suggesting re-save
    - `SESSION_FORMAT_VERSION = 2` constant for future format evolution

15. **Legacy `workspace_id` migration warning** — `SavedWindow` now has a `#[serde(default, skip_serializing)] workspace_id: Option<u64>` field. Old session files with `workspace_id` deserialize it silently, but `restore_session_internal` detects its presence and logs: *"Warning: session file contains deprecated 'workspace_id' field. Workspace info was lost. Re-save to upgrade format."*

16. **`$SHELL` fallback chain** — Restore command now resolves the shell at build time instead of relying on runtime `$SHELL` expansion:
    1. `$SHELL` environment variable (if non-empty)
    2. `/proc/self/status` → UID → `/etc/passwd` lookup (Linux-only)
    3. `/bin/sh` as ultimate fallback
    - `get_restore_shell()` returns the resolved shell path
    - `build_terminal_restore_command` uses `shell_escape(&restore_shell)` — fully escaped, no `$SHELL` expansion needed
    - Fixes: terminals closing immediately after child exits when `$SHELL` is unset

17. **`flake.nix` overlay bug fixed** — Changed from `self: super:` (which shadowed the flake's `self`) to `final: prev:` pattern. The overlay now correctly references `self.packages.${prev.hostPlatform.system}` to pull the package from the flake. `nix flake check` passes completely.

18. **`/proc` module tests** — 9 new tests in `proc.rs` using tempdir-based fake `/proc` filesystem:
    - `resolve_finds_direct_child` — kitty → fish → btop walks through shell to find child
    - `resolve_skips_shell_and_finds_grandchild` — multi-level shell skip (kitty → bash → nvim)
    - `resolve_skips_helpers` — kitten helper process skipped correctly
    - `resolve_returns_none_when_only_shell` — no child process found, returns None
    - `resolve_returns_none_when_pid_missing` — PID doesn't exist in fake `/proc`
    - `resolve_filters_atexit` — `__atexit__` fish artifact filtered via tpgid path
    - `resolve_uses_tpgid_fallback` — tpgid from `/proc/stat` used when no children file
    - `is_shell_detection` — shell name matching with login shell prefixes
    - `is_helper_detection` — helper name matching

19. **New tests in `main.rs`** — 6 additional tests:
    - `session_data_parses_versioned_format` — parses `VersionedSession` JSON
    - `session_data_parses_legacy_array_format` — parses old `Vec<SavedWindow>` array
    - `versioned_session_serializes_correctly` — round-trip serialize → deserialize
    - `saved_window_detects_legacy_workspace_id` — detects deprecated field
    - `get_restore_shell_returns_non_empty` — shell resolution always returns something
    - `get_restore_shell_prefers_env_var` — `$SHELL` takes priority

20. **Clippy clean** — Derived `Default` for `AppConfig` (removed manual `impl`), collapsed nested `if` in passwd parser, removed unused `PathBuf` import. Zero clippy warnings.

21. **`default.nix` reformatted** — `nix fmt` applied to all `.nix` files for RFC-style compliance.

### Refactored Internals

22. **`proc.rs` path abstraction** — All internal functions now accept a `base: &Path` parameter instead of hardcoded `/proc`:
    - `read_cmdline_at`, `read_cwd_at`, `read_comm_at`, `get_children_at`, `read_stat_field_at`
    - `resolve_child_process_at(base, ...)` — testable with any directory
    - Public `resolve_child_process()` delegates to `resolve_child_process_at(Path::new("/proc"), ...)`
    - Zero impact on production behavior; enables the 9 fake-filesystem tests

23. **Test helper `assert_restore_command`** — Dynamic assertion helper that validates command prefix and exec suffix separately, avoiding lifetime issues with `get_restore_shell()` returning runtime-dependent strings.

---

## B) PARTIALLY DONE

### 1. Manual testing on a live niri session
- Builds clean, all 42 tests pass, but **zero runtime testing has been done**
- No niri compositor available in this environment
- The entire `/proc` walking logic is tested against fake filesystems only
- The workspace restore logic is untested against real multi-monitor setups
- The resolved-shell restore command is untested against real terminal emulators

### 2. Integration tests
- Unit tests cover pure functions and fake `/proc` extensively
- No integration test for the full save → file → restore round-trip
- Would require mocking niri IPC socket (significant effort)

---

## C) NOT STARTED

### 1. CLI overrides
- No `--terminal-state` flag to override config from command line
- No `--dry-run` flag to preview restore without spawning
- No `--save-only` / `--restore-only` mode

### 2. Window title preservation
- Title is still discarded (was in original code)
- Could be useful for better matching or user identification

### 3. Config migration tooling
- Old config files without `[terminal_state]` section work via `#[serde(default)]`
- But old session.json files with `workspace_id` lose workspace info on restore
- The warning is now logged (done this round), but no automatic migration

### 4. `restore_shell` config option
- Currently resolves shell automatically via env/passwd
- Could add explicit `terminal_state.restore_shell` config for user override

### 5. Logging verbosity levels
- All logging is via `eprintln` / `println`
- No quiet/normal/verbose mode
- No log levels or filtering

### 6. Session diff viewer
- No way to show what changed between saves
- Would require diffing old vs new `Vec<SavedWindow>`

---

## D) TOTALLY FUCKED UP

### 1. No runtime verification possible
- We compile, we pass 42 unit tests with fake `/proc` data, but **zero runtime testing** has been done
- All `TerminalProfile` flag patterns are based on documentation, not verified execution
- The workspace restore logic is untested against real multi-monitor setups
- The resolved-shell `exec '/nix/store/.../bash'` pattern is untested in real terminals
- The tpgid fallback code path is untested against real process trees
- **This is a fundamental limitation — we cannot fix it without a running niri compositor**

### 2. PR #2 coordination still unresolved
- Our branch and PR #2 touch overlapping code
- Both upgrade niri-ipc (now same version family: 25.x)
- Both change the session file format (now compatible approach)
- Both rewrite IPC call sites (now both use `Reply` type alias pattern)
- But merge order and conflict resolution still need human coordination
- Our session format is now versioned (v2) while PR #2 may not version — potential incompatibility

### 3. `/proc/<pid>/task/<tid>/children` not universal
- The `children` file requires `CONFIG_PROC_CHILDREN` kernel option
- Not available on all Linux kernels
- When missing, `get_children` returns empty → falls through to tpgid
- tpgid fallback works but is less precise (only finds foreground process group leader)
- Not testable without a real kernel

---

## E) WHAT WE SHOULD IMPROVE

### Architecture / Code Quality
1. **`build_terminal_restore_command` still has `sh -c` + escaped command pattern** — the `sh -c` wrapper is necessary but could benefit from a documented builder pattern
2. **`load_app_config()` called in both `save_session_with_backup` and `restore_session_internal`** — could be cached or passed as parameter to avoid double reads
3. **`SessionData` uses `#[serde(untagged)]`** — this is convenient but fragile; serde tries each variant in order, so adding new variants requires care
4. **Error handling is still mixed** — some paths use `anyhow::bail!`, some use `log_error`, some silently return `None`. A consistent error strategy would improve debuggability
5. **`log()` / `log_error()` are just println/eprintln** — should use the `log` crate or `tracing` for proper log levels, filtering, and structured output
6. **`SavedWindow` has 9 fields** — could benefit from splitting into `WindowLocation` (workspace fields), `WindowIdentity` (id, app_id), and `TerminalInfo` (pid, terminal_state) for cleaner separation
7. **Tests depend on `$SHELL` env var** — `assert_restore_command` dynamically resolves the shell, making test output environment-dependent. Should mock `get_restore_shell()` in tests
8. **`get_shell_from_passwd` reads entire `/etc/passwd`** — could use `getpwuid_r` via libc/nix for efficiency, but current approach is simple and correct

### Build / Infra
9. **No CI for this branch** — GitHub Actions `checks.yml` exists but may need updates for new dependencies (`tempfile`)
10. **`default.nix` source filter is still fragile** — custom filter could break if new file types are added
11. **`nixfmt-rfc-style` deprecation warning** — should switch to `nixfmt` in treefmt config
12. **`Cargo.lock` has changed** — `tempfile` dev-dependency pulls in `rustix`, `fastrand`, etc. Not a runtime concern but increases lock file size

---

## F) Top #25 Things We Should Get Done Next

| # | Priority | Task | Effort |
|---|----------|------|--------|
| 1 | **P0** | Manual test on live niri session with running terminals (kitty + btop, nvim, ssh) | S |
| 2 | **P0** | Test backward compatibility with old session.json (legacy array, no pid/terminal_state/workspace_idx) | S |
| 3 | **P0** | Test workspace restore on multi-monitor setup | M |
| 4 | **P0** | Discuss merge strategy with upstream maintainer (coordinate with PR #2 author) | S |
| 5 | **P1** | Test the resolved-shell `exec '/path/to/bash'` command actually works in all 5 terminals | S |
| 6 | **P1** | Test nested shell walking (kitty → fish → sudo → btop) against real process trees | S |
| 7 | **P1** | Verify tpgid fallback works on real process trees (kernel without children file) | M |
| 8 | **P1** | Add integration test for save → file → restore round-trip (mock niri IPC) | L |
| 9 | **P1** | Add `--dry-run` flag to preview restore without spawning | M |
| 10 | **P2** | Switch `log()`/`log_error()` to `tracing` crate for proper log levels | M |
| 11 | **P2** | Cache `load_app_config()` result instead of calling per-save | S |
| 12 | **P2** | Add `terminal_state.restore_shell` config option for explicit shell override | S |
| 13 | **P2** | Test on kernel without `/proc/<pid>/task/<tid>/children` support | M |
| 14 | **P2** | Add `--terminal-state` CLI flag to override config | S |
| 15 | **P2** | Mock `get_restore_shell()` in tests to make assertions environment-independent | S |
| 16 | **P3** | Add `--save-only` / `--restore-only` CLI mode | S |
| 17 | **P3** | Separate niri-ipc upgrade into its own commit/PR (if maintainer prefers) | M |
| 18 | **P3** | Extract `shell_escape` into a shared utility module | S |
| 19 | **P3** | Split `SavedWindow` into focused sub-structs (location, identity, terminal) | S |
| 20 | **P3** | Add window title to saved data for better matching | S |
| 21 | **P3** | Fix `nixfmt-rfc-style` deprecation warning in treefmt config | S |
| 22 | **P4** | Add logging verbosity levels (quiet/normal/verbose) | S |
| 23 | **P4** | Add a session diff viewer (show what changed between saves) | L |
| 24 | **P4** | Consider saving environment variables from the terminal | M |
| 25 | **P4** | Add session file encryption for multi-user systems | L |

---

## G) Top #1 Question I Cannot Figure Out Myself

**Should we rebase this branch on top of PR #2, or keep them separate for the maintainer to merge independently?**

Our branch now has **versioned session JSON (v2)** which PR #2 may not have. This creates a potential format incompatibility: if PR #2 merges first and writes unversioned `Vec<SavedWindow>` arrays, our code will log a "legacy format" warning on every restore. If we merge first, PR #2's restore code won't understand our `VersionedSession` wrapper.

Three options:
1. **Rebase on PR #2** — we adopt their session format and add versioning on top; cleanest for maintainer
2. **Keep separate** — maintainer merges PR #2 first, then ours (we handle conflicts + add versioning)
3. **Combined PR** — merge both into one; simplest process, biggest blast radius

The versioned session format was added specifically to prevent format confusion, but it only works if both PRs agree on the format. This requires maintainer + PR #2 author coordination.

---

## File Change Summary

**Changes since last commit (hardening round 3):**
```
 Cargo.lock         |  84 ++++++-
 Cargo.toml         |   3 +
 default.nix        |  10 +-
 flake.nix          |   6 +-
 src/main.rs        | 503 ++++++++++++++++++++++++++++++------------
 src/proc.rs        | 309 +++++++++++++++++++++++----
 6 files changed, +768/-147
```

**Source code:** 1,689 lines total (`main.rs` 1,304 + `proc.rs` 385)  
**Tests:** 42 unit tests, all passing  
  - `main.rs`: 33 tests (shell escape: 7, terminal profiles: 6, restore commands: 8, spawn command: 2, SavedWindow: 4, session format: 3, config: 1, shell resolution: 2)
  - `proc.rs`: 9 tests (process tree walking with fake `/proc`)
**Build:** `nix build` clean  
**Lint:** `cargo clippy` zero warnings, `cargo fmt` clean  
**Flake:** `nix flake check` all checks pass (including overlay validation)  
**IPC version:** niri-ipc 25.5.1 (compatible with PR #2's 25.5.1)  
**Session format:** version 2 (backward compatible with legacy array format)

---

_Waiting for instructions._
