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

| Task                                                                                                                                                                                                       | Status       | Impact | Effort | Evidence                                                                                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ | ------ | ------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Terminal ground truth (ROADMAP Q3): which terminals run daily? ghostty/kitty/foot/alacritty all have live carrier-restore coverage; wezterm blocked on install. Daily-driver picks become must-not-regress | 🔵 `BLOCKED` | Medium | Low    | `CARRIER=... scripts/soak-test.sh c` 2026-09-16: kitty 12/12, foot 12/12, alacritty 13/13 (after fixing the `Alacritty` app_id casing bug, see CHANGELOG [Unreleased]); maintainer input still needed |
| Overnight durability soak via the DEPLOYED service (the local 35-min `--save-only` leg ran green 2026-09-16; the deployed-service leg follows the v0.6.1 re-pin)                                           | 🔵 `BLOCKED` | Medium | Low    | local leg: `/tmp/nsm-durability` (PID-monitored, saves + backups flowing, clean SIGTERM); deployed leg blocked on tag/push + SystemNix re-pin                                                         |

## Low Impact

| Task                                                                                                                                                              | Impact | Effort | Evidence |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ | ------ | -------- |
| _(none — the 0.5.1-vs-0.6.0 decision resolved 2026-09-15: cut as **v0.6.0** because `[Unreleased]` carried features, not just fixes; see `CHANGELOG.md` [0.6.0])_ | —      | —      | —        |

---

_Verified 2026-09-16 (second pass) against code at 139 passing tests (+1 ignored benchmark): the repo-tail batch from the 12-48 report is done — `--protocol-probe` (live-verified), health-check staleness warning, rate-limited flapping summary, drift-guard canary workflow, markdownlint enforcement (CI + devshell, repo clean), README compat + `; exec $SHELL` docs, wire-format/fuzz/pin tests, `--save-once` end-to-end test, benchmark refresh (100.4 ms/window unchanged), and carrier coverage for foot + alacritty — which surfaced and fixed a real bug: niri reports alacritty's app_id as `Alacritty`, so terminal state was never captured and restore spawned a nonexistent binary (see `CHANGELOG.md` [Unreleased]). **Deployed-service alert (still open, observed 12:40): the service was re-pinned and restarted 2026-09-16 08:25 — but to v0.6.0, which predates the fix batch; its `session.json` has been stale since 08:25 (F1 recurrence in production). Tag + push v0.6.1, re-pin SystemNix, restart; acceptance = fresh session.json mtime. If you run alacritty, also add `"Alacritty" = ["alacritty"]` to your config.toml's `[app_mappings]` and `"Alacritty"` to `terminal_app_ids`.**_

_Verified 2026-09-15 (third pass) against code at 124 passing tests (+1 ignored benchmark). Resolved this round (see `CHANGELOG.md` [0.6.0]): the focus steal race (final focus pass after all spawns settle, ordering pinned by the harness focus test), stale-marker pruning when the session file vanishes, `--health-check` layout-coverage reporting, the hardening-test batch (unknown future keys, valid-session-over-corrupt-backups, unreadable boot id), the CI test-count guard, the CI `--dry-run` exit-code smoke, and the release-flow decision (v0.6.0 cut 2026-09-15: fmt/clippy/124-test suite/nix build + flake check/docs-citations all green pre-tag; `--version` reports 0.6.0 in cargo and Nix builds). Tags `v0.5.0` + `v0.6.0` pushed 2026-09-15 — SystemNix can pin. Post-push discovery resolved 2026-09-15: Actions were enabled and the first real run (workflow_dispatch `34990898129`) passed end-to-end in 5m27s at `4256880`; the Nix Magic Cache save/restore flaked (GitHub cache-service 400s) non-fatally. Previous round: exponential restore-retry backoff, spawn I/O on tokio's blocking pool, session-format v5 window-layout capture, `src/main.rs` module split (behavior-frozen), suite looped 5× green, focus-race pre-existence proven at `8d2386b`, benchmark unchanged (100.4 ms/window)._
