# Status Report — Bug Review & AGENTS.md Session

**Date:** 2026-09-03 00:00
**Branch:** main
**Base:** `f082000` (upstream `fix(restore): end the restore-storm class`)
**Head:** `71e1c1b` (auto-commit daemon)
**Commits this session:** 6 (5 auto-commits + 1 AGENTS.md)
**Working tree:** clean (daemon committed everything)

**Session scope:** 1) Generate AGENTS.md for future agents. 2) Full-file bug review of `src/main.rs` (1,847→~1,890 lines) and `src/proc.rs` (436 lines), fix confirmed bugs, verify.

---

## A) FULLY DONE

### Documentation
| # | Item | Notes |
|---|------|-------|
| 1 | `AGENTS.md` created | Commands, architecture, invariants, session-format compat rules, test conventions, known issues. Updated at end of session to reflect fixes. |
| 2 | README test count fixed | "57 tests" → count-agnostic "test suite" (stops the staleness class). |

### Bugs found and fixed (7)
| # | Bug | Severity | Fix |
|---|-----|----------|-----|
| 1 | **`--dry-run` wrote the boot restore-marker** → real restore skipped for the entire boot after a preview run | High — silent feature loss | Marker written only when `!dry_run` |
| 2 | **`--dry-run` saved `session.json`** when no session file existed | Medium — violated documented "no spawning or saving" contract | Early return before any write; smoke-verified session file byte-identical |
| 3 | **Shutdown `select!` race killed the final save** — `signal_task` branch completed instantly, aborting the periodic task before its final save; "Final session saved" was effectively dead code; plus a `notify_waiters` lost-wakeup window | High — final save rarely ran | Deterministic flow: await signal task → abort save task (and await the abort, preserving single-writer) → 5s-timeout-guarded inline final save. Smoke-tested with real SIGTERM. |
| 4 | **Flatpak-mapped terminals restored with wrong CLI flags** — profile detection used only the first mapping arg (`flatpak` → Generic profile → `-e` instead of `--`, no `start` subcommand, wrong cwd flag) | Medium — broken restore command for wrapped terminals | New `TerminalProfile::from_args` scans all mapping args, matches reverse-DNS app ids (`org.wezfurlong.wezterm` → Wezterm). +2 tests. |
| 5 | **Corrupt `session.json` entered the backup rotation** — each save cycle backed up the corrupt file, evicting valid backups until recovery was impossible | Medium — slow-burn data loss | `create_backup` skips files that fail `SessionData` parse. +2 tests. |
| 6 | **Guaranteed IPC error per window for windows without workspace info** — `WorkspaceReferenceArg::Index(0)` (niri is 1-based) | Low — log noise, no placement | Move skipped when neither name nor idx present; window stays on active workspace |
| 7 | **Duplicate `#[test]` + missing `#[test]` on `shell_escape_empty`** — the two pre-existing rust-analyzer warnings | Low | Fixed; project now warning-free |

### Housekeeping
| # | Item |
|---|------|
| 8 | Misleading log "(will retry via periodic save)" → accurate "(restore will be attempted again on next service start)". Periodic save never re-restores. |
| 9 | Pre-existing nixfmt drift in `module.nix` (nixpkgs formatter version bump) fixed via `nix fmt` — `nix flake check` green again. |
| 10 | Removed dead `resolve_executable_name` (superseded by `from_args`). |

### Verification performed
- 63/63 tests pass (was 60; net +4 new tests, −1 removed test)
- `cargo clippy --all-features`: 0 issues
- `cargo fmt --all -- --check`: clean
- `nix flake check`: all checks passed
- `nix build`: package builds
- Manual smoke tests: SIGTERM shutdown flow (final-save path exercised, clean exit); dry-run (no marker, no file mutation, stateless-terminal guard still fires)

---

## B) PARTIALLY DONE

| # | Item | State |
|---|------|-------|
| 1 | **AGENTS.md accuracy over time** | Written and updated this session, but it is a point-in-time snapshot. The "Known Issues" section now contains the design gaps below — it needs maintenance when they get fixed. |
| 2 | **Bug fixes recorded** | Everything is in git history (auto-commits, unlabeled) and this report — but there is no CHANGELOG.md and no version bump, so the fixes are invisible to downstream consumers (SystemNix). |
| 3 | **Terminal restore command correctness** | Unit tests assert what the code *builds*, not what real terminals *accept*. kitty/foot/wezterm/ghostty/alacritty flags were never verified against actual CLI help/docs in this session. |

---

## C) NOT STARTED (found during review, deliberately deferred)

