# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **`--protocol-probe` self-diagnosis mode**: round-trips a Version request, then reads the head of a fresh event-stream subscription and reports exactly which lines the pinned niri-ipc cannot parse — a seconds-check for protocol drift after a niri upgrade (`run_protocol_probe` in `src/main.rs`; live-verified against real niri unstable 2026-08-02: 6 burst lines, no drift).
- **Health-check staleness warning**: `--health-check` now warns when the session file is older than 2 × the save interval — the in-service signature of a save loop that silently stopped saving (the F1 recurrence detector; `session_staleness_warning` in `src/main.rs`).
- **Rate-limited stream-flapping summary**: every 10 event-stream deaths the save loop logs one WARN health summary (total deaths, rapid-death streak, next reconnect delay) so chronic flapping is visible without journal diving (`flapping_summary` in `src/save.rs`).
- **niri-ipc drift-guard canary workflow** (`.github/workflows/drift-guard.yml`): weekly (and on demand) repins niri-ipc to the newest published release and runs the drift-sensitive tests (live-capture fixture, tolerant-reader fuzz, wire-format pins, protocol probe) — `continue-on-error`, a signal rather than a release blocker.
- **Markdownlint enforcement**: a CI step plus devshell entry for `markdownlint-cli` (`.markdownlint.json` was previously advisory — nothing ran it); the repo is clean under it, and a `.markdownlintignore` excludes `target/`.
- **Drift-resilience tests** (suite now 139 + 1 ignored benchmark): a proptest pins that arbitrary event-stream lines never kill the reader (classify as serde does, EOF is the only death), the quoted bare-string request wire format is byte-pinned, `request_reply` round-trips over a real socketpair, and the exact-pin policy itself is asserted against Cargo.toml; plus an end-to-end `--save-once` test (the suspend-hook path).
- **Soak-script carrier override** (`CARRIER=foot|wezterm|alacritty|kitty` for `scripts/soak-test.sh` phase C): per-carrier launch argv and niri app_id mapping; foot and alacritty carrier restores verified live 2026-09-16 (12/12 assertions each, same as kitty).

### Changed

- **Benchmark re-verified after the IPC rewrite**: restore burst remains 100.4 ms/window (30 windows / 3.013 s) after the raw-UnixStream client, the 5s timeouts, and the niri-ipc `=26.4.0` repin (`docs/benchmarks/restore-burst.md`).
- **README niri-version compatibility section** documenting the exact-pin + tolerant-reader strategy, the `--protocol-probe` check, and the `; exec $SHELL` terminal-restore composition (restored terminals stay open after their command exits).

### Fixed

- **alacritty windows never captured terminal state, and restore spawned a nonexistent binary**: niri reports alacritty's app_id capitalized (`Alacritty`), but the default `terminal_app_ids` only listed lowercase — the capture walk never descended alacritty process trees, and restore used the app_id verbatim as the launch command (`Alacritty`, no such executable), which timed out after 5s and restored nothing. Both spellings now ship in `default_terminal_app_ids`, and the default config template maps `"Alacritty" = ["alacritty"]` (found live 2026-09-16 via the `CARRIER=alacritty` soak; existing config.toml files must add the entry by hand). The soak script also now waits for the carrier window to appear before capturing — a cold alacritty first launch previously outran the fixed 3s pre-capture sleep.

## [0.6.1] - 2026-09-16

### Added

- **First real CI execution (2026-09-15)**: the `Checks` workflow ran end-to-end on GitHub's runner for the first time (workflow_dispatch, run `34990898129`, green at `4256880` in 5m27s) — build, test-count assertion, clippy, fmt, dry-run smoke, nix, cargo-deny, and docs-citations all executed remotely; all prior CI evidence was local parity because the fork had Actions disabled.
- **Real-hardware soak test** (`scripts/soak-test.sh`): a three-phase live-niri soak (A: reactive-save timing with burst/debounce/idle/open-close assertions; B: saved-session-vs-live-state fidelity; C: idempotent-restore proof with a real self-closing carrier window). First fully green end-to-end run 2026-09-16 on niri unstable 2026-08-02: Phase A 18/18, Phase B 8/8, Phase C 12/12 — deficit restore spawns exactly the dead carrier, immediate re-restore spawns 0, the boot gate holds, saved focus is restored.
- **Regression tests for the IPC-drift class** (suite now 131 + 1 ignored benchmark): the fake niri server emulates real niri's up-front state-sync burst (including the `CastsChanged` line that killed production streams and an unknown-variant poison line), instant stream-death injection, and rapid-death flapping; a sanitized live capture is checked in as `src/testdata/niri-event-stream-2026-08-02.jsonl` and must parse under the pinned niri-ipc; a never-replying-socket test pins bounded shutdown; a proc-tree test pins the children-file fallback.

