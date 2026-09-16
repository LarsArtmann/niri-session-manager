# AGENTS.md

Context for AI sessions working on this repository.

## What This Is

`niri-session-manager`: a Rust daemon that reactively saves the Niri Wayland compositor's window layout to JSON (event-stream driven with an interval fallback) and restores it idempotently on startup. Deployed via Nix flake + NixOS module as a systemd user service (`Restart=always`), with a suspend hook that saves once before sleep. Consumed externally by the SystemNix config repo — API changes (CLI flags, NixOS module options) are breaking changes for that consumer.

## Commands

```bash
cargo build                      # build
cargo test                       # full suite: unit + fake-IPC integration tests (139 + 1 ignored benchmark)
cargo clippy --all-features      # lint (CI runs this exact form; pedantic+nursery denies enforced)
cargo fmt --all -- --check       # format check (CI enforces)
nix build                        # build Nix package
nix flake check                  # flake + module + treefmt (nixfmt + statix)
nix fmt                          # fix Nix formatting
bash scripts/docs-citations.sh   # verify file:line citations + relative links in living docs (CI)
nix develop -c markdownlint-cli . # markdown lint (CI step; .markdownlint.json + .markdownlintignore)
bash scripts/soak-test.sh [a|b|c|all]   # REAL-hardware soak vs the live compositor (disrupts the desktop ~4-6 min; see the script header)
CARRIER=foot|wezterm|alacritty bash scripts/soak-test.sh c   # phase C with a different carrier terminal (kitty is the default)
cargo test restore_burst --release -- --ignored --nocapture   # benchmark (see docs/benchmarks/)
```

- Devshell: `nix develop` (`.envrc` has `use flake`, so direnv handles it).
- CI (`.github/workflows/checks.yml`) additionally runs `deadnix`, `statix check`, `cargo-deny` (advisories/licenses/bans, config in `deny.toml`), a markdownlint step, a `--version` smoke step, and the docs-citations linter. A separate canary workflow (`.github/workflows/drift-guard.yml`, weekly + dispatch, `continue-on-error`) repins niri-ipc to the newest release and runs the drift-sensitive tests. CI builds with nightly Rust, but the code must compile on **stable Rust** (edition 2021) — a past release broke NixOS stable builds over `let` chains. No nightly features.
- **This repo is a FORK of `MTeaHead/niri-session-manager`; its Actions were first enabled and executed 2026-09-15**: the first real run (workflow_dispatch `34990898129`) passed end-to-end in 5m27s at `4256880`. Fork workflows had stayed disabled until the owner clicked "enable" (no API consent exists — verified 2026-09-15: workflow `active`, triggers correct, 0 runs ever, while sibling repos run fine). `workflow_dispatch` + `v*` tag triggers (added 2026-09-15) fire runs on demand and on release tags, and `push` triggers now fire normally with Actions enabled.
- Supply-chain policy: the crate declares `license = "GPL-3.0-only"` (Cargo.toml) and deny.toml's allowlist admits MPL-2.0 + GPL-3.0-only/-or-later because niri-ipc is GPL-3.0-or-later — a new dependency with an unlisted license fails `cargo deny check licenses` (add it to `deny.toml`; cargo-deny/cargo-audit are in the devshell, not on bare PATH).
- Lint configs the repo honours: `.markdownlint.json` (MD013/24/26/29 off by design — wide tables + archived status docs; MD040 stays on: fenced code declares a language), `.lycheeignore` + `lychee.toml` (fsf.org is canonical GPL text on a TLS-broken server; gnu.org rate-limits checkers with 429, so 429 is accepted and gnu.org is throttled).
- Never use a Makefile/justfile; flake.nix is the task runner.

## Session rules learned the hard way

