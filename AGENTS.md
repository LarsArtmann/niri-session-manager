# AGENTS.md

Context for AI sessions working on this repository.

## What This Is

`niri-session-manager`: a Rust daemon that reactively saves the Niri Wayland compositor's window layout to JSON (event-stream driven with an interval fallback) and restores it idempotently on startup. Deployed via Nix flake + NixOS module as a systemd user service (`Restart=always`), with a suspend hook that saves once before sleep. Consumed externally by the SystemNix config repo — API changes (CLI flags, NixOS module options) are breaking changes for that consumer.

## Commands

```bash
cargo build                      # build
cargo test                       # full suite: unit + fake-IPC integration tests (108 + 1 ignored benchmark)
cargo clippy --all-features      # lint (CI runs this exact form; pedantic+nursery denies enforced)
cargo fmt --all -- --check       # format check (CI enforces)
nix build                        # build Nix package
nix flake check                  # flake + module + treefmt (nixfmt + statix)
nix fmt                          # fix Nix formatting
bash scripts/docs-citations.sh   # verify file:line citations + relative links in living docs (CI)
cargo test restore_burst --release -- --ignored --nocapture   # benchmark (see docs/benchmarks/)
```

- Devshell: `nix develop` (`.envrc` has `use flake`, so direnv handles it).
- CI (`.github/workflows/checks.yml`) additionally runs `deadnix`, `statix check`, `cargo-deny` (advisories/licenses/bans, config in `deny.toml`), a `--version` smoke step, and the docs-citations linter. CI builds with nightly Rust, but the code must compile on **stable Rust** (edition 2021) — a past release broke NixOS stable builds over `let` chains. No nightly features.
- Never use a Makefile/justfile; flake.nix is the task runner.

## Session rules learned the hard way

- **Re-verify toolchain baselines at session start** (build + clippy + fmt + `nix flake check`). A previous session's "all green" was wrong on three counts (clippy 39 errors, fmt dirty, flake check broken by a treefmt-nix rename).
- **Verify-after-write, always.** After scripted/bulk edits, grep-assert the expected marker in the same command. Edits have silently vanished here (parallel writes racing the auto-commit daemon).
- **Never mutate one file from two tools in flight.** Serialize file writes; re-read after any "file modified" rejection.
- **Never pipe test output through grep in background shells** — write to a file and tail it, or you debug blind.
- Stale rust-analyzer diagnostics (e.g. the duplicate-attribute/`shell_escape_empty` warnings in `src/main.rs`) are cache lies; trust `cargo build`, not the LSP cache.

## Architecture

Three source files (the first two are the real code, the third is test infrastructure):

- `src/main.rs` (~3400 lines): niri IPC, session model, idempotent restore planning, reactive save loop, config, backups, export/import, health check, CLI, unit tests.
- `src/proc.rs` (~435 lines): `/proc` process-tree walking for terminal state recovery. Linux-only code is gated with `#[cfg(target_os = "linux")]` with a portable no-op fallback for `resolve_child_process`. Everything takes an injectable `base: &Path` so tests can mount fake proc trees.
- `src/fake_niri.rs` (~820 lines, `#[cfg(test)]` only): an in-process fake niri IPC server (real Unix socket, real protocol) — Windows/Workspaces/Version replies, Spawn/Move/Focus recording with failure injection and concurrency metering, plus an EventStream mode with queued events. Tests take the process-global `IPC_ENV_LOCK` via `FakeNiri::env()` because `Socket::connect()` reads `$NIRI_SOCKET` from the environment; `FakeNiri::close()` must be called before a test ends if it spawned the reactive save loop (tokio's runtime drop waits for `spawn_blocking` readers).

### Runtime flow

1. `main`: parse/validate CLI → load `AppConfig` (TOML, cached — never re-read) → dispatch run mode: `--health-check` / `--export` / `--import` / `--save-once` exit after their one job; otherwise boot-gated restore (`run_boot_restore`) → `--dry-run`/`--restore` exit here → spawn `reactive_save_session` + await shutdown signal → `shutdown_with_final_save` (aborts the save task, one final save under a 5s timeout).
2. **Save (reactive)**: subscribe to niri's event stream → layout-relevant events trigger a **debounced** (2s) save; if the stream is unavailable or dies, fall back to the configured interval until niri accepts a subscription again. A capture byte-identical to the file on disk skips backup rotation and the write entirely.
3. **Restore (idempotent)**: read session → on parse failure fall back to most recent valid `.bak` → prepare (sort, drop stateless terminals, warn on suspicious per-app counts, cap at `--max-restore-windows`) → snapshot running windows → `plan_spawns` **matches by workspace first (name, then index), then caps at `saved − running` per app** → spawn through `SpawnLimiter` (global cap 5, **per-app serialization**) → place (output fallback via workspace host, workspace move, focus restore for the saved focused window).
4. **Boot gate**: `should_restore_on_boot` compares the marker to `boot_id` and **prunes stale markers from previous boots**; the marker is written only after a successful non-dry-run restore.

### Invariants that past bugs paid for (do not regress)

- **All session-file writes go through `atomic_write`.** Temp file + fsync of contents + rename + **fsync of the parent directory** — the last one is what makes the rename survive power loss.
- **Restore completes before the save loop starts.** Concurrent save during restore snapshots partial state.
- **Restore failure is non-fatal.** Errors are logged, never returned from `main` — under `Restart=always`, a failing restore crash-loops the service.
- **`--dry-run` never spawns and never writes** — no session file, no marker (regression-tested twice over).
- **Restore is idempotent**: re-running spawns only the per-app deficit; single-instance apps are skipped when any instance runs. The harness test `re_restore_spawns_only_the_missing_windows` guards this.
- **Same-app spawns are serialized** (`SpawnLimiter`) — two instances of one app cannot claim each other's windows and swap workspaces.
- **Boot-scoped restore gate**: one restore per boot; `--retry-attempts` retries _within_ that restore.
- **Zero-valued CLI args are rejected at startup** (`save_interval=0` once caused a tight spin loop); `terminal_state.max_walk_depth = 0` is rejected too.
- **Unchanged saves are skipped** (byte-identical capture → no backup rotation, no write).

### Session format compatibility

- `SessionData` is a `#[serde(untagged)]` enum: `Versioned` (current, `SESSION_FORMAT_VERSION = 4`) or legacy plain array. The version is **descriptive, not enforced** — files from versions 1–3 load via `#[serde(alias)]` and migrate on next save. Q4 in ROADMAP is resolved; see `docs/example-session.json`.
- When changing serialized keys, always add aliases so old `session.json` files still load, and extend the property tests (round-trip + legacy-alias identity).

### Terminal state recovery

`proc.rs` walks the terminal's process tree to the foreground child (skips shells and helpers like `kitten`, prefers the child matching `tpgid`), capturing cmdline + cwd. On restore, `build_terminal_restore_command` composes a terminal-specific command. The five profiles (kitty, foot, wezterm, ghostty, alacritty) were **verified against the official CLI docs on 2026-09-04** (kitty/foot take positional commands with no `-e`; foot's `-e` is xterm-compat and ignored; wezterm needs `start --cwd … --`; ghostty uses `--working-directory=…` + `-e`; alacritty `--working-directory` + `-e`). Adding a terminal means: a new profile in `TerminalProfile` + app_id in `default_terminal_app_ids` + doc verification + tests.

