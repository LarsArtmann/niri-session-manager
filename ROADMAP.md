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
  _Shipped 2026-09-04 with workspace-first matching on top._
- Per-app spawn serialization to eliminate same-app workspace-swap races
  _Shipped 2026-09-04 (`SpawnLimiter`)._
- Output matching by EDID/position instead of name
  _Partially shipped: workspace-hosting output fallback. True position/EDID
  matching remains blocked — niri's IPC exposes no output positions._
- Window size / column-width capture — _capture shipped 2026-09-15_ as
  session-format v5 (`SavedWindowLayout` in `src/session.rs`; upstream
  `Window.layout` verified against the resolved crate source). Still raw:
  **applying** the captured geometry at restore — decide resize vs.
  column-index navigation first, and gate on the real-hardware soak test
  (the captured data currently has no consumer; see TODO_LIST for the
  interim `--health-check` coverage row).
- Watch niri upstream IPC for window-position application support before
  building our own geometry applier
- Multi-monitor soak scenarios: output rename/reorder while windows are open
- Duplicate-window dedup on save (distinct from single-instance dedup)
- Config hot-reload via file watching (inotify) — restart picks up changes today
- Per-app restore delay tuning, `--migrate` session-file migration command
- ~~Spawn-timeout exponential backoff~~ done 2026-09-15: `--retry-delay` is the base of a capped exponential backoff (`next_retry_delay`, see `CHANGELOG.md` [Unreleased])
- SSH suspend guard integration
- journald log-volume review (per-window restore info lines)
- Cross-platform CI job (macOS build-only; proc module is linux-gated)
- `nix flake check --all-systems` (aarch64)
- ~~Split `src/main.rs` into modules once boundaries prove stable~~ done 2026-09-15: behavior-frozen split into config/ipc/session/terminal/restore/save (see `CHANGELOG.md` [Unreleased])

### 2. Reactive session keeping

**Shipped 2026-09-04:** niri event-stream subscription with debounced saves
and an interval fallback, plus focus restoration — see FEATURES.md. What
remains raw:

- Layout-change coalescing beyond the fixed 2s debounce (adaptive quiet
  windows during interactive drags)
- Event-flood safety valve in the save loop (cap saves/sec under window churn)
- Restore knowledge of focus across a multi-monitor focus history

### 3. Operability

Make the service observable and debuggable without reading its source.

Raw ideas:

- systemd notify readiness signaling (`Type=notify`)
- ~~Spawn-timeout exponential backoff~~ done 2026-09-15: `--retry-delay` is the base of a capped exponential backoff (`next_retry_delay`, see `CHANGELOG.md` [Unreleased])
- SSH suspend guard integration
- journald log-volume review (per-window restore info lines)
- Dry-run output designed for humans _and_ for machine diffing
- IPC health/status endpoint beyond the `--health-check` one-shot
- `--print-config` support/debug mode (dump effective CLI+TOML config)
- Per-window outcome summary log line at restore end
- Export/import scope: decide and document whether the restore marker belongs in exports
- Backup compression or retention-policy review
- `--retry-base-delay` rename decision (the flag now takes a base delay, not a fixed delay; a rename is a breaking CLI change for SystemNix — coordinate before pinning)

### 4. Supply chain and packaging

Raw ideas:

- Coverage reporting in CI
- `nix flake check --all-systems` (aarch64)
- Config TOML structural fuzzing beyond arbitrary-input no-panic
- Broader harness window accessor if more tests need identity lookups

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
   Implemented in `plan_spawns` (`src/restore.rs`) with 8 unit tests plus an
   end-to-end re-restore test against the fake IPC server.
3. **Terminal ground truth** — still open: which terminals run on real
   hardware daily? Those profiles become must-not-regress; the rest get
   verified against CLI docs only.
4. **`SESSION_FORMAT_VERSION` 3 → 4 — RESOLVED.** Bumped to 4. The version is
   descriptive, not enforced: files from versions 1–3 still load via
   `#[serde(alias)]`; the bump marks the key-name change honestly.
   See `docs/example-session.json` for the current shape.
   _Superseded 2026-09-15: format v5 added per-window layout capture; the
   version is still descriptive and pre-v5 files keep loading._

## Non-goals

Things we are deliberately NOT pursuing and why:

- **Rewriting published git history** (e.g. squashing unlabeled auto-commits):
  SystemNix pins this flake; history rewrites break downstream pins.
- **Building features niri already provides**: before implementing any
  restore-adjacent feature, evaluate niri's native session support to avoid
  duplicating upstream work. **Re-verified 2026-09-04** against niri releases
  through v26.04: no native session save/restore exists — only static
  `spawn-at-startup`, manual `window-rule` placement, and the IPC surface this
  tool builds on. We are complementary, not duplicative. Re-check on major
  niri releases.
- **Window size capture ahead of upstream**: blocked by niri IPC (no layout
  geometry exposed); revisit when upstream exposes it.
  _Superseded 2026-09-06: upstream exposed layout geometry (niri-ipc
  `WindowLayout`, since v25.08) and the pinned crate already carries it — the
  capture feature moved back to Ideas pending a session-format v5 design._
- **Library-ification with structured error types** (`thiserror` at module
  boundaries): premature while the crate is a binary consumed via flake.
- **Reactive saves before the event-stream foundation exists**: hot-reload and
  backoff ideas stay raw until the subscription model lands.
  _Superseded 2026-09-04: the event-stream foundation shipped (M12)._
- **Publishing to crates.io** (evaluated 2026-09-04): the crate is a flake
  binary with no library API, a `#[cfg(test)]` IPC fake, and Nix as the only
  supported install path. Publishing adds maintenance (version discipline,
  README packaging) for no consumer — defer until there is one.
- **DMS (DankMaterialShell) integration** (spiked 2026-09-04): a session-state
  display belongs behind a stable health/status surface (`--health-check`
  today, an IPC endpoint eventually). Defer until DMS expresses interest.
- **`docs/DOMAIN_LANGUAGE.md`** (deferred 2026-09-04): the domain vocabulary
  (saved/running/matched/deficit, boot gate, restore outcome) is small and is
  documented inline in `plan_spawns` and the README Behavior Notes. Revisit
  when the vocabulary outgrows a page.

---

_Reconstructed 2026-09-03 from the archived status reports in `docs/status/archived/`._