- **Re-verify toolchain baselines at session start** (build + clippy + fmt + `nix flake check`). A previous session's "all green" was wrong on three counts (clippy 39 errors, fmt dirty, flake check broken by a treefmt-nix rename).
- **Verify-after-write, always.** After scripted/bulk edits, grep-assert the expected marker in the same command. Edits have silently vanished here (parallel writes racing the auto-commit daemon).
- **Never mutate one file from two tools in flight.** Serialize file writes; re-read after any "file modified" rejection.
- **Never pipe test output through grep in background shells** — write to a file and tail it, or you debug blind.
- Stale rust-analyzer diagnostics (e.g. the duplicate-attribute/`shell_escape_empty` warnings in the Rust sources) are cache lies; trust `cargo build`, not the LSP cache.
- **Probe the protocol before soaking the product, and baseline production first** (2026-09-16 lessons): a 2-minute raw-socket probe (capture the burst, offline-deserialize against the pinned crate) plus one `ls` on the deployed session file's mtime would have found the F1 outage before any live-desktop disruption. `scripts/soak-test.sh` encodes the full procedure; verify every spawned test process exits (a wedged probe manager once flapped against the compositor for 7.5h).
- **Wait for spawned carriers to APPEAR, never sleep a fixed delay** (2026-09-16 lesson): alacritty's cold first launch outlived the soak script's fixed 3s pre-capture sleep, so the capture missed the carrier and the whole phase-C proof ran against nothing (2 red assertions). The script now waits app-specifically for the carrier window to exist before capturing — apply the same pattern to any future live-desktop phase.
- **`nix build` only sees git-TRACKED files** (2026-09-16 lesson): the flake source is the git tree, so a new untracked file (e.g. `src/testdata/*.jsonl` required by `include_str!`) silently breaks the Nix build while `cargo build` stays green — stage new source files before evaluating nix. Also: `cmd | tail` pipelines mask cargo/nix failures (check `$?` immediately or avoid pipes on gates).

## Architecture

Behavior-frozen module split (2026-09-15; formerly one ~3.7k-line `main.rs`):

- `src/main.rs`: module wiring, shutdown-signal handling, `run_health_check` (warns when the session file is staler than 2 × the save interval — `session_staleness_warning`), `run_protocol_probe` (`--protocol-probe`: version round-trip + event-stream burst-head drift report), `run_service_loop` (mode dispatch), `main` (exits the process explicitly after the service loop — the runtime drop would otherwise wait on any parked blocking IPC thread).
- `src/ipc.rs`: raw-socket niri IPC — `open_niri_socket`/`request_reply` (every request/reply carries the 5s `IPC_REQUEST_TIMEOUT` read/write timeout; a wedged niri surfaces as an error, not a hang) and `niri_send` (all blocking I/O on `spawn_blocking`), `get_niri_windows`/`get_niri_workspaces`.
- `src/session.rs` (~570 lines): session model (`SavedWindow`, `WorkspaceInfo`, `SavedWindowLayout`, `SessionData`), capture from niri, atomic writes, backups, export/import.
- `src/terminal.rs` (~200 lines): restore-command composition, the five terminal profiles, `/proc` terminal-state resolution (bridge to `proc.rs`).
- `src/config.rs` (~280 lines): CLI `Config` (clap) + TOML `AppConfig` + validation + embedded config template.
- `src/restore.rs` (~700 lines): boot gate, idempotent planning (`plan_spawns`), `SpawnLimiter`, spawning, placement, final focus pass, retry backoff, `run_boot_restore`.
- `src/save.rs`: reactive save loop (tolerant event-stream reader + debounce + polling fallback + reconnect backoff + rapid-death fallback + rate-limited flapping summary — one WARN per 10 stream deaths, `flapping_summary`), `EventConnection` (boxed reader), graceful shutdown with final save.
- `src/proc.rs`: `/proc` process-tree walking for terminal state recovery. Linux-only code is gated with `#[cfg(target_os = "linux")]` with a portable no-op fallback for `resolve_child_process`. Everything takes an injectable `base: &Path` so tests can mount fake proc trees. Child discovery: `/proc/<pid>/task/<pid>/children` first, **stat-ppid scan fallback** (the children file reports zero children for NixOS wrapper fork shapes like `.ghostty-wrapper`); candidate order: tpgid match, then non-shell/non-helper children (kitty spawns leaf `kitten __atexit__`/`__watch_conf__` helpers beside the real child), then first child.
- `src/fake_niri.rs` (`#[cfg(test)]` only): an in-process fake niri IPC server (real Unix socket, real protocol) — Windows/Workspaces/Version replies, Spawn/Move/Focus recording with failure injection and concurrency metering (global AND per-app in-flight tracking), event-stream refusal injection (`refuse_event_streams`), **state-sync burst emulation** (`emit_state_sync_burst` — mirrors real niri's up-front full-state push including the `CastsChanged` line and an unknown-variant poison line), stream-death injection (`close_next_event_stream_after`), instant-kill streams (`kill_next_event_streams`), plus an EventStream mode with queued events. Tests take the process-global `IPC_ENV_LOCK` via `FakeNiri::env()`/`SocketEnv::at(path)`; `FakeNiri::close()` must be called before a test ends if it spawned the reactive save loop (tokio's runtime drop waits for `spawn_blocking` readers).
- `src/tests.rs` (`#[cfg(test)]` only): the unit-test module (moved out of `main.rs` in the split); imports the code under test via glob per module; includes the live-capture fixture test over `src/testdata/niri-event-stream-2026-08-02.jsonl`.

