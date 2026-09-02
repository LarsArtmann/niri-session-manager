# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