| # | Item | Why deferred |
|---|------|--------------|
| 1 | **Count-based idempotent restore** — spawn `saved − currently_running` windows per app instead of all-or-nothing | Semantic change to core restore logic; deserves its own change + the maintainer's product decision |
| 2 | **Same-app claim swap race** — two concurrently restored windows of one app can claim each other's windows and land on swapped workspaces | niri `Action::Spawn` returns no window id; the only deterministic fix is serializing spawns per app (throughput tradeoff) |
| 3 | **`atomic_write` does not fsync the parent directory** | Rename may not survive power loss → reverts to previous session, not corruption. Fix is 3 lines but touches the atomicity invariant; should come with a durability test strategy |
| 4 | **`is_focused` saved but never used on restore** | Pre-existing gap (already in prior status report); focus restoration is a feature, not a bug |

---

## D) TOTALLY FUCKED UP

**Nothing this session broke the build, tests, or behavior.** All gates green at close.

Self-critique (what I forgot / could have done better):

| # | Mistake or gap | Consequence |
|---|----------------|-------------|
| 1 | **No regression tests for fixes #1–#3** — dry-run marker, dry-run save, and the shutdown flow live in `main()`/`restore_session_internal()` and are untestable without refactoring. I smoke-tested manually only. | These exact bugs can silently regress; nothing fails in CI if someone reintroduces them. |
| 2 | **One multiedit failed on a wrong assumption** (assumed `build_spawn_command` followed `resolve_executable_name`; it was `build_terminal_restore_command`) | Cost one extra round trip. Lesson: verify adjacency before writing multi-function old_strings. |
| 3 | **First `cargo test` verification run had a count discrepancy** I noticed (60 passed before AND after adding a test) and hand-waved instead of resolving | Suggests I should have compared "running N tests" lines, not just result lines. Count math: 60 → 63 is now consistent, but the detour showed sloppy verification discipline. |
| 4 | **Didn't check `git log`/`git blame` before reviewing** (workflow says to) | Missed free context on why the boot gate was shaped the way it is; recovered via the status doc instead. |
| 5 | **Didn't run `cargo audit`/`cargo outdated`** | Dependency freshness unverified (niri-ipc 25.5.1 pinned). |
| 6 | **No version bump for behavior-changing fixes** | Dry-run contract, shutdown semantics, and restore command format changed — downstream (SystemNix) gets them invisibly. |
| 7 | **AGENTS.md created in the same session that changed the facts it documents** — first version documented the buggy shutdown behavior as an invariant ("restore completes before periodic save" is still true, but the final-save description changed) | Required a same-session rewrite; minor churn that a post-fix write would have avoided. |

---

## E) WHAT WE SHOULD IMPROVE

### Architecture / correctness
1. **Make restore idempotent** (count-based per app) — closes the retry/restart duplicate window and the partial-failure hole the boot gate can't cover.
2. **Serialize spawns per app_id** (keep cross-app concurrency) — eliminates the workspace-swap race for multi-window same-app restores (very common: multiple terminals).
3. **fsync parent dir in `atomic_write`** — full crash-durability.
4. **Refactor `main()` for testability** — extract boot-gate decision and shutdown sequence into pure/injectable functions so the two nastiest bug classes from this session become unit-testable.
5. **Return restore outcome from `restore_session_internal`** (restored / would-restore / failed) instead of `Result<()>` + internal dry-run branching — kills the scattered `config.dry_run` checks.
6. **Verify terminal profiles against real CLIs** — `wezterm start --cwd X -- sh -c`, `ghostty -e`, `foot positional`, kitty `--directory`, alacritty `-e`. One wrong flag = silently broken terminal restore.

### Testing
7. Integration test with a fake niri socket (Unix listener speaking the IPC protocol) — would have caught/locked in fixes #1–#3.
8. Property tests for session serialization round-trips.
9. Test for `cleanup_old_backups` (currently untested).
10. Test for `dedupe_single_instance_windows` PID-crossing-app edge cases.

### Release / docs
11. Bump version (0.4.0 → 0.4.1) — these are behavior fixes consumers should be able to pin.
12. ~~CHANGELOG.md — session fixes are currently only in unlabeled auto-commits.~~ done (CHANGELOG.md created in docs-health pass 1cc6821)
13. ~~README: add missing `--max-restore-windows` CLI option (exists since v0.3.x, never documented).~~ done (--max-restore-windows documented in README (docs-health pass 2026-09-03))
14. ~~README: document the restore-marker/boot-gate behavior and dry-run semantics.~~ done (boot-gate + dry-run semantics documented in README Behavior Notes (docs-health pass 2026-09-03))
15. ~~Commit messages: auto-commits are "heuristic" noise — the 7 bug fixes deserve one descriptive commit each or one squash with a real message.~~ **Won't implement — rewriting published history breaks SystemNix flake pins (ROADMAP non-goal).**

---

## F) TOP THINGS TO DO NEXT (impact-sorted, ~50)

