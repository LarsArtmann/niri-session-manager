# AGENTS.md

Context for AI sessions working on this repository.

## What This Is

`niri-session-manager`: a Rust daemon that periodically saves the Niri Wayland compositor's window layout to JSON and restores it on startup. Deployed via Nix flake + NixOS module as a systemd user service (`Restart=always`). Consumed externally by the SystemNix config repo — API changes (CLI flags, NixOS module options) are breaking changes for that consumer.

## Commands

```bash
cargo build                      # build
cargo test                       # unit tests (63 as of this writing; README intentionally says "test suite")
cargo clippy --all-features      # lint (CI runs this exact form)
cargo fmt --all -- --check       # format check (CI enforces)
nix build                        # build Nix package
nix flake check                  # includes treefmt check (nixfmt-rfc-style + statix) on all .nix files
nix fmt                          # fix Nix formatting
```

- Devshell: `nix develop` (`.envrc` has `use flake`, so direnv handles it).
- CI (`.github/workflows/checks.yml`) additionally runs `deadnix` on tracked `.nix` files and `statix check`. CI builds with nightly Rust, but the code must compile on **stable Rust** (edition 2021) — a past release broke NixOS stable builds over `let` chains. No nightly features.
- Never use a Makefile/justfile; flake.nix is the task runner.

## Architecture

Deliberately two files — do not split further without strong reason:

- `src/main.rs` (~1900 lines): everything else — niri IPC, session model, save/restore, config, backups, CLI, tests.
- `src/proc.rs` (~440 lines): `/proc` process-tree walking for terminal state recovery. Linux-only code is gated with `#[cfg(target_os = "linux")]` with a portable no-op fallback for `resolve_child_process`. Everything in this file takes an injectable `base: &Path` so tests can mount fake proc trees.

### Runtime flow

1. `main`: parse/validate CLI → load `AppConfig` (TOML, cached — never re-read) → boot-gated restore → `--dry-run` exits here → spawn periodic save + signal handler.
2. **Save**: niri IPC `Windows`+`Workspaces` → `SavedWindow` list (+ `/proc` terminal state per terminal window) → `atomic_write` (temp + fsync + rename) to `session.json` → backup rotation.
3. **Restore**: read session → on parse failure fall back to most recent valid `.bak` → filter (skip apps, terminals without captured state) → cap at `--max-restore-windows` → spawn via niri IPC through a `Semaphore` (max 5 concurrent).

### Invariants that past bugs paid for (do not regress)

- **All session-file writes go through `atomic_write`.** Plain `fs::write` corrupted sessions on crash.
- **Restore completes before periodic save starts.** Concurrent save during restore snapshots partial state.
- **Restore failure is non-fatal.** Errors are logged, never returned from `main` — under `Restart=always`, a failing restore crash-loops the service.
- **Boot-scoped restore gate**: `boot_id` (`/proc/sys/kernel/random/boot_id`) + `restore-marker` file beside `session.json` ensures one restore per boot. `--retry-attempts` retries _within_ that single restore, not across restarts.
- **Zero-valued CLI args are rejected at startup** (`save_interval=0` once caused a tight spin loop; `retry_attempts=0` silently did nothing).

### Session format compatibility

- `SessionData` is a `#[serde(untagged)]` enum: `Versioned` (current, `SESSION_FORMAT_VERSION = 3`) or legacy plain array. Old files parse transparently and migrate on next save — keep it that way.
- `WorkspaceInfo` was renamed from `workspace_idx`/`workspace_name`/`workspace_output` to `idx`/`name`/`output`; old keys are kept readable via `#[serde(alias)]`. When changing serialized keys, always add aliases so old `session.json` files still load.
- Known gap (documented, unfixed): version should be bumped to 4 to reflect the key rename.

### Terminal state recovery

`proc.rs` walks the terminal's process tree to the foreground child (skips shells and helpers like `kitten`, prefers the child matching `tpgid`), capturing cmdline + cwd. On restore, `build_terminal_restore_command` composes a terminal-specific command — each terminal emulator has its own profile (kitty, foot, wezterm, ghostty, alacritty) with different working-directory and `-e` flags. Adding a terminal means: a new profile in `build_terminal_restore_command` + app_id added to `default_terminal_app_ids` + a test. Shell detection relies on `comm` names including login-shell forms like `-fish`/`-bash`.

## Config surface

- CLI: defined in `Config` (clap derive) in `src/main.rs`; validated manually in `main` (clap can't express "must be ≥ 1" per-arg with defaults).
- TOML: `AppConfig` at `$XDG_CONFIG_HOME/niri-session-manager/config.toml` (app_mappings, single_instance_apps, skip_apps, terminal_state). Invalid TOML logs a warning and falls back to defaults — it does not fail startup.
- NixOS module (`module.nix`): mirrors 5 of the 7 CLI flags as camelCase options (`maxRestoreWindows` is missing — see TODO_LIST; `dryRun` is CLI-only by design), wired into a systemd user service ordered after `niri.service` + `graphical-session.target`. When adding a CLI flag, update `module.nix` and the README option tables together.

## Testing

- Unit tests only, in `#[cfg(test)] mod tests` at the bottom of each file. No test harness config beyond `dev-dependencies` (`tempfile`).
- `proc.rs` tests build fake `/proc` trees in tempdirs (NUL-separated `cmdline`, `comm`, `task/<pid>/children`, `cwd` symlink) and call the `*_at(base, ...)` variants. Follow this pattern for new `/proc` behavior; do not test against the real `/proc`.
- Everything touching live niri IPC (`restore_session_internal` spawning, semaphore rate limiting, retry loop) is untested — don't assume IPC paths have coverage.
- Serialization round-trip tests exist for the session format; extend them when touching `SavedWindow`/`SessionData`.

## Docs map

- `README.md` (user-facing), `FEATURES.md` (honest feature status), `TODO_LIST.md` (open bounded work), `ROADMAP.md` (vision + pending maintainer questions), `CHANGELOG.md` (per-version changes). Keep all five in sync with code; done TODO items move to CHANGELOG.
- `docs/status/*.md` are annotated point-in-time reports, archived under `docs/status/archived/` once every item is resolved inline — historical evidence, not current truth.

## Known Issues (pre-existing, not yours to fix unprompted)

- `docs/status/archived/*.md` are point-in-time reports — re-verify claims against code before acting on them.
- Everything touching live niri IPC is untested (see Testing); don't assume IPC paths have coverage.
- `SavedWindow.is_focused` is saved but never used during restore (focus restoration not implemented).
- Restore is not idempotent across failed restores: if a restore partially spawns windows and then fails, the retry (or next service start without a marker) re-spawns windows for non-single-instance apps. Boot marker is only written on full success.
