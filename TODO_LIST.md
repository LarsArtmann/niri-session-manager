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

| Task                                                                                                                           | Status       | Impact | Effort | Evidence                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------ | ------ | ------ | ------------------------------------------------------------------------------------------ |
| Release v0.5.0 — tag cut locally (`00424ca`, `v0.5.0`), awaiting push so SystemNix can pin the tag                             | 🔵 `BLOCKED` | High   | Low    | `CHANGELOG.md` [0.5.0]; push awaits maintainer go-ahead                                    |
| Soak-test reactive saves + idempotent restore on real hardware (daily driver) — the fake server cannot prove niri-event timing | 🔴 `TODO`    | High   | Low    | `run_reactive_save_session` in `src/save.rs`; all integration tests use `src/fake_niri.rs` |

## Medium Impact

| Task                                                                                                                         | Status       | Impact | Effort | Evidence                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------- | ------------ | ------ | ------ | ------------------------------------------------------------------------------- |
| Terminal ground truth (ROADMAP Q3): confirm which terminals run daily and give those profiles must-not-regress soak coverage | 🔵 `BLOCKED` | Medium | Low    | profiles doc-verified 2026-09-04; real-binary coverage pending maintainer input |

## Low Impact

| Task                                                                                                                                                  | Status    | Impact | Effort | Evidence                                                                                                                  |
| ----------------------------------------------------------------------------------------------------------------------------------------------------- | --------- | ------ | ------ | ------------------------------------------------------------------------------------------------------------------------- |
| Focus steal race: a window spawned after the saved-focused one can take focus (concurrent spawns); consider a final focus pass once all spawns settle | 🔴 `TODO` | Low    | Low    | `focus_window` runs per spawn task in `src/restore.rs` `spawn_single_window`; **pre-existing proven 2026-09-15**: at pre-change commit `8d2386b` the old arrival-order assertion failed 8/20 (release) and 7/20 (debug) runs on a multi-thread runtime — production spawn order was never saved-order |
| Surface captured-geometry coverage in `--health-check` (e.g. "N of M windows carry layout") — until restore applies v5 geometry, the captured data has no consumer | 🔴 `TODO` | Low    | Low    | `SavedWindowLayout` captured in `src/session.rs` `capture_session_json`; `run_health_check` in `src/main.rs` does not report it |
| Restore/save hardening tests batch: session JSON with unknown future keys still loads; corrupt-backup + valid-session interplay; marker pruning when `boot_id` is unreadable; `--dry-run` exit-code smoke in CI | 🔴 `TODO` | Low    | Low    | `SessionData` in `src/session.rs`; `should_restore_on_boot` / `find_latest_valid_backup` in `src/restore.rs`; smoke step in `.github/workflows/checks.yml` |
| CI: assert the test count so a refactor cannot silently drop tests (the module split proved the risk), and record the ~16s suite timing budget | 🔴 `TODO` | Low    | Low    | suite is 121 tests (+1 ignored) as of 2026-09-15; the split needed a manual fn-name diff to prove zero test loss |
| Stale-marker pruning when the session file vanishes — `should_restore_on_boot` currently prunes only markers from previous boot ids | 🔴 `TODO` | Low    | Low    | `should_restore_on_boot` in `src/restore.rs` |
| Release flow: decide 0.5.1 vs 0.6.0 for the `[Unreleased]` section and bump `Cargo.toml` (needs a maintainer decision; SystemNix pins the flake) | 🔴 `TODO` | Low    | Low    | `CHANGELOG.md` `[Unreleased]`; `Cargo.toml` still `0.5.0` |

---

_Verified 2026-09-15 (second pass) against code at 121 passing tests (+1 ignored benchmark). Resolved this round (see `CHANGELOG.md` [Unreleased]): exponential restore-retry backoff, spawn I/O moved to tokio's blocking pool, session-format v5 window-layout capture, and the `src/main.rs` module split (behavior-frozen changeset). Module-split verification debt is closed: suite looped 5× green, harness audited for arrival-order coupling (none beyond the fixed focus test), focus-race pre-existence proven at `8d2386b`, benchmark re-run unchanged (100.4 ms/window), CI-parity checks green locally, `--help` documents the tunables._
