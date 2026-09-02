# TODO List

> Short-term, actionable, bounded work items, verified against the actual code.
> For long-term vision and unrefined ideas, use ROADMAP.md.
> Items are ranked by impact. Status is verified, not assumed.

## Status legend

| Status           | Meaning                                                     |
| ---------------- | ----------------------------------------------------------- |
| 🔴 `TODO`        | Not started. Needs doing.                                   |
| 🟡 `IN_PROGRESS` | Actively being worked on.                                   |
| 🔵 `BLOCKED`     | Cannot proceed, external dependency or decision needed.     |
| 🟢 `DONE`        | Completed. Remove from this list and log in `CHANGELOG.md`. |

## High Impact

| Task                                                                                          | Status     | Impact | Effort | Evidence                                                                                    |
| --------------------------------------------------------------------------------------------- | ---------- | ------ | ------ | ------------------------------------------------------------------------------------------- |
| Count-based idempotent restore: spawn `saved − running` windows per app instead of all-or-nothing | 🔵 `BLOCKED` | High   | Medium | `src/main.rs:633` `restore_session_internal`; selection semantics pending (ROADMAP Q2)      |
| Serialize spawns per app_id — two concurrently restored windows of one app can claim each other's windows and land on swapped workspaces | 🔴 `TODO`  | High   | Medium | `src/main.rs:745` global `Semaphore` only; niri `Action::Spawn` returns no window id         |
| Extract boot-gate + shutdown sequence into testable units; add regression tests for the 3 fixed dry-run/shutdown bugs | 🔴 `TODO`  | High   | Medium | `src/main.rs:1090`–`1140` lives in `main()`, untestable; fixed in 0.4.0, can silently regress |
| Integration test harness with a fake niri IPC socket (Unix listener speaking the IPC protocol) | 🔴 `TODO`  | High   | High   | all IPC paths (`src/main.rs:633`+) untested; would have caught the 0.4.0 shutdown/dry-run bugs |
| Version bump 0.4.0 → 0.4.1 + release notes; SystemNix pins the flake and can't see the 7 behavior fixes without it | 🔵 `BLOCKED` | High | Low    | `Cargo.toml:3`; release policy pending (ROADMAP Q1)                                          |
| Reactive saves via niri event-stream subscription (save on layout change instead of polling)   | 🔴 `TODO`  | High   | High   | `src/main.rs:896` `periodic_save_session` polls; requested in both status reports            |

## Medium Impact

| Task                                                                                          | Status    | Impact | Effort | Evidence                                                                      |
| --------------------------------------------------------------------------------------------- | --------- | ------ | ------ | ----------------------------------------------------------------------------- |
| Return a restore outcome (`Restored`/`WouldRestore`/`Failed`) from `restore_session_internal` to kill the scattered `config.dry_run` checks | 🔴 `TODO` | Medium | Medium | `src/main.rs:633`; dry-run branching spread across restore path                |
| fsync the parent directory in `atomic_write` — rename may not survive power loss               | 🔴 `TODO` | Medium | Low    | `src/main.rs:517`; reverts to previous session after power loss, not corruption |
| Verify terminal restore flags against real CLIs (wezterm `start --cwd`, ghostty `-e`, foot/kitty/alacritty) | 🔴 `TODO` | Medium | Low    | `src/main.rs:432` `build_terminal_restore_command`; unit tests assert what code *builds*, not what terminals *accept* |
| Focus restoration — `SavedWindow.is_focused` is saved but never used on restore                 | 🔴 `TODO` | Medium | Medium | `SavedWindow` struct in `src/main.rs`; gap noted since v0.3.0                  |
| Add `maxRestoreWindows` NixOS module option (only 5 of 7 CLI flags mirrored; `dryRun` stays CLI-only by design) | 🔴 `TODO` | Medium | Low    | `module.nix:33`–`60` vs `src/main.rs:1032`–`1057`                              |
| Restore retry loop test with injectable failure injection                                       | 🔴 `TODO` | Medium | Medium | retry loop inside `restore_session_internal` (`src/main.rs:633`) untested      |
| Multi-monitor robustness: match workspace outputs by position/EDID, not name (monitor port changes break placement) | 🔴 `TODO` | Medium | High   | `WorkspaceInfo.output` matching in restore path                                |

## Low Impact

| Task                                                                 | Status    | Impact | Effort | Evidence                                                        |
| -------------------------------------------------------------------- | --------- | ------ | ------ | --------------------------------------------------------------- |
| `cargo audit` + dependency refresh (niri-ipc 25.5.1; tool not installed locally) | 🔴 `TODO` | Low    | Low    | `Cargo.toml`; cargo-audit absent from devshell                  |
| Tests for `cleanup_old_backups`                                       | 🔴 `TODO` | Low    | Low    | `src/main.rs:931` area; untested                                |
| `dedupe_single_instance_windows` PID-crossing-app edge-case tests     | 🔴 `TODO` | Low    | Low    | `src/main.rs:533`                                               |
| Property tests for `SessionData`/`SavedWindow` serialization round-trips | 🔴 `TODO` | Low  | Medium | round-trip tests exist but are example-based, not property-based |
| Snapshot test for dry-run output format                               | 🔴 `TODO` | Low    | Low    | dry-run printing path untested                                  |
| `--config-file` CLI override for the config path                      | 🔴 `TODO` | Low    | Low    | config path hardcoded to XDG in `main`                          |
| `restore-marker` staleness cleanup (marker survives boots forever)    | 🔴 `TODO` | Low    | Low    | marker beside `session.json`, never pruned                      |
| Warn when `terminal_state.enabled` but zero terminals matched during save | 🔴 `TODO` | Low  | Low    | save path in `src/main.rs`; silent no-op today                  |
| `--version` smoke test in CI                                          | 🔴 `TODO` | Low    | Low    | `.github/workflows/checks.yml`                                  |
| `--health-check` subcommand (verify service liveness beyond "process running") | 🔴 `TODO` | Low | Low  | no health surface today                                         |
| CI badge in README                                                    | 🔴 `TODO` | Low    | Low    | `.github/workflows/checks.yml` exists, badge missing            |
| CONTRIBUTING.md                                                       | 🔴 `TODO` | Low    | Low    | missing                                                         |
| cargo-deny supply-chain check in CI                                   | 🔴 `TODO` | Low    | Low    | `.github/workflows/checks.yml`                                  |
| Workspace `idx = Some(0)` from legacy files: decide clamp vs skip (niri is 1-based) | 🔴 `TODO` | Low | Low | restore workspace-move guard in `src/main.rs`                   |
| Sanity-check `max_walk_depth` upper bound in config validation        | 🔴 `TODO` | Low    | Low    | `terminal_state` config validation                              |
| Example `session.json` in `docs/`                                     | 🔴 `TODO` | Low    | Low    | format documented only in code                                  |
| `--restore` / `--save-only` explicit run modes (currently implicit)   | 🔴 `TODO` | Low    | Low    | `Config` struct `src/main.rs:1032`                              |

---

*Harvested 2026-09-03 from `docs/status/2026-09-03_00-00_bug-review-and-agents-md.md` and `docs/status/2026-07-03_10-38_reliability-hardening-and-v0.3.0.md` (now archived under `docs/status/archived/`). Every item verified against code on this date.*