### Runtime flow

1. `main`: parse/validate CLI → load `AppConfig` (TOML, cached — never re-read) → `run_service_loop` (mode dispatch extracted from `main` so the harness can drive it with an injected shutdown signal): `--health-check` / `--protocol-probe` / `--export` / `--import` / `--save-once` exit after their one job; otherwise boot-gated restore (`run_boot_restore`) → `--dry-run`/`--restore` exit here → spawn `reactive_save_session` + await shutdown signal → `shutdown_with_final_save` (sends the watch-based shutdown signal, waits ≤5s for a graceful stop, aborts as deadline fallback, then one final save under a 5s timeout).
2. **Save (reactive)**: subscribe to niri's event stream → niri immediately pushes a full **state-sync burst** (layout-relevant → one debounced save right after subscribe) → layout-relevant events trigger **debounced** (2s) saves; if the stream is unavailable or dies, fall back to the configured interval until niri accepts a subscription again (the accepted probe subscription is used directly, not discarded; the fallback interval is injectable via `run_reactive_save_session` for tests). **A stream dying mid-debounce flushes the pending save** before reconnecting. After 3 consecutive subscriptions that died within 5s, periodic saves are mixed between reconnects (`RAPID_DEATH_FALLBACK_THRESHOLD`). Reconnects after stream death use a capped exponential backoff (1s doubling to 30s; a stream that stayed alive ≥5s resets it). Each event connection is an `EventConnection` owning the socket plus a `try_clone`d shutdown handle — the drive loop shuts the socket down on exit so the parked blocking reader unblocks (no leaked reader threads on shutdown). A capture byte-identical to the file on disk skips backup rotation and the write entirely.
3. **Restore (idempotent)**: read session → on parse failure fall back to most recent valid `.bak` → prepare (sort, drop stateless terminals, warn on suspicious per-app counts, cap at `--max-restore-windows`) → snapshot running windows → `plan_spawns` **matches by workspace first (name, then index), then caps at `saved − running` per app** → spawn through `SpawnLimiter` (global cap 5, **per-app serialization**) → place (output fallback via workspace host, workspace move) → **final focus pass**: focus is applied once, after every spawn task has joined (`SpawnOutcome` carries each task's confirmed id + focused flag), so a later-arriving window cannot steal focus from the saved-focused one.
4. **Boot gate**: `should_restore_on_boot` compares the marker to `boot_id` and **prunes stale markers from previous boots**; a marker matching the _current_ boot is also pruned when the session file has vanished (a fresh restore just seeds a new session). An unreadable `boot_id` always restores and never prunes. The marker is written only after a successful non-dry-run restore.

### Invariants that past bugs paid for (do not regress)

- **All session-file writes go through `atomic_write`.** Temp file + fsync of contents + rename + **fsync of the parent directory** — the last one is what makes the rename survive power loss.
- **Blocking niri IPC happens only inside `niri_send` (`spawn_blocking`).** Inline `Socket` I/O on the runtime serialized every spawn behind one worker on current-thread runtimes (paid for in tests); keep socket calls behind the async wrappers.
- **Restore completes before the save loop starts.** Concurrent save during restore snapshots partial state.
- **Restore failure is non-fatal.** Errors are logged, never returned from `main` — under `Restart=always`, a failing restore crash-loops the service.
- **`--dry-run` never spawns and never writes** — no session file, no marker (regression-tested twice over).
- **Restore is idempotent**: re-running spawns only the per-app deficit; single-instance apps are skipped when any instance runs. The harness test `re_restore_spawns_only_the_missing_windows` guards this.
- **Same-app spawns are serialized** (`SpawnLimiter`) — two instances of one app cannot claim each other's windows and swap workspaces.
- **Boot-scoped restore gate**: one restore per boot; `--retry-attempts` retries _within_ that restore.
- **Zero-valued CLI args are rejected at startup** (`save_interval=0` once caused a tight spin loop); `terminal_state.max_walk_depth = 0` is rejected too.
- **Unchanged saves are skipped** (byte-identical capture → no backup rotation, no write).
- **The event stream must survive protocol drift.** niri-ipc is pinned exactly (`=26.4.0`, Cargo.toml — the crate follows niri's versioning; bump deliberately and re-verify against `src/testdata/` fixtures) but rolling unstable niri adds event variants between releases (`CastsChanged` after 25.11 killed every stream ~2ms after subscribe on 2026-09-15, zero event-driven saves in production for 33h). One undecodable line is WARN-logged (bounded) and conservatively treated as layout-relevant; the reader KEEPS READING (`event_reader` in `src/save.rs`). The fixture test fails if the pin regresses below the captured variants.
- **Request/reply IPC is time-bounded** (`IPC_REQUEST_TIMEOUT` 5s, `src/ipc.rs`) and `main` exits explicitly after the service loop — a niri that accepts but never replies must surface as errors and a clean exit, never a parked blocking thread hanging the runtime drop (a real manager survived SIGTERM until SIGKILL that way on 2026-09-15).
- **Spawn confirmation matches `app_id`** — restoring a terminal launched with a custom `--app-id` (e.g. `foot --app-id=foo`) can never confirm because profile-launched terminals reproduce the default app id. Known limitation; soak carriers must use default app ids.

### Session format compatibility

- `SessionData` is a `#[serde(untagged)]` enum: `Versioned` (current, `SESSION_FORMAT_VERSION = 5`) or legacy plain array. The version is **descriptive, not enforced** — files from versions 1–4 load (newer keys deserialize to defaults) and migrate on next save. Q4 in ROADMAP is resolved; see `docs/example-session.json`.
- v5 adds per-window `layout` (`SavedWindowLayout`: scrolling-layout slot + tile size, via `SavedWindowLayout::from_niri`). Restore does not apply geometry yet — that design waits for the real-hardware soak test.
- serde_json has the `float_roundtrip` feature enabled (Cargo.toml): the format carries `f64` tile sizes and the default float parser is off-by-1-ULP lossy — the property tests catch its removal. Do not drop the feature.
- When changing serialized keys, always add aliases/defaults so old `session.json` files still load, and extend the property tests (round-trip + legacy-alias identity).

### Terminal state recovery

`proc.rs` walks the terminal's process tree to the foreground child (skips shells and helpers like `kitten`, prefers the child matching `tpgid`, then non-helper children over kitty's leaf helper kittens), capturing cmdline + cwd; child discovery falls back to a stat-ppid scan when `/proc/.../children` reports nothing (NixOS wrapper fork shapes). On restore, `build_terminal_restore_command` composes a terminal-specific command. The five profiles (kitty, foot, wezterm, ghostty, alacritty) were **verified against the official CLI docs on 2026-09-04** (kitty/foot take positional commands with no `-e`; foot's `-e` is xterm-compat and ignored; wezterm needs `start --cwd … --`; ghostty uses `--working-directory=…` + `-e`; alacritty `--working-directory` + `-e`); **ghostty and kitty are additionally real-binary verified** (capture + carrier restore on the daily driver, 2026-09-16, via `scripts/soak-test.sh`). Adding a terminal means: a new profile in `TerminalProfile` + app_id in `default_terminal_app_ids` + doc verification + tests.

## Config surface

- CLI (`Config`, clap derive in `src/config.rs`): tunables `--save-interval/--max-backup-count/--spawn-timeout/--retry-attempts/--retry-delay/--max-restore-windows`; behaviors `--dry-run`, `--restore`, `--save-only`, `--save-once` (suspend hook), `--health-check`, `--protocol-probe` (diagnostic modes; like `--health-check`, `--protocol-probe` is deliberately NOT mirrored in module.nix), `--export <DIR>`, `--import <DIR>`, `--config-file <PATH>` (explicit path missing = error; default path missing = template written). Validation in `Config::validate`. `--retry-delay` is the base of an exponential backoff (doubles per attempt, capped at 30s).
- TOML: `AppConfig` at `$XDG_CONFIG_HOME/niri-session-manager/config.toml`. Invalid TOML warns and falls back to defaults; a missing default file is created from the embedded template (`DEFAULT_APP_CONFIG_TOML`).
- NixOS module (`module.nix`): mirrors **6 of 7** tunables (`maxRestoreWindows` included; `dryRun` is CLI-only by design) + `saveOnSuspend` (default true) which installs a `sleep.target`-ordered oneshot running `--save-once`. When adding a CLI flag, update `module.nix` and the README together.

## Testing

- 139 tests + 1 `#[ignore]`d benchmark. Unit tests live in `src/tests.rs`; IPC integration tests live in `src/fake_niri.rs` against the fake server. `src/testdata/niri-event-stream-2026-08-02.jsonl` is a sanitized REAL capture (niri unstable 2026-08-02) that must parse under the pinned niri-ipc — it guards the pin and the tolerant-reader contract. Also pinned: the quoted bare-string request wire format, the exact-pin policy itself (a test reads Cargo.toml), a proptest that arbitrary event lines never kill the reader, and an end-to-end `--save-once` (suspend-hook) run.
- **CI asserts the passing test count** (`.github/workflows/checks.yml`, `expected=139`): adding tests is free, dropping tests fails CI. Bump `expected` only when intentionally removing tests. CI also runs a `--dry-run` smoke with a fresh `XDG_DATA_HOME` and no niri — it must exit 0 (restore failure is non-fatal); this is the guard that catches an accidentally fatal niri-absent path. First real GitHub execution 2026-09-15: green end-to-end (run `34990898129`).
- Never test against a live niri session in the SUITE; the fake server covers restore, save, shutdown (graceful + final save), idempotency, focus, retries, concurrency (global cap + per-app serialization), health, the event stream (including the state-sync burst + unknown-variant poison), stream-death flush, rapid-death fallback, never-replying-socket shutdown, the polling fallback with recovery, and `--save-only` end-to-end (see `layout_event_triggers_debounced_save`, `state_sync_burst_and_unknown_event_lines_do_not_kill_the_stream`, `stream_death_mid_debounce_flushes_the_pending_save`, `rapid_stream_deaths_engage_periodic_fallback_saves`, `service_shuts_down_when_ipc_accepts_but_never_replies`, `polling_fallback_saves_when_event_stream_refused_then_recovers`, `save_only_skips_boot_restore_and_runs_the_save_loop`). Real-hardware verification belongs to `scripts/soak-test.sh` (first fully green run 2026-09-16), NOT the suite.
- Tests that touch `$NIRI_SOCKET` MUST hold `IPC_ENV_LOCK` (`FakeNiri::env()` / `FakeNiri::env_without_socket()`); parallel tests race on the environment otherwise.
- **Never assert spawn arrival order or spawned window ids in harness tests.** Spawn requests legitimately interleave across apps (proven: the pre-2026-09-15 focus test passed only because inline blocking I/O accidentally serialized arrivals on the current-thread test runtime). Assert app identity via `FakeNiri::window_app_id(id)` instead. A 2026-09-15 audit of `fake_niri.rs`/`tests.rs` found no other order-coupled assertions; keep it that way. Same reasoning: after any concurrency-affecting change, run the full suite ~5× before declaring it green — one pass is not a verification for 2s-debounce, polling, and arrival-sensitive tests.
- `#[tokio::test]` defaults to a current_thread runtime. All niri IPC runs behind `niri_send` (`spawn_blocking`), so spawns no longer serialize the runtime; the concurrency-metering tests still use `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]` so runtime work and blocking-pool work genuinely overlap.
- Clippy policy (since 2026-09-15): the three test modules (`fake_niri.rs`, `tests.rs`, proc.rs's inline `mod tests`) carry scoped `#![allow(...)]`/`#[allow(...)]` exemptions (pedantic, nursery, unwrap/expect/indexing/panic/arithmetic/as_conversions), so `cargo clippy --all-features --all-targets` is fully clean — run that form locally. CI still runs the testless form. Harness tests keep using `unwrap`/`expect` freely; the footgun denies (`todo`/`unimplemented`/`exit`/`unreachable`/`string_slice`/`panic_in_result_fn`) still apply inside tests. A bare buildflow run (outside `nix develop`) reports cargo-audit/cargo-deny as "no such command" — they are devshell-only by design; run `nix develop -c buildflow ...` for the full gate (CI covers cargo-deny regardless).
- After timeout-joining a `JoinHandle` via `&mut`, do NOT await the owned handle again (double-poll panics: "JoinHandle polled after completion") — await only in the elapsed/abort arm.

## Docs map

- `README.md` (user-facing), `FEATURES.md` (honest feature status), `TODO_LIST.md` (open bounded work), `ROADMAP.md` (vision + resolved/open questions), `CHANGELOG.md` (per-version changes), `CONTRIBUTING.md` (process + rules). Keep all in sync with code; done TODO items move to CHANGELOG.
- `docs/planning/` holds the executed Pareto plan; `docs/status/*.md` are annotated point-in-time reports (archived under `docs/status/archived/` once fully resolved) — historical evidence, not current truth.
- `docs/example-session.json` shows the current format; `docs/benchmarks/restore-burst.md` records benchmark methodology + numbers.

## Known Issues (open, pre-existing or accepted)

- Terminal flag profiles are doc-verified; ghostty, kitty, foot, and alacritty additionally have real-binary carrier coverage (2026-09-16, `CARRIER=… scripts/soak-test.sh c`, 12/12 each). wezterm remains doc-verified only (not installed on the daily driver) — ROADMAP Q3 still asks which terminals are daily drivers.
- If shutdown grace expires (save task wedged >5s), the abort path skips the event-connection cleanup, so the parked reader leaks until process exit — bounded in practice because request/reply IPC is time-bounded and `main` exits the process explicitly; the graceful path is the norm.
- vulnix (buildflow) flags build-time stdenv toolchain advisories (binutils/bison/zlib — 20 derivations); the shipped runtime closure is 7 store paths and clean, so these are accepted, and the scan needs >2 min so buildflow's default timeout kills it first.
- jscpd flags the deliberate test-fixture similarity in proc.rs/fake_niri.rs; in-code rationale comments mark the clones (each test's fixture is its input data) — do not extract.
- `nix flake check` prints `evaluation warning: nixfmt-rfc-style is now the same as pkgs.nixfmt` — an upstream treefmt-nix/nixpkgs deprecation, not this repo (`flake.nix` already uses the current `nixfmt.enable = true`). Investigating it again is wasted time; it is accepted.
