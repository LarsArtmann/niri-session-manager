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
| Push tags `v0.5.0` + `v0.6.0` so SystemNix can pin (`v0.5.0` cut at `00424ca`; `v0.6.0` cut 2026-09-15)                        | 🔵 `BLOCKED` | High   | Low    | `CHANGELOG.md` [0.6.0]; `git tag -l`; push awaits maintainer go-ahead                      |
| Soak-test reactive saves + idempotent restore on real hardware (daily driver) — the fake server cannot prove niri-event timing | 🔴 `TODO`    | High   | Low    | `run_reactive_save_session` in `src/save.rs`; all integration tests use `src/fake_niri.rs` |

## Medium Impact

| Task                                                                                                                         | Status       | Impact | Effort | Evidence                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------- | ------------ | ------ | ------ | ------------------------------------------------------------------------------- |
| Terminal ground truth (ROADMAP Q3): confirm which terminals run daily and give those profiles must-not-regress soak coverage | 🔵 `BLOCKED` | Medium | Low    | profiles doc-verified 2026-09-04; real-binary coverage pending maintainer input |

## Low Impact

| Task | Impact | Effort | Evidence |
| --- | --- | --- | --- |
| _(none — the 0.5.1-vs-0.6.0 decision resolved 2026-09-15: cut as **v0.6.0** because `[Unreleased]` carried features, not just fixes; see `CHANGELOG.md` [0.6.0])_ | — | — | — |

---

_Verified 2026-09-15 (third pass) against code at 124 passing tests (+1 ignored benchmark). Resolved this round (see `CHANGELOG.md` [0.6.0]): the focus steal race (final focus pass after all spawns settle, ordering pinned by the harness focus test), stale-marker pruning when the session file vanishes, `--health-check` layout-coverage reporting, the hardening-test batch (unknown future keys, valid-session-over-corrupt-backups, unreadable boot id), the CI test-count guard, the CI `--dry-run` exit-code smoke, and the release-flow decision (v0.6.0 cut 2026-09-15: fmt/clippy/124-test suite/nix build + flake check/docs-citations all green pre-tag; `--version` reports 0.6.0 in cargo and Nix builds). Previous round: exponential restore-retry backoff, spawn I/O on tokio's blocking pool, session-format v5 window-layout capture, `src/main.rs` module split (behavior-frozen), suite looped 5× green, focus-race pre-existence proven at `8d2386b`, benchmark unchanged (100.4 ms/window)._
