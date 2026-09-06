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

| Task                                                                                                                           | Status       | Impact | Effort | Evidence                                                                               |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------ | ------ | ------ | -------------------------------------------------------------------------------------- |
| Cut release 0.5.0 (idempotent restore, reactive saves, graceful shutdown, backoff) and let SystemNix pin it                    | 🔵 `BLOCKED` | High   | Low    | `CHANGELOG.md` [Unreleased]; tag/push awaits maintainer go-ahead                       |
| Soak-test reactive saves + idempotent restore on real hardware (daily driver) — the fake server cannot prove niri-event timing | 🔴 `TODO`    | High   | Low    | `reactive_save_session` in `src/main.rs`; all integration tests use `src/fake_niri.rs` |

## Medium Impact

| Task                                                                                                                                                                                               | Status       | Impact | Effort | Evidence                                                                                                                           |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ | ------ | ------ | ---------------------------------------------------------------------------------------------------------------------------------- |
| Terminal ground truth (ROADMAP Q3): confirm which terminals run daily and give those profiles must-not-regress soak coverage                                                                       | 🔵 `BLOCKED` | Medium | Low    | profiles doc-verified 2026-09-04; real-binary coverage pending maintainer input                                                    |
| Move `spawn_single_window`'s blocking niri I/O off the async runtime (`spawn_blocking` or async client)                                                                                            | 🔴 `TODO`    | Medium | Low    | `spawn_single_window` calls `Socket::send` inline; a current-thread runtime serializes all spawns behind it (hit in the new tests) |
| Split `src/main.rs` (~3600 lines) into modules (types / restore / save / cli) — deliberate call made 2026-09-06: yes, but as a dedicated behavior-frozen changeset, not mixed into behavioral work | 🔴 `TODO`    | Medium | Medium | module boundaries verified during the shutdown/backoff rework; keep the split reviewable on its own                                |

## Low Impact

| Task                                                                                                      | Status    | Impact | Effort | Evidence                                                                                                                                  |
| --------------------------------------------------------------------------------------------------------- | --------- | ------ | ------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Capture `WindowLayout` (window size / scroll position) in the session file — upstream prerequisite landed | 🔴 `TODO` | Low    | Medium | niri-ipc `Window.layout` (geometry exposed upstream since v25.08; pinned crate 25.11 already carries it) — needs session-format v5 design |
| Spawn-timeout exponential backoff in the restore retry loop                                               | 🔴 `TODO` | Low    | Low    | fixed `--retry-delay` between attempts today                                                                                              |

---

_Verified 2026-09-06 against code at 114 passing tests (+1 ignored benchmark). Resolved this round (see `CHANGELOG.md` [Unreleased]): polling-fallback testability + test, graceful event-reader shutdown, capped reconnect backoff, same-app spawn sequencing test, window-closed re-restore test, `--save-only` end-to-end test, niri upstream geometry watch (landed), CI cargo caching. The previous list's remaining items are unchanged._
