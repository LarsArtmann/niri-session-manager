# Status Report — niri-session-manager v0.3.0

**Date:** 2026-07-03 10:38
**Branch:** main
**Base:** `2d47dc3` (upstream `feat: terminal state recovery via /proc PID resolution`)
**Head:** `a81cb9d` (fork `chore: bump version to 0.3.0`)
**Commits this session:** 17

---

## A) FULLY DONE

### Reliability Hardening
| # | Commit | Description |
|---|--------|-------------|
| 1 | `9f6d8d6` | Atomic session writes (temp + fsync + rename) — prevents corruption on crash |
| 2 | `9f6d8d6` | Stable Rust compilation (edition 2021, refactored all `let` chains) |
| 3 | `9f6d8d6` | Startup ordering: restore completes before periodic save starts |
| 4 | `9f6d8d6` | Fix `retry_attempts=0` edge case (ensure at least one attempt) |
| 5 | `9f6d8d6` | Guard `save_interval=0` (prevent tight spin loop) |
| 6 | `9f6d8d6` | Systemd module hardening: `requires niri.service`, `RestartSec`, `StartLimitBurst`, `OOMScoreAdjust` |
| 7 | `a8e8dd0` | Non-fatal restore: don't exit process if niri IPC isn't ready (prevents crash loop) |
| 8 | `7918d08` | Corrupted session recovery: try most recent valid `.bak` if JSON parse fails |

### Architecture Improvements
| # | Commit | Description |
|---|--------|-------------|
| 9 | `3642b2a` | Cache `AppConfig` at startup — eliminated 96 TOML re-reads per day |
| 10 | `6ef61d8` | Remove dead `workspace_id` field from `SavedWindow` |
| 11 | `70d7403` | Switch to `tracing` for structured logging (levels, timestamps, `RUST_LOG`) |
| 12 | `e32950c` | Complete tracing migration: replace last 3 `eprintln!` calls |
| 13 | `830fb0c` | Replace unreachable `Ok(())` with `unreachable!` macro |
| 14 | `055d79d` | Extract `WorkspaceInfo` cohesive type (serde flatten + alias backward compat) |
| 15 | `14eec3f` | Rate limiting: max 5 concurrent window spawns during restore (semaphore) |
| 16 | `3e6ef09` | CLI arg validation at startup with clear error messages |

### Features
| # | Commit | Description |
|---|--------|-------------|
| 17 | `2929a30` | `--dry-run` flag to preview restore without spawning |
| 18 | `6126861` | NixOS module options for all CLI args (`saveInterval`, `maxBackupCount`, etc.) |

### Testing
| # | Commit | Description |
|---|--------|-------------|
| 19 | `7918d08` | +3 tests: backup recovery (most-recent, corrupt-skip, empty-dir) |
| 20 | `f6233c1` | +4 tests: `atomic_write` (create, overwrite, no-temp-left, parent-dirs) |
| 21 | `1a7467c` | +4 tests: TOML config parsing (full, empty, partial, terminal_state defaults) |
| — | — | **Total: 57 tests, 0 failures** |

### Documentation & Release
| # | Commit | Description |
|---|--------|-------------|
| 22 | `cd57e47` | Complete README rewrite with all new features |
| 23 | `a81cb9d` | Version bump 0.2.0 → 0.3.0 |

### SystemNix Integration
| # | Commit | Description |
|---|--------|-------------|
| 24 | SystemNix `01af3406` | Flake input pointed to `github:LarsArtmann/niri-session-manager` |
| 25 | SystemNix `01af3406` | Config.toml: added ghostty mapping + terminal_state section |

---

## B) PARTIALLY DONE

### Session Format v4 Migration
- `WorkspaceInfo` extracted with serde flatten + alias — **but the serialized JSON keys changed** from `workspace_idx`/`workspace_name`/`workspace_output` to shorter `idx`/`name`/`output`
- Deserialization handles both old and new keys via `#[serde(alias = "...")]`
- No migration step needed — old files auto-migrate on next save
- **Gap:** `SESSION_FORMAT_VERSION` is still 3, should bump to 4 to reflect the new key names in serialized output

### Test Coverage
- 57 tests total (up from baseline 46)
- **Well covered:** atomic_write, config parsing, serialization round-trips, proc PID resolution, terminal profiles, shell escaping, backup recovery
- **Not covered:** IPC integration (requires live niri), rate limiting semaphore, dry-run output, restore_session retry loop

---

## C) NOT STARTED

| # | Item | Impact |
|---|------|--------|
| 1 | niri event stream subscription (reactive saves instead of polling) | High — would enable instant saves on layout changes |
| 2 | Focus restoration after workspace placement | Medium — focused window not restored |
| 3 | Window size/column-width capture and restoration | Medium — blocked by niri IPC limitations |
| 4 | Config hot-reload via file watching (inotify) | Low — restart picks up config changes |
| 5 | `--config-file` CLI override for config path | Low — XDG is sufficient |
| 6 | Integration test with mock IPC socket | Medium — would catch IPC protocol regressions |
| 7 | thiserror for structured error types | Low — anyhow is sufficient for this scope |
| 8 | Cargo-deny for supply chain auditing | Low — nice-to-have |
| 9 | `CONTRIBUTING.md` | Low |
| 10 | `CHANGELOG.md` | Low — git log is clean and descriptive |
| 11 | CI badge in README | Low |
| 12 | Multi-monitor edge case tests | Medium — output names can change between sessions |

