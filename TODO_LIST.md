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

| Task                                                                                                                           | Status       | Impact | Effort | Evidence                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------ | ------ | ------ | ----------------------------------------------------------------------------------------- |
| Cut release 0.5.0 (idempotent restore, reactive saves, focus, v4 format) and let SystemNix pin it                              | 🔵 `BLOCKED` | High   | Low    | `CHANGELOG.md` [Unreleased]; tag/push awaits maintainer go-ahead                          |
| Soak-test reactive saves + idempotent restore on real hardware (daily driver) — the fake server cannot prove niri-event timing | 🔴 `TODO`    | High   | Low    | `reactive_save_session` in `src/main.rs`; all integration tests use `src/fake_niri.rs`    |
| Make the reactive save loop's polling-fallback branch testable (injectable interval) and add a test                            | 🔴 `TODO`    | High   | Medium | fallback loop sleeps `config.save_interval` (min 1 = 60s); currently untestable under 60s |

## Medium Impact

| Task                                                                                                                                         | Status       | Impact | Effort | Evidence                                                                                                                           |
| -------------------------------------------------------------------------------------------------------------------------------------------- | ------------ | ------ | ------ | ---------------------------------------------------------------------------------------------------------------------------------- |
| Graceful shutdown for the event-stream reader: socket read timeout or join-with-timeout instead of leaving it blocked until connection close | 🔴 `TODO`    | Medium | Low    | `drive_event_driven_saves` (`src/main.rs`); aborted task leaves a `spawn_blocking` reader blocked — harmless today, sloppy forever |
| Exponential backoff (capped) for event-stream reconnects instead of the fixed 1s                                                             | 🔴 `TODO`    | Medium | Low    | `reactive_save_session` reconnect `sleep(Duration::from_secs(1))`                                                                  |
| Terminal ground truth (ROADMAP Q3): confirm which terminals run daily and give those profiles must-not-regress soak coverage                 | 🔵 `BLOCKED` | Medium | Low    | profiles doc-verified 2026-09-04; real-binary coverage pending maintainer input                                                    |
| Consider splitting `src/main.rs` (~3400 lines) into modules (types / restore / save / cli) now that boundaries are clean                     | 🔴 `TODO`    | Medium | Medium | `AGENTS.md` documents the old two-file rule; the codebase outgrew it — needs a deliberate call                                     |

## Low Impact

| Task                                                                                                | Status    | Impact | Effort | Evidence                                                                 |
| --------------------------------------------------------------------------------------------------- | --------- | ------ | ------ | ------------------------------------------------------------------------ |
| Same-app spawn ORDER assertion in the fake-IPC harness (strict sequence, distinct from non-overlap) | 🔴 `TODO` | Low    | Low    | `SpawnLimiter` serializes same-app spawns; order is currently untested   |
| Harness test: window closed between restores → re-restore respawns exactly it on its workspace      | 🔴 `TODO` | Low    | Low    | fake server supports window removal via `set_windows`                    |
| Harness test: `--save-only` skips boot restore end-to-end                                           | 🔴 `TODO` | Low    | Low    | `RunMode::SaveOnly` dispatch in `main`                                   |
| Watch niri upstream for window-geometry IPC (prerequisite for window size capture)                  | 🔴 `TODO` | Low    | Low    | ROADMAP non-goal "window size capture"; re-evaluate on upstream releases |
| CI: cache cargo builds to cut the ~2 min proptest-tree rebuild per run                              | 🔴 `TODO` | Low    | Low    | `.github/workflows/checks.yml`                                           |

---

_Verified 2026-09-04 against code at 108 passing tests (+1 ignored benchmark). The previous list's 31 items are all resolved — see `CHANGELOG.md` [Unreleased] and `FEATURES.md` for the evidence trail._
