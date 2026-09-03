# Roadmap

> Long-term direction and raw ideas. Items here are NOT actionable tasks.
> When an idea is refined into bounded work, it moves to TODO_LIST.md.

## Themes

### 1. Restore correctness as a guarantee

Today restore is best-effort: it runs once per boot, and a partial failure can
duplicate windows on retry. The destination is a restore that is _idempotent by
construction_ — safe to re-run at any time — with placement that survives
monitor changes.

Raw ideas:

- Count-based idempotent restore (spawn `saved − running` per app)
- Per-app spawn serialization to eliminate same-app workspace-swap races
- Output matching by EDID/position instead of name
- Window size / column-width capture (blocked on niri IPC — evaluate upstream first)
- Duplicate-window dedup on save (distinct from single-instance dedup)
- Config hot-reload via file watching (inotify) — restart picks up changes today
- Per-app restore delay tuning, `--migrate` session-file migration command

### 2. Reactive session keeping

Move from periodic polling to a model where the session file is always
current and restore knows about focus, not just window lists.

Raw ideas:

- niri event-stream subscription: save on layout change, not every 15 minutes
- Focus restoration (`is_focused` is already saved, never used)
- Debounced saves to avoid write storms during interactive layout changes

### 3. Operability

Make the service observable and debuggable without reading its source.

Raw ideas:

- Health-check surface (`--health-check` subcommand or IPC endpoint)
- systemd notify readiness signaling (`Type=notify`)
- Spawn-timeout exponential backoff
- SSH suspend guard integration
- journald log-volume review (per-window restore info lines)
- DMS (DankMaterialShell) integration for session state display
- Dry-run output designed for humans _and_ for snapshot testing

### 4. Supply chain and packaging

Raw ideas:

- Publish to crates.io
- cargo-deny in CI, coverage reporting
- Cross-platform CI job (macOS build-only; proc module is linux-gated)
- `nix flake check --all-systems` (aarch64)

## Open Questions (maintainer decisions pending)

1. **Release policy** — bump to 0.4.1 and cut a release now so SystemNix can
   pin the 0.4.0 behavior fixes, or batch with the idempotent-restore work?
   (from `2026-09-03` report G.1)
2. **Idempotent restore semantics** — when N windows of an app are saved and M
   are already running, spawn the first N−M saved entries, or match by
   workspace first? (from `2026-09-03` report G.2)
3. **Terminal ground truth** — which terminal emulators are actually used on
   real hardware? Those profiles become must-not-regress; the rest get verified
   against CLI docs only. (from `2026-09-03` report G.3)
4. **`SESSION_FORMAT_VERSION` 3 → 4?** — serialized keys changed
   (`workspace_idx`→`idx` etc.) while the version stayed 3. Old files still
   load via `#[serde(alias)]`. Bumping is honest; keeping avoids churn.
   (from `2026-07-03` report G)

## Non-goals

Things we are deliberately NOT pursuing and why:

- **Rewriting published git history** (e.g. squashing unlabeled auto-commits):
  SystemNix pins this flake; history rewrites break downstream pins.
- **Building features niri already provides**: before implementing any
  restore-adjacent feature, evaluate niri's native session support to avoid
  duplicating upstream work.
- **Window size capture ahead of upstream**: blocked by niri IPC; revisit when
  upstream exposes layout geometry.
- **Library-ification with structured error types** (`thiserror` at module
  boundaries): premature while the crate is a binary consumed via flake.
- **Reactive saves before the event-stream foundation exists**: hot-reload and
  backoff ideas stay raw until the subscription model lands.

---

_Reconstructed 2026-09-03 from the archived status reports in `docs/status/archived/`._