| # | Task | Impact | Effort |
|---|------|--------|--------|
| 1 | Count-based idempotent restore | High | Medium |
| 2 | Serialize same-app spawns (fix workspace swap) | High | Medium |
| 3 | Refactor main() shutdown + boot gate into testable units + regression tests | High | Medium |
| ~~4~~ | ~~Version bump 0.4.1 + CHANGELOG for this session's fixes~~ done — CHANGELOG created 1cc6821; version bump → TODO_LIST T5 | ~~High~~ | ~~Low~~ |
| 5 | Fake-socket IPC integration test harness | High | High |
| 6 | fsync parent dir in atomic_write | Medium | Low |
| 7 | Verify terminal flags against real wezterm/ghostty/foot/kitty/alacritty | Medium | Low |
| ~~8~~ | ~~Document `--max-restore-windows` in README~~ done — documented in README (docs-health pass 2026-09-03) | ~~Medium~~ | ~~Low~~ |
| 9 | niri event-stream subscription (reactive saves, already planned pre-session) | High | High |
| 10 | Focus restoration (`is_focused` unused) | Medium | Medium |
| 11 | Multi-monitor output-name robustness (match by EDID/position) | Medium | High |
| 12 | `cargo audit` + dependency refresh (niri-ipc track latest niri) | Medium | Low |
| 13 | `cleanup_old_backups` tests | Low | Low |
| 14 | Window size/column-width capture (blocked by niri IPC — upstream?) | Medium | ? |
| 15 | `--config-file` CLI override | Low | Low |
| 16 | Config hot-reload (inotify) | Low | Medium |
| 17 | Health-check subcommand / IPC endpoint | Medium | Medium |
| 18 | thiserror error types at module boundary | Low | Medium |
| 19 | cargo-deny supply chain check in CI | Low | Low |
| 20 | CI badge in README | Low | Low |
| ~~21~~ | ~~Squash this session's auto-commits into descriptive messages (if history rewrite acceptable)~~ **Won't implement — rewriting published history breaks SystemNix pins.** | ~~Medium~~ | ~~Low~~ |
| 22 | dry-run output snapshot test | Low | Low |
| 23 | Property tests for SessionData/SavedWindow round-trip | Medium | Medium |
| 24 | Test restore retry loop with injectable failure injection | Medium | Medium |
| 25 | Log rotation/journald rate review | Low | Low |
| 26 | `restore-marker` staleness cleanup (marker survives boots forever) | Low | Low |
| 27 | Handle `SHELL` unset + passwd lookup failure explicitly in tests | Low | Low |
| 28 | Workspace idx=Some(0) from legacy files: decide clamp vs skip | Low | Low |
| 29 | flake check `--all-systems` in CI (aarch64) | Low | Low |
| 30 | CONTRIBUTING.md | Low | Low |
| 31 | Cross-platform CI job (macOS build only, proc module is linux-gated) | Low | Medium |
| 32 | `--restore` / `--save-only` run modes (currently implicit) | Medium | Low |
| ~~33~~ | ~~Confirm RUST_LOG docs in README (tracing env filter syntax)~~ done — README documents RUST_LOG (verified 2026-09-03) | ~~Low~~ | ~~Low~~ |
| 34 | Example session.json in docs/ | Low | Low |
| 35 | Bench/limit log volume of per-window restore info lines | Low | Low |
| 36 | Cap `max_walk_depth` sanity check in config validation | Low | Low |
| 37 | Warn when `terminal_state.enabled` but zero terminals matched during save | Low | Low |
| 38 | Add `--version` smoke test in CI | Low | Low |
| ~~39~~ | ~~(dead) `select!` import check — unused imports after F4 (verify none remain)~~ done — cargo build emits zero warnings at 915216c | ~~Low~~ | ~~Low~~ |
| 40 | Evaluate niri's native session/restore features for overlap (avoid building upstream features) | Medium | Low |

(Items 41–50 intentionally left unstated rather than padded — the list above is what this session actually surfaced.)

---

## G) QUESTIONS FOR YOU

1. **Release policy:** Should I bump to 0.4.1 and cut a release for these behavior fixes (dry-run contract, shutdown final save, flatpak terminal restore)? SystemNix pins the flake — do you want consumers to get this via a pin bump now, or batch it with the idempotent-restore work?
2. **Idempotent restore semantics:** When a saved session has N windows of app X and M are already running (partial restore / autostart race), should restore spawn the first N−M saved entries (my proposal), or do you want different selection (e.g., match by workspace first)?
3. **Terminal flag ground truth:** Have the kitty/foot/wezterm/ghostty/alacritty restore commands ever been verified on your machine end-to-end? If yes, which terminals do you actually use so I treat those profiles as must-not-regress and can test the rest against CLI docs?
