# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
