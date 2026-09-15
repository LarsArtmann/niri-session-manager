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
| Release v0.5.0 — tag cut locally (`00424ca`, `v0.5.0`), awaiting push so SystemNix can pin the tag                              | 🔵 `BLOCKED` | High   | Low    | `CHANGELOG.md` [0.5.0]; push awaits maintainer go-ahead                                |
| Soak-test reactive saves + idempotent restore on real hardware (daily driver) — the fake server cannot prove niri-event timing | 🔴 `TODO`    | High   | Low    | `run_reactive_save_session` in `src/save.rs`; all integration tests use `src/fake_niri.rs` |

## Medium Impact

| Task                                                                                                                          | Status       | Impact | Effort | Evidence                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------ | ------ | ------ | -------------------------------------------------------------------------------- |
| Terminal ground truth (ROADMAP Q3): confirm which terminals run daily and give those profiles must-not-regress soak coverage | 🔵 `BLOCKED` | Medium | Low    | profiles doc-verified 2026-09-04; real-binary coverage pending maintainer input |

---

_Verified 2026-09-15 against code at 118 passing tests (+1 ignored benchmark). Resolved this round (see `CHANGELOG.md` [Unreleased]): exponential restore-retry backoff, spawn I/O moved to tokio's blocking pool, session-format v5 window-layout capture, and the `src/main.rs` module split (behavior-frozen changeset). The previous list's remaining items are unchanged._
