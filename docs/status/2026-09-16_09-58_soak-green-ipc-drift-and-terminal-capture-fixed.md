# Real-Hardware Soak: GREEN — IPC Drift, Terminal Capture, and Shutdown Hangs Fixed

**Date:** 2026-09-16 (session continued from `2026-09-16_02-10_real-hardware-soak-ipc-stream-death-discovery.md`)
**Task:** Execute the discovery report's P0/P1 plan: root-cause confirmation, the fix batch, and the green re-run of the real-hardware soak.
**Outcome:** **All three soak phases green on the daily driver** (`scripts/soak-test.sh all`: A 18/18, B 8/8, C 12/12), **six production bugs fixed** (the F1 outage plus five more the soak surfaced), suite at **131 tests** (was 124), fmt/clippy/`nix build`/`nix flake check` all green. v0.6.1 is release-prepped; tagging, pushing, and the SystemNix re-pin remain with the user.

---

## Root cause confirmed at byte level

Live capture of the up-front state-sync burst from niri unstable 2026-08-02 (47 lines, kept in `/tmp/nsm-soak/fixtures/`): line 7 is `{"CastsChanged":{"casts":[]}}` — an event variant niri-ipc **25.11.0 rejects** (unknown variant → deserialization error → stream torn down ~2 ms after subscribe). niri-ipc **26.4.0** (newest release, 2026-04-25) parses all 46 event lines. The other 2026-09-15 mystery (F3) dissolved: the manager sends valid quoted-JSON bare-string requests (`b'"EventStream"'`, byte-captured); the old probe had sent unquoted, invalid JSON.

Fix policy (the report's question 1): **both** — pin `niri-ipc = "=26.4.0"` (exact pin per the crate's recommendation) AND tolerant event parsing, because the daily driver runs rolling unstable niri that will drift past 26.4 eventually. Tolerance treats an undecodable line as WARN-logged (bounded), conservatively layout-relevant, and SKIPPED — the stream must survive protocol drift.

## Fixes shipped (all in `CHANGELOG.md` [Unreleased])

| # | Bug | Fix |
| --- | --- | --- |
| F1 | One unknown event variant killed the stream; zero event-driven saves in production for 33 h | Tolerant reader (`event_reader`, `src/save.rs`) + niri-ipc `=26.4.0` |
| F5a | Stream dying mid-debounce dropped the pending save | Flush the debounced save before reconnecting |
| F5b | Subscribe-ok-then-instant-death looped forever without saving | Periodic saves mixed between reconnects after 3 consecutive sub-5s streams (`RAPID_DEATH_FALLBACK_THRESHOLD`) |
| F6 | The parse error was swallowed | Bounded WARN per unparsable line (3 logged, 200 chars, rest summarized) |
| F7 | A niri that accepts but never replies hung the daemon past SIGTERM | 5 s read/write timeout on all request/reply IPC + explicit `process::exit` after the service loop |
| F9 (new) | `/proc/.../children` reports zero children for NixOS `.ghostty-wrapper` fork shapes — terminal state was `null` on EVERY real capture | Stat-ppid scan fallback (`get_children_at`, `src/proc.rs`); ghostty now captures `child_command`/`child_cwd` live |
| F10 (new) | kitty spawns leaf helper kittens (`__atexit__`, `__watch_conf__`) that sorted first; the walker descended into a helper dead-end | Prefer non-shell/non-helper children over helpers when tpgid does not disambiguate |

F9 and F10 explain why the deployed 0.4.1's session files never contained terminal state: capture has been silently broken on real hardware all along (the fake proc trees in tests are too clean).

## Real-hardware evidence (2026-09-16, niri unstable 2026-08-02, live desktop)

- **Phase A (reactive saves)**: burst→save ~2 s after subscribe; focus clusters collapse to ≤1 save; window open/close each save; 50 s idle = zero saves + unchanged mtime; zero "event stream ended" lines (the F1 regression proof); zero unparsable-line warnings; clean SIGTERM exit 0 with final save. 11 event-driven saves per run (pre-fix: 0).
- **Phase B (fidelity)**: every live window matches its saved entry on app_id, workspace idx+name, focus flag, and v5 layout; ghostty windows carry real `terminal_state`.
- **Phase C (idempotent restore)**: dry-run writes nothing; `--restore` spawns **exactly the dead carrier** (a real kitty window whose captured command is restored and re-run — first real-binary exercise of a terminal profile on hardware); immediate re-restore spawns 0; the boot gate skips the third run; saved focus restored via the final focus pass.
- Methodology is encoded in `scripts/soak-test.sh` (three selectable phases, scratch XDG isolation, self-closing carriers, machine-checkable assertions). Lessons baked into its design: wait for the carrier to die before restoring, reset the boot-gate marker, close leftover carriers (the restore composition deliberately ends with `; exec $SHELL`, so restored terminals stay open), and use an app the user doesn't run (kitty) for the deficit carrier.

## Test and gate inventory

- 131 tests + 1 ignored benchmark (was 124): state-sync burst + poison line vs the live-stream reader, stream-death flush, rapid-death fallback, never-replying-socket bounded shutdown, live-capture fixture (`src/testdata/niri-event-stream-2026-08-02.jsonl`, sanitized, guards the niri-ipc pin), children-file fallback, kitty helper ordering. CI `expected=131`. Suite looped 5× green; fmt + `cargo clippy --all-features --all-targets` clean; `nix build` + `nix flake check` green (the fixture required a `.jsonl` case in `default.nix`'s source filter and git-staging — nix only sees tracked files).
- Environment note: the shared `/mnt/buildcache` cargo registry hit 100 % this session; work proceeded with a private `CARGO_HOME=/home/lars/.cache/nsm-cargo-home`. The mount still needs attention (17 G emergency swapfile, 114 G rust toolchains live there).

## Open items (moved to `TODO_LIST.md`)

- Unattended durability soak (30–60 min or overnight via the deployed service) after the v0.6.1 deploy — the user's real acceptance check is a fresh `session.json` mtime within minutes of the upgrade.
- ROADMAP Q3 narrows: ghostty + kitty now have real-binary coverage; foot/wezterm/alacritty remain doc-verified.
- The 2026-09-15 report's P2 hardening list (health-check staleness warning, reconnect rate-limited logging, `--protocol-probe` subcommand) remains open.