### Changed

- **niri-ipc pinned to exactly `=26.4.0`** (was `"25.5.1"` resolving to 25.11.0): the crate follows niri's own versioning and recommends exact pins so patch releases cannot shift IPC semantics silently. 26.4.0 understands the event variants current unstable niri emits (verified against a live capture).
- **All-targets clippy gate**: the three test modules (`src/fake_niri.rs`, `src/tests.rs`, and proc.rs's inline `mod tests`) now carry scoped clippy exemptions, so `cargo clippy --all-features --all-targets` is fully clean — previously it hard-failed with 231 deny-level errors from tests legitimately using `unwrap`/`expect`/indexing/panics. CI's testless clippy invocation is unchanged, and the footgun denies (`todo`/`unimplemented`/`exit`/`unreachable`/`string_slice`/`panic_in_result_fn`) still apply inside tests.

### Fixed

- **Reactive saves were silently dead on niri newer than the pinned niri-ipc** (found on the daily driver 2026-09-15, production down for 33 h): niri pushes a full state-sync burst immediately after accepting an `EventStream` subscription, and a single line the pinned crate cannot deserialize — `CastsChanged`, added after niri-ipc 25.11 — killed the stream ~2 ms after every subscribe. Because each subscribe succeeded, the polling fallback never engaged, and the reader died before the debounce settled, so **no event-driven save ever fired**; the deployed 0.4.1 service exhibited the identical failure. The event reader now parses tolerantly: an undecodable line is WARN-logged (bounded, first 200 chars, max 3 per connection plus a suppressed count) and conservatively treated as layout-relevant, and the stream keeps reading (`event_reader` in `src/save.rs`).
- **A dying stream no longer drops a pending debounced save**: when the reader dies mid-debounce after layout-relevant events were delivered, the save is flushed before reconnecting instead of being silently lost (`drive_event_driven_saves` in `src/save.rs`).
- **Streams that subscribe successfully but die instantly no longer loop forever without saving**: after 3 consecutive subscriptions that lived under 5 s, the save loop mixes periodic saves (at the configured interval) between reconnect attempts, mirroring the existing subscribe-refusal fallback (`RAPID_DEATH_FALLBACK_THRESHOLD` in `src/save.rs`).
- **A wedged niri can no longer hang the process at shutdown**: all request/reply IPC now carries a 5 s read/write timeout (`IPC_REQUEST_TIMEOUT` in `src/ipc.rs`), and `main` exits the process explicitly after the service loop finishes — previously a socket that accepted connections but never replied parked a blocking-pool thread forever, and the runtime drop (which joins blocking threads) hung the daemon past SIGTERM until SIGKILL (observed live 2026-09-15).
- **Terminal-state capture silently returned nothing on real NixOS process trees**: the `/proc` walker discovered children exclusively via `/proc/<pid>/task/<pid>/children`, which reports zero children for the `.ghostty-wrapper` fork shape used on NixOS even while the child exists and points back via ppid — so every capture on the daily driver recorded `terminal_state: null` (the deployed 0.4.1 shows the same). The walker now falls back to scanning `/proc/*/stat` ppids (the ground truth `ps` uses) when the children file yields nothing (`get_children_at` in `src/proc.rs`); verified live: ghostty windows now capture `child_command`/`child_cwd`, and the restored carrier re-runs its captured command.
- **Terminal-state capture mis-descended into kitty's helper kittens**: kitty always spawns leaf helpers (`kitten __atexit__`, `kitten __watch_conf__`) beside the real child, and they sort first by pid — with no foreground (tpgid) match to disambiguate, the walker descended into the first child (a helper dead-end) and returned nothing, so no kitty window ever captured state. The walk now prefers non-shell/non-helper children over helpers before falling back to the first child (`resolve_child_process_at` in `src/proc.rs`); verified live with a real kitty carrier captured and restored.

## [0.6.0] - 2026-09-15

### Added

- **Focus restoration via a final focus pass**: focus is applied once, after every spawn task has settled, instead of per-spawn — a window appearing after the saved-focused one can no longer steal focus back (`spawn_windows` in `src/restore.rs`). The pre-existing race was proven at commit `8d2386b`; the ordering invariant is now pinned by the harness focus test.
- **Restore-marker pruning when the session file vanishes**: a marker matching the current boot whose session file is gone no longer blocks a re-restore — the gate prunes the orphaned marker and restores (a fresh restore just seeds a new session from the current state). Markers are still left alone when the boot id itself is unreadable, since there is nothing to compare against (`should_restore_on_boot` in `src/restore.rs`).
- **Health-check layout coverage**: `--health-check` now reports how many saved windows carry captured v5 layout data, e.g. "session file: 12 window(s), 9 with captured layout" (`run_health_check` in `src/main.rs`).
- **CI hardening**: the workflow asserts the passing test count (a refactor can no longer silently drop tests) and runs a `--dry-run` smoke without niri, locking in that restore failure stays non-fatal (`.github/workflows/checks.yml`).
- **Hardening tests**: session JSON with unknown future keys still loads, a valid session file wins over corrupt backups, and an unreadable boot id restores while leaving the marker in place (`src/tests.rs`) — the suite is at 124 tests (+1 ignored benchmark).
- **Window layout capture (session format v5)**: each saved window now records its scrolling-layout slot (`scroll_position`: 1-based `column` + `tile_in_column`) and visible tile size (`tile_width`/`tile_height` in logical pixels), captured from niri-ipc 25.11's `Window.layout` (`SavedWindowLayout::from_niri` in `src/session.rs`). Viewport-relative and Wayland-internal geometry is deliberately dropped. Pre-v5 files still load (the key defaults to absent); `WindowLayoutsChanged` events trigger reactive saves. Restore does not apply the geometry yet — that design waits for a real-hardware soak test.
- **Exponential retry backoff in the restore loop**: `--retry-delay` is now the base delay; each failed attempt doubles it, capped at 30s (`next_retry_delay` in `src/restore.rs`), mirroring the event-stream reconnect backoff. A `--retry-delay` of 0 still retries immediately.

### Changed

- **All blocking niri IPC now runs on tokio's blocking pool**: `spawn_single_window` and `apply_window_placement` route through `niri_send` (`spawn_blocking`) instead of issuing inline `Socket` I/O — a current-thread runtime no longer serializes every spawn behind one worker.
- **`src/main.rs` (~3.7k lines) split into focused modules** — `config`, `ipc`, `session`, `terminal`, `restore`, `save`, plus a dedicated `tests` module — as a behavior-frozen changeset: no logic changes; the suite passes unchanged at 118 tests (+1 ignored benchmark), up from 114 via four new tests (retry-backoff units, layout capture end-to-end, v4-file compat, `WindowLayoutsChanged` relevance).
- **serde_json `float_roundtrip` enabled**: the session format now carries `f64` tile sizes, and the feature guarantees lossless JSON float round-trips (locked in by the property tests).

## [0.5.0] - 2026-09-14

### Added

- **Idempotent restore**: running windows are matched against saved entries by workspace first (name, then index) and the spawn list is capped at `saved − running` per app — re-running a restore resumes instead of duplicating; single-instance apps keep the skip rule (`plan_spawns`, 8 unit tests + end-to-end re-restore test)
- **Reactive saving**: the daemon subscribes to niri's event stream and saves after layout activity settles (2s debounce); when the stream is unavailable it falls back to the configured interval until niri accepts a subscription again (`reactive_save_session`, replaces the blind 15-min poll)
- **Focus restoration**: the saved focused window receives `Action::FocusWindow` after placement — `is_focused` was saved since v0.3.0 but never used
- **Per-app spawn serialization** (`SpawnLimiter`): same-app spawns never overlap, eliminating the same-app workspace-swap race on top of the global 5-spawn cap
- **Save throttling**: a capture byte-identical to the file on disk skips backup rotation and the write entirely
- **Output fallback for multi-monitor**: a saved output that no longer exists falls back to the output hosting the saved workspace (name → index)
- **Stale restore-marker pruning**: markers from previous boots are removed instead of accumulating forever
- **Run modes**: `--restore` (restore then exit), `--save-only` (skip boot restore), `--save-once` (one save then exit; powers the suspend hook)
- **`--health-check`**: reports niri reachability + version, boot-gate state, and session-file contents/age; fails loudly when niri is unreachable
- **`--export <DIR>` / `--import <DIR>`**: safe session portability — import validates before replacing and backs up the current session first
- **`--config-file <PATH>`**: app-config override (explicit path missing = error; default path missing = template created)
- **NixOS module**: `maxRestoreWindows` option (6 of 7 tunables mirrored; `dryRun` stays CLI-only) and `saveOnSuspend` (default true) installing a `sleep.target` oneshot that runs `--save-once`
- **Fake niri IPC test server** (`src/fake_niri.rs`): real Unix-socket protocol server with failure injection, spawn metering, and an event-stream mode; the IPC paths (restore, spawn ordering, placement, focus, retries, concurrency cap, health, shutdown final save, idempotency) are now integration-tested — 108 tests + 1 benchmark, up from 63
- **Property tests** (proptest): session round-trip identity, legacy-key alias identity, and arbitrary-input parse fuzzing for session and config formats
- `SESSION_FORMAT_VERSION = 4` (descriptive, not enforced — versions 1–3 still load via serde aliases); see `docs/example-session.json`

- **Graceful shutdown for the event-stream reader**: each event connection is an `EventConnection` owning the socket plus a `try_clone`d shutdown handle — on shutdown the drive loop shuts the socket down, unblocking the parked `spawn_blocking` reader, and the save loop stops via a watch signal (abort remains the deadline fallback under a 5s grace). SIGTERM on an idle desktop no longer risks hanging exit on a blocked reader
- **Capped exponential reconnect backoff**: stream-death reconnects start at 1s and double to a 30s cap; a stream that stayed healthy ≥5s resets the backoff, so one niri restart does not poison later reconnects
- **Harness coverage for the reactive loop's fallback branch**: the fake server can refuse event streams (`refuse_event_streams`) and meters per-app spawn concurrency; new tests cover polling-fallback saves with recovery onto the accepted stream, parked-reader shutdown, same-app spawn sequencing under cross-app overlap, window-closed re-restore, and an end-to-end `--save-only` run — 114 tests, up from 108

### Fixed

- **cargo-deny licenses check passes**: the crate now declares `license = "GPL-3.0-only"` (the missing manifest field failed the policy check), the Nix package declares `meta.license`, and the deny.toml allowlist admits MPL-2.0 (option-ext via dirs) plus GPL-3.0-only/-or-later (this crate and niri-ipc, which is GPL-3.0-or-later)
- Flake evaluation no longer warns: `getPlatform` reads `stdenv.hostPlatform` instead of the renamed top-level `hostPlatform` alias
- **Injectable polling-fallback interval** (`run_reactive_save_session`), replacing the hard-coded `config.save_interval` sleep so the fallback branch is testable below the 60s minimum
- **CI caches cargo builds** (Swatinem/rust-cache), cutting the per-run dependency rebuild
- `run_service_loop`: mode dispatch + service loop extracted from `main` so the harness can drive it end-to-end with an injected shutdown signal

### Changed

- The fallback loop's accepted subscription probe is now used directly for event-driven saves instead of being dropped in favor of one more reconnect
- The reactive save loop stops cooperatively on a shutdown signal before the deadline abort is considered
- `niri_send` now runs the blocking socket round-trip inside `spawn_blocking` instead of blocking the async executor
- Terminal CLI profiles (kitty, foot, wezterm, ghostty, alacritty) verified against official docs; `dedupe_single_instance_windows` tracks PIDs per app so two different single-instance apps sharing a PID no longer swallow each other's windows
- `terminal_state.max_walk_depth = 0` is rejected at startup (was a silent no-op)

### Fixed

- All 39 pre-existing clippy (pedantic+nursery) errors after a toolchain drift; clippy is green again and enforced in CI
- Broken `nix flake check`: the pinned treefmt-nix dropped the `programs.nixfmt-rfc-style` alias (plain `programs.nixfmt` is the RFC formatter now)
- Rust sources were no longer rustfmt-clean; `cargo fmt` applied and enforced
- Parent-directory fsync added to `atomic_write` — the rename now survives power loss, not just the file contents
- Corrupt session with no valid backup no longer writes a new session file during `--dry-run`
- `terminal_state` enabled with zero matched terminals now warns instead of silently saving nothing

## [0.4.1] - 2026-09-03

Patch release so flake consumers (SystemNix) can pin and receive the `0.4.0`
behavior fixes. No code changes on top of `0.4.0`.

### Added

- Living docs: `TODO_LIST.md`, `FEATURES.md`, `ROADMAP.md`, this `CHANGELOG.md`; README now documents `--max-restore-windows` and the boot-gate/dry-run behavior

### Changed

- Point-in-time status reports annotated inline and archived under `docs/status/archived/`

## [0.4.0] - 2026-09-02

### Added

- Boot-scoped restore gate: `boot_id` (`/proc/sys/kernel/random/boot_id`) + `restore-marker` file ensure restore runs at most once per boot; `--retry-attempts` retries within that single restore, not across restarts (`f082000`)
- `--max-restore-windows` sanity cap (default 100) on how many windows a single restore may spawn (`f082000`)

### Fixed

- Restore-storm class: save dedupe, stateless-terminal guard, boot-scoped gate (`f082000`)
- `--dry-run` wrote the boot restore-marker, silently disabling real restore for the entire boot after a preview run (`e4bf031`, `71e1c1b`)
- `--dry-run` saved `session.json` when no session file existed, violating the documented no-spawn/no-save contract (`e4bf031`)
- Shutdown race killed the final session save: the `select!` signal branch completed instantly and aborted the save task before its final write; shutdown now deterministically awaits the signal, joins the save task, and performs a 5s-timeout-guarded final save (`b97db66`)
- Flatpak-mapped terminals (e.g. `flatpak run org.wezfurlong.wezterm`) were profiled as Generic and restored with wrong CLI flags; profile detection now scans all mapping args and matches reverse-DNS app ids (`d215ab8`, `71e1c1b`)
- Corrupt `session.json` entered the backup rotation and evicted valid backups until recovery was impossible; `create_backup` now skips files that fail to parse (`e4bf031`)
- Guaranteed IPC error per window lacking workspace info (`WorkspaceReferenceArg::Index(0)`; niri is 1-based) — workspace move now skipped when neither name nor valid index exists (`71e1c1b`)
- Misleading log message "(will retry via periodic save)" — periodic save never re-restores (`e4bf031`)
- Duplicate `#[test]` attribute and missing `#[test]` on `shell_escape_empty`; project builds warning-free (`01b7640`)

## [0.3.0] - 2026-07-03

### Added

- `--dry-run` flag to preview restore without spawning (`2929a30`)
- NixOS module options for all then-existing CLI args: `saveInterval`, `maxBackupCount`, `spawnTimeout`, `retryAttempts`, `retryDelay` (`6126861`)
- Corrupted session recovery: falls back to most recent valid `.bak` when JSON parse fails (`7918d08`)
- Rate limiting: max 5 concurrent window spawns during restore via semaphore (`14eec3f`)

### Changed

- Atomic session writes (temp + fsync + rename) prevent corruption on crash (`9f6d8d6`)
- Stable Rust compilation (edition 2021, all `let` chains refactored) — a nightly-only release previously broke NixOS stable builds (`9f6d8d6`)
- Startup ordering: restore completes before periodic save starts (`9f6d8d6`)
- Non-fatal restore: niri IPC not ready logs an error instead of crash-looping under `Restart=always` (`a8e8dd0`)
- `AppConfig` cached at startup — eliminated 96 TOML re-reads/day (`3642b2a`)
- Structured logging via `tracing` with `RUST_LOG` support (`70d7403`)
- `WorkspaceInfo` extracted as a cohesive type; serialized keys `workspace_idx`/`workspace_name`/`workspace_output` → `idx`/`name`/`output` with `#[serde(alias)]` backward compat (`055d79d`)
- Systemd module hardening: `requires niri.service`, `RestartSec`, `StartLimitBurst`, `OOMScoreAdjust` (`9f6d8d6`)
- Complete README rewrite (`cd57e47`)
- CLI arg validation at startup with clear error messages (`3e6ef09`)

### Fixed

- `retry_attempts=0` silently did nothing; `save_interval=0` caused a tight spin loop — both now rejected at startup (`9f6d8d6`)
- 20+ windows spawned simultaneously overwhelmed niri IPC (`14eec3f`)
- Periodic save running concurrently with restore could snapshot partial state (`9f6d8d6`)

## [0.2.0] - 2026-07-03

### Added

- Terminal state recovery via `/proc` PID resolution — restores running commands inside terminals (`2d47dc3`)
- Session save/restore core, backup rotation, Nix flake + NixOS module, TOML config

---

*Version history reconstructed 2026-09-03 from git history and archived status reports; earlier history was committed via an unlabeled auto-commit daemon, so per-change hashes start at `9f6d8d6`.*
