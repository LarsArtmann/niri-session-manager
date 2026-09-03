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

## Open Questions

Resolved on 2026-09-03/04 during the Pareto execution (kept here for the
record; rationale in the linked code):

1. **Release policy — RESOLVED.** 0.4.1 was cut and pushed immediately so
   SystemNix's flake pin received the 0.4.0 behavior fixes. Standing policy:
   ship accumulated fixes as patch releases instead of batching them behind
   feature work.
2. **Idempotent restore semantics — RESOLVED.** Workspace-first matching
   (name, then index) with a per-app count cap of `saved − running`;
   single-instance apps keep the stronger "skip if any instance runs" rule.
   Implemented in `plan_spawns` (`src/main.rs`) with 8 unit tests plus an
   end-to-end re-restore test against the fake IPC server.
3. **Terminal ground truth** — still open: which terminals run on real
   hardware daily? Those profiles become must-not-regress; the rest get
   verified against CLI docs only.
4. **`SESSION_FORMAT_VERSION` 3 → 4 — RESOLVED.** Bumped to 4. The version is
   descriptive, not enforced: files from versions 1–3 still load via
   `#[serde(alias)]`; the bump marks the key-name change honestly.
   See `docs/example-session.json` for the current shape.

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
