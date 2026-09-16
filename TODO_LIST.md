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

_(none — the real-hardware soak resolved 2026-09-16 together with the IPC-drift fix batch it discovered; see `CHANGELOG.md` [0.6.1] and `docs/status/` 2026-09-16 reports)_

## Medium Impact

| Task                                                                                                                                                                                           | Status       | Impact | Effort | Evidence                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ | ------ | ------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| Terminal ground truth (ROADMAP Q3): ghostty now has real-binary soak coverage (capture + carrier restore, 2026-09-16); confirm which OTHER terminals run daily and give them the same coverage | 🔵 `BLOCKED` | Medium | Low    | profiles doc-verified 2026-09-04; ghostty verified live 2026-09-16 via `scripts/soak-test.sh`; other terminals pending maintainer input |
| Unattended durability soak: 30–60 min (or overnight via the deployed service after the v0.6.1 upgrade) accumulating long-run evidence that the fix holds                                       | 🔴 `TODO`    | Medium | Low    | `scripts/soak-test.sh` covers ~4 min interactively; longer windows need the deployed service                                            |

## Low Impact

| Task                                                                                                                                                              | Impact | Effort | Evidence |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ | ------ | -------- |
| _(none — the 0.5.1-vs-0.6.0 decision resolved 2026-09-15: cut as **v0.6.0** because `[Unreleased]` carried features, not just fixes; see `CHANGELOG.md` [0.6.0])_ | —      | —      | —        |

---

_Verified 2026-09-16 against code at 131 passing tests (+1 ignored benchmark): the real-hardware soak item completed (Phase A 18/18, B 8/8, C 12/12 — `scripts/soak-test.sh`), resolving together with the six IPC-drift/resilience/terminal-capture fixes it surfaced (see `CHANGELOG.md` [0.6.1]). **Deployed-service alert (observed 12:40): the service was re-pinned and restarted 2026-09-16 08:25 — but to v0.6.0, which predates the fix batch; its `session.json` has been stale since 08:25 (F1 recurrence). Re-pin to v0.6.1 once tagged.**_

_Verified 2026-09-15 (third pass) against code at 124 passing tests (+1 ignored benchmark). Resolved this round (see `CHANGELOG.md` [0.6.0]): the focus steal race (final focus pass after all spawns settle, ordering pinned by the harness focus test), stale-marker pruning when the session file vanishes, `--health-check` layout-coverage reporting, the hardening-test batch (unknown future keys, valid-session-over-corrupt-backups, unreadable boot id), the CI test-count guard, the CI `--dry-run` exit-code smoke, and the release-flow decision (v0.6.0 cut 2026-09-15: fmt/clippy/124-test suite/nix build + flake check/docs-citations all green pre-tag; `--version` reports 0.6.0 in cargo and Nix builds). Tags `v0.5.0` + `v0.6.0` pushed 2026-09-15 — SystemNix can pin. Post-push discovery resolved 2026-09-15: Actions were enabled and the first real run (workflow_dispatch `34990898129`) passed end-to-end in 5m27s at `4256880`; the Nix Magic Cache save/restore flaked (GitHub cache-service 400s) non-fatally. Previous round: exponential restore-retry backoff, spawn I/O on tokio's blocking pool, session-format v5 window-layout capture, `src/main.rs` module split (behavior-frozen), suite looped 5× green, focus-race pre-existence proven at `8d2386b`, benchmark unchanged (100.4 ms/window)._