---

## D) TOTALLY FUCKED UP

**Nothing is currently broken or incorrect.** All items below were found and fixed during this session:

| Issue | Status | Fix Commit |
|-------|--------|------------|
| 3 `eprintln!` calls missed during tracing migration | **Fixed** | `e32950c` |
| Unreachable `Ok(())` dead code in retry loop | **Fixed** | `830fb0c` |
| `retry_attempts=0` silently did nothing | **Fixed** | `9f6d8d6` |
| `save_interval=0` caused tight spin loop | **Fixed** | `9f6d8d6` |
| Restore failure crashed the process → crash loop with `Restart=always` | **Fixed** | `a8e8dd0` |
| Non-atomic session writes → corruption risk | **Fixed** | `9f6d8d6` |
| `let` chains required nightly Rust → NixOS stable couldn't build | **Fixed** | `9f6d8d6` |
| Periodic save ran concurrently with restore → partial state snapshots | **Fixed** | `9f6d8d6` |
| Config re-read from disk 96x/day | **Fixed** | `3642b2a` |
| 20+ windows spawned simultaneously → niri IPC overwhelmed | **Fixed** | `14eec3f` |

---

## E) WHAT WE SHOULD IMPROVE

### Architecture
1. **Session format version bump to 4** — serialized JSON now uses `idx`/`name`/`output` instead of `workspace_idx`/`workspace_name`/`workspace_output`. The version should reflect this.
2. **Reactive saves via niri event stream** — instead of polling every 15 minutes, subscribe to niri's IPC event stream and save on layout changes. Would make session saves near-instant.
3. **Error type strategy** — currently using `anyhow` everywhere. For a library boundary (if this ever becomes one), structured errors via `thiserror` would be better. For now anyhow is fine.

### Operational
4. **Focus restoration** — `SavedWindow` has `is_focused` but it's never used during restore. After all windows are placed, the focused window should be brought to front.
5. **Multi-monitor robustness** — workspace output names can change between sessions (e.g., monitor plugged into different port). Should match outputs by EDID or position, not name.
6. **Health check** — no way to verify the service is healthy beyond "process is running". A `/health` IPC endpoint or `--health-check` CLI subcommand would help.

### Testing
7. **Integration tests with mock IPC** — all IPC code paths are untested. A test harness with a fake niri socket would catch protocol regressions.
8. **Property-based testing** for serialization — quickcheck or proptest would catch edge cases in the SessionData/SavedWindow round-trip.
9. **Snapshot tests** for dry-run output — ensure the preview format doesn't accidentally change.

---

## F) TOP 25 THINGS TO DO NEXT (sorted by impact/effort)

| # | Task | Impact | Effort |
|---|------|--------|--------|
| 1 | Bump `SESSION_FORMAT_VERSION` to 4 | Medium | 5 min |
| 2 | Focus restoration after workspace placement | High | 30 min |
| 3 | niri event stream subscription for reactive saves | High | 2-4 hr |
| 4 | Integration test harness with mock niri IPC socket | High | 3-5 hr |
| 5 | Multi-monitor output matching (by position, not name) | Medium | 1-2 hr |
| 6 | Snapshot test for dry-run output | Low | 20 min |
| 7 | Property-based testing for serialization round-trips | Medium | 1 hr |
| 8 | Config hot-reload via inotify | Low | 1-2 hr |
| 9 | `--config-file` CLI override | Low | 15 min |
| 10 | `CHANGELOG.md` | Low | 20 min |
| 11 | `CONTRIBUTING.md` | Low | 15 min |
| 12 | Cargo-deny for supply chain | Low | 15 min |
| 13 | CI badge in README | Low | 5 min |
| 14 | `--health-check` subcommand | Medium | 30 min |
| 15 | Window size/column-width capture (when niri IPC supports it) | High | Blocked upstream |
| 16 | Spawn timeout exponential backoff | Low | 20 min |
| 17 | Deduplication of identical windows in session file | Low | 30 min |
| 18 | thiserror for public error types | Low | 1 hr |
| 19 | SSH suspend guard integration | Low | 30 min |
| 20 | DMS (DankMaterialShell) integration for session state display | Low | 1 hr |
| 21 | Per-app restore delay config | Low | 20 min |
| 22 | Session file migration command (`--migrate`) | Low | 30 min |
| 23 | systemd notify readiness signaling | Low | 15 min |
| 24 | Coverage reporting in CI (`tarpaulin`) | Low | 30 min |
| 25 | Crate publication to crates.io | Low | 15 min |

---

## G) TOP QUESTION

**Should `SESSION_FORMAT_VERSION` be bumped to 4?**

The serialized JSON keys changed from `workspace_idx`/`workspace_name`/`workspace_output` to `idx`/`name`/`output` due to the `WorkspaceInfo` extraction with `#[serde(flatten)]`. Old format files still deserialize correctly via `#[serde(alias = "...")]`, but newly written files use the shorter keys. The version number in the file is still 3.

Options:
- **A) Bump to 4** — accurate, signals the format change, but technically no migration is needed since aliases handle backward compat
- **B) Keep at 3** — safe, but misleading since the serialized representation changed

I lean towards **A (bump to 4)** for honesty about the format change, but would defer to the user's preference.