## Config surface

- CLI (`Config`, clap derive in `src/main.rs`): tunables `--save-interval/--max-backup-count/--spawn-timeout/--retry-attempts/--retry-delay/--max-restore-windows`; behaviors `--dry-run`, `--restore`, `--save-only`, `--save-once` (suspend hook), `--health-check`, `--export <DIR>`, `--import <DIR>`, `--config-file <PATH>` (explicit path missing = error; default path missing = template written). Validation in `Config::validate`.
- TOML: `AppConfig` at `$XDG_CONFIG_HOME/niri-session-manager/config.toml`. Invalid TOML warns and falls back to defaults; a missing default file is created from the embedded template (`DEFAULT_APP_CONFIG_TOML`).
- NixOS module (`module.nix`): mirrors **6 of 7** tunables (`maxRestoreWindows` included; `dryRun` is CLI-only by design) + `saveOnSuspend` (default true) which installs a `sleep.target`-ordered oneshot running `--save-once`. When adding a CLI flag, update `module.nix` and the README together.

## Testing

- 108 tests + 1 `#[ignore]`d benchmark. Unit tests live in-file; IPC integration tests live in `src/fake_niri.rs` against the fake server.
- Never test against a live niri session; the fake server covers restore, save, shutdown, idempotency, focus, retries, concurrency, health, and the event stream (see `layout_event_triggers_debounced_save`).
- Tests that touch `$NIRI_SOCKET` MUST hold `IPC_ENV_LOCK` (`FakeNiri::env()` / `FakeNiri::env_without_socket()`); parallel tests race on the environment otherwise.
- The polling-fallback branch of `reactive_save_session` is untested (real 60s+ wait); known gap, listed in TODO_LIST.

## Docs map

- `README.md` (user-facing), `FEATURES.md` (honest feature status), `TODO_LIST.md` (open bounded work), `ROADMAP.md` (vision + resolved/open questions), `CHANGELOG.md` (per-version changes), `CONTRIBUTING.md` (process + rules). Keep all in sync with code; done TODO items move to CHANGELOG.
- `docs/planning/` holds the executed Pareto plan; `docs/status/*.md` are annotated point-in-time reports (archived under `docs/status/archived/` once fully resolved) — historical evidence, not current truth.
- `docs/example-session.json` shows the current format; `docs/benchmarks/restore-burst.md` records benchmark methodology + numbers.

## Known Issues (open, pre-existing or accepted)

- The polling-fallback branch of the reactive save loop is untested (interval-bound).
- `drive_event_driven_saves`: aborting the save task (shutdown) leaves the event reader blocked until the connection closes — harmless because the process exits, but a graceful socket-timeout join would be cleaner.
- Event-stream reconnect uses a fixed 1s delay, not exponential backoff.
- Terminal flag profiles are doc-verified but not exercised against real terminal binaries; daily-driver terminals deserve must-not-regress status (ROADMAP Q3 open).
