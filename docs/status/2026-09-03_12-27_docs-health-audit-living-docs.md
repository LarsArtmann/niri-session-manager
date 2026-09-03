# Status Report — Docs-Health Audit: Living Docs & Archive Session

**Date:** 2026-09-03 12:27 CEST
**Branch:** main
**Base:** `915216c` (post bug-review session)
**Head:** `1783e77` (auto-commit daemon; working tree clean)
**Commits this session:** 3 daemon commits (`1cc6821` living docs created, `6080cfa` README/AGENTS/ROADMAP/TODO updates + archive, `1783e77` final state)

**Session scope:** Full docs-health AUDIT per user command: view all `**/2026-0*` files, execute docs-health skill, make TODO_LIST / CHANGELOG / AGENTS / README / ROADMAP / FEATURES superb, archive fully-done-and-updated reports. No code changes intended, none made.

---

## a) FULLY DONE

### Audit & verification

| # | Item                                                                                                                                                                                                                                                                                                                    | Evidence                                        |
| - | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| 1 | Both `2026-0*` status reports viewed 100% (the only matches for the glob)                                                                                                                                                                                                                                               | `docs/status/` listing before archive           |
| 2 | Baseline proven healthy before any doc work: 63/63 tests, `nix flake check` green, `cargo build` 0 warnings                                                                                                                                                                                                             | terminal runs, session start                    |
| 3 | The 2 rust-analyzer warnings in `main.rs` (duplicate attribute, dead `shell_escape_empty`) proven to be **stale LSP cache**, not real — no phantom "fix" applied                                                                                                                                                        | `cargo build` + read of `src/main.rs:1125-1140` |
| 4 | Every concrete claim in README/AGENTS verified against code: CLI surface (7 flags, defaults), `SESSION_FORMAT_VERSION = 3`, package name, systemd values (`RestartSec 2s`, `StartLimitBurst 5`, `OOMScoreAdjust`), config example vs `default_shell_names()` (exact match), default-config creation (`src/main.rs:282`) | greps + file reads this session                 |
| 5 | Cross-file consistency pass: no stale `docs/status/2026` references, all pointers resolve, features/status vocabulary consistent                                                                                                                                                                                        | final greps                                     |

### Living docs (4 built from zero, 2 corrected)

| #  | Item                   | Notes                                                                                                                                                                                                               |
| -- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 6  | `TODO_LIST.md` created | 31 open bounded items in 3 impact tiers; every row cites `file:line` evidence + status; no done items; harvest sources footnoted                                                                                    |
| 7  | `CHANGELOG.md` created | Keep-a-Changelog format; 0.2.0 → 0.3.0 → 0.4.0 → Unreleased reconstructed from git hashes and both reports; 0.4.0's 7 bug fixes now visible to SystemNix with per-fix commit hashes                                 |
| 8  | `FEATURES.md` created  | Honest statuses, never rounded up: atomic writes 🟡 (no parent-dir fsync), terminal restore flags 🟡 (never verified vs real CLIs), restore 🟡 (non-idempotent), NixOS module 🟡 (5/7 flags)                        |
| 9  | `ROADMAP.md` created   | 4 themes, **Open Questions** section capturing all 4 unanswered maintainer questions from both reports, explicit non-goals (no history rewrite, no upstream-feature duplication)                                    |
| 10 | `README.md` fixed      | Added missing `--max-restore-windows` (default 100); new **Behavior Notes** section (boot gate, dry-run contract, retry semantics, non-idempotency warning); dev commands aligned with CI (`clippy --all-features`) |
| 11 | `AGENTS.md` corrected  | module.nix "mirrors all CLI flags" → truthful "5 of 7" with reasons; line counts refreshed (~1900/436); new **Docs map** section; known-issues path updated to `docs/status/archived/`                              |

### Harvest, annotation, archive

| #  | Item                                             | Notes                                                                                                                                                                            |
| -- | ------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 12 | HARVEST complete                                 | All 65 forward-looking items from both reports routed: bounded → TODO_LIST (31), vague/blocked/raw → ROADMAP, done → CHANGELOG. Zero items lost, zero duplicated                 |
| 13 | 09-03 report annotated: 67 items resolved inline | 10 strikes via `annotate-rows.py`/`annotate-prose.py` (dry-run first, shape-verified), 57 routing pointers, B2 struck via script, D6 pointered                                   |
| 14 | 07-03 report annotated: 59 items resolved inline | 2 strikes (C10/F10 → `done at 1cc6821`), 57 pointers; A-section + D-table deliberately SKIP (rows already carry their own fix-commit hashes — striking would duplicate evidence) |
| 15 | Both reports archived                            | `git mv docs/status/2026-*.md → docs/status/archived/` (history-preserving)                                                                                                      |

### Final gates

| #  | Item              | Result                                         |
| -- | ----------------- | ---------------------------------------------- |
| 16 | `cargo test`      | 63 passed, 0 failed                            |
| 17 | `nix flake check` | all checks passed (treefmt + statix)           |
| 18 | Working tree      | clean; daemon committed everything (`1783e77`) |

---

## b) PARTIALLY DONE

| # | Item                                | State                                                                                                                                                                                                                                                                                |
| - | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1 | **README superb-ness**              | Fixed 3 gaps, but the NixOS options table still documents only the 5 options module.nix actually has (correct, but the asymmetry with CLI flags is undocumented there — it lives in AGENTS/TODO_LIST instead); CI badge still missing                                                |
| 2 | **CHANGELOG completeness**          | 0.4.0 and 0.3.0 are hash-backed; the **0.2.0 section is thin** — pre-daemon history is unlabeled and reconstructed from 2 reports + git log only. Changes without report coverage are invisible                                                                                      |
| 3 | **Annotation evidence granularity** | Strikes cite exact commit hashes where identifiable (`b97db66`, `e4bf031`, …) but fall back to "docs-health pass 1cc6821" / bare dates for doc-creation items and for fixes whose auto-commit is ambiguous (`01b7640` is a guess for the 1-line F1 fix). Consistent-ish, not uniform |
| 4 | **FEATURES 🟡 statuses**            | Several rest on "works in daily use" inference, not end-to-end verification this session (restore path, terminal flags). Honest, but the honesty is itself untested claims                                                                                                           |
| 5 | **AGENTS.md test count**            | "63 as of this writing" is still a hand-maintained number — the staleness class is documented, not eliminated                                                                                                                                                                        |

---

## c) NOT STARTED

| # | Item                                                                                                                                                                                                         | Why                                                                                                                                               |
| - | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1 | All 31 TODO_LIST items — incl. the three High-Impact correctness items: count-based idempotent restore, per-app spawn serialization, main() refactor + regression tests for the 0.4.0 dry-run/shutdown fixes | This session was docs-only by scope                                                                                                               |
| 2 | Version bump 0.4.0 → 0.4.1 + release + SystemNix pin bump                                                                                                                                                    | Blocked on maintainer release-policy answer (ROADMAP Q1)                                                                                          |
| 3 | ROADMAP Open Questions 1–4                                                                                                                                                                                   | Maintainer decisions; not mine to make (release policy, idempotent-restore semantics, terminal ground truth, SESSION_FORMAT_VERSION 3→4)          |
| 4 | `cargo audit` / dependency refresh                                                                                                                                                                           | cargo-audit not installed in devshell (TODO_LIST T14)                                                                                             |
| 5 | `docs/DOMAIN_LANGUAGE.md`                                                                                                                                                                                    | Deliberately omitted: 2-file, ~2,300-line binary with no complex ubiquitous language; AGENTS.md carries the domain terms. Revisit if domain grows |
| 6 | Regression tests for the 0.4.0 fixes (dry-run marker, dry-run save, shutdown final save)                                                                                                                     | Requires main() refactor first (TODO_LIST T3) — still the top testing debt                                                                        |

---

## d) TOTALLY FUCKED UP

**Nothing this session broke the build, tests, repo state, or any doc.** All gates green at close; no destructive operations; archive used `git mv`. Honest mistakes and judgment calls that could bite later:

| # | Mistake or gap                                                                                                                                                                                                                     | Consequence                                                                                                                                                                                                                                                                                                              |
| - | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1 | **Three tool-shape failures in one session** (09-03 multiedit hit "file modified since read" after script writes; README edit assumed pipe-table where a plain code fence was; 07-03 C section assumed prose where it was a table) | Three wasted round trips. Same root cause each time: I did not re-verify the file's _current_ shape before editing/invoking the tool. The multiedit-after-script one is a repeat of a known lesson                                                                                                                       |
| 2 | **Archive-criterion judgment call**                                                                                                                                                                                                | I archived both reports while open items carry routing pointers (not strikethrough-done). Defensible reading of "every item resolved" (each has an inline verdict + new home), but a stricter reading of the skill says annotate-in-place only until every item is truly DONE. If you disagree, `git mv` back is trivial |
| 3 | **F1 fix hash is a guess**                                                                                                                                                                                                         | I annotated the duplicate-`#[test]` fix as `01b7640` based on a 1-line diff heuristic; I did not diff-verify each of the 6 unlabeled auto-commits against each bug. Citation confidence is high for F4/F5+F6/F7+F8, medium for F1                                                                                        |
| 4 | **Evidence-granularity inconsistency**                                                                                                                                                                                             | Some doc-work annotations cite hashes, others bare "docs-health pass 2026-09-03". No written policy existed before I started annotating; I picked one mid-flight                                                                                                                                                         |
| 5 | **Background-run swallowed cargo test output**                                                                                                                                                                                     | Chained `cargo test \| tail -1` with a slow `nix flake check` in one background command; had to re-run the test count. Sloppy command composition                                                                                                                                                                        |
| 6 | **FEATURES claims written before the last verification grep**                                                                                                                                                                      | The config-example/shell_names claim could have been wrong when I wrote FEATURES row evidence; the final check happened to confirm it. Verification order was luck, not discipline                                                                                                                                       |
| 7 | **0.2.0 CHANGELOG section is reconstruction, not record**                                                                                                                                                                          | Documented with an explicit footnote, but it's the weakest link in the changelog's promise                                                                                                                                                                                                                               |

---

## e) WHAT WE SHOULD IMPROVE

### Docs system

1. **Kill hand-maintained counts.** Test count lives in AGENTS.md prose; a CI job (or a flake check) greps actual counts vs docs, or drop numbers from prose entirely. Same for the "5 of 7 CLI flags" fact — it will rot the moment someone adds a flag.
2. **Annotate-evidence policy in AGENTS.md.** One paragraph: when to cite `done at <hash>` vs `docs-health pass <date>` vs routing pointers, and when A-section rows are SKIP. Next annotator shouldn't improvise like I did.
3. **Link/consistency linter.** Internal markdown references and `file:line` citations in TODO_LIST/FEATURES are unchecked; a tiny CI script would catch rot (e.g. `src/main.rs:896` moves on refactor).
4. **docs-health ↔ status-report loop.** This report's section (f) must be harvested into TODO_LIST/ROADMAP or explicitly dropped — otherwise the timestamped file becomes the new ghost backlog the skill warns about.
5. **CHANGELOG 0.2.0 honesty**: either dig pre-daemon git archaeology once, or shorten the section to "initial feature set (details unlabeled in history)".

### Content

6. **Force a SESSION_FORMAT_VERSION decision** — the question is 2 months old (07-03 report G), 5-minute fix, still open.
7. **module.nix symmetry**: add `maxRestoreWindows` (TODO_LIST T11); document `dryRun` as CLI-only by design in module docs.
8. **README options table**: note the 5-of-7 asymmetry inline so users don't hunt for missing options.
9. **Age the 🟡 FEATURES statuses honestly each release**: re-verify terminal flags vs real CLIs and restore idempotency before any of them graduate to 🟢.
10. **Auto-commit daemon vs citation archaeology**: hand-work worth citing deserves real commit messages when feasible; the hash-hunting I did this session (per-fix mapping) shouldn't be needed again.

---

## f) UP TO 50 THINGS WE SHOULD GET DONE NEXT

_Impact-sorted brainstorm. Items 1–31 already live in TODO_LIST.md (cited by T#); 32+ are new/ROADMAP-fuel from this session and need routing._

| #  | Task                                                                                                  | Impact | Effort | Home         |
| -- | ----------------------------------------------------------------------------------------------------- | ------ | ------ | ------------ |
| 1  | Count-based idempotent restore (T1)                                                                   | High   | Med    | TODO_LIST    |
| 2  | Serialize same-app spawns — workspace-swap race (T2)                                                  | High   | Med    | TODO_LIST    |
| 3  | main() refactor + regression tests for 0.4.0 fixes (T3)                                               | High   | Med    | TODO_LIST    |
| 4  | Fake-socket IPC integration harness (T4)                                                              | High   | High   | TODO_LIST    |
| 5  | Version 0.4.1 + release + SystemNix pin bump (T5)                                                     | High   | Low    | TODO_LIST    |
| 6  | niri event-stream reactive saves (T6)                                                                 | High   | High   | TODO_LIST    |
| 7  | Restore outcome type — kill dry_run branching (T7)                                                    | Med    | Med    | TODO_LIST    |
| 8  | fsync parent dir in atomic_write (T8)                                                                 | Med    | Low    | TODO_LIST    |
| 9  | Verify terminal flags vs real CLIs (T9)                                                               | Med    | Low    | TODO_LIST    |
| 10 | Focus restoration (T10)                                                                               | Med    | Med    | TODO_LIST    |
| 11 | module.nix `maxRestoreWindows` option (T11)                                                           | Med    | Low    | TODO_LIST    |
| 12 | Restore retry-loop injection test (T12)                                                               | Med    | Med    | TODO_LIST    |
| 13 | Multi-monitor output matching (T13)                                                                   | Med    | High   | TODO_LIST    |
| 14 | cargo audit + dep refresh (T14)                                                                       | Low    | Low    | TODO_LIST    |
| 15 | cleanup_old_backups tests (T15)                                                                       | Low    | Low    | TODO_LIST    |
| 16 | dedupe PID-crossing-app edge tests (T16)                                                              | Low    | Low    | TODO_LIST    |
| 17 | Property tests for serialization (T17)                                                                | Low    | Med    | TODO_LIST    |
| 18 | Dry-run output snapshot test (T18)                                                                    | Low    | Low    | TODO_LIST    |
| 19 | `--config-file` override (T19)                                                                        | Low    | Low    | TODO_LIST    |
| 20 | restore-marker staleness cleanup (T20)                                                                | Low    | Low    | TODO_LIST    |
| 21 | Zero-terminals-matched warning (T21)                                                                  | Low    | Low    | TODO_LIST    |
| 22 | `--version` smoke test in CI (T22)                                                                    | Low    | Low    | TODO_LIST    |
| 23 | Health-check subcommand (T23)                                                                         | Low    | Low    | TODO_LIST    |
| 24 | CI badge (T24)                                                                                        | Low    | Low    | TODO_LIST    |
| 25 | CONTRIBUTING.md (T25)                                                                                 | Low    | Low    | TODO_LIST    |
| 26 | cargo-deny in CI (T26)                                                                                | Low    | Low    | TODO_LIST    |
| 27 | idx=Some(0) clamp-vs-skip decision (T27)                                                              | Low    | Low    | TODO_LIST    |
| 28 | max_walk_depth bound check (T28)                                                                      | Low    | Low    | TODO_LIST    |
| 29 | Example session.json in docs/ (T29)                                                                   | Low    | Low    | TODO_LIST    |
| 30 | SHELL-unset test handling (T30)                                                                       | Low    | Low    | TODO_LIST    |
| 31 | `--restore` / `--save-only` run modes (T31)                                                           | Low    | Low    | TODO_LIST    |
| 32 | Decide SESSION_FORMAT_VERSION 3→4 (ROADMAP Q4, 2 months stale)                                        | Med    | 5 min  | ROADMAP→TODO |
| 33 | CI docs-freshness job: grep test counts + CLI-flag count vs README/AGENTS                             | Med    | Low    | new          |
| 34 | Internal-link + file:line citation linter for md docs                                                 | Med    | Low    | new          |
| 35 | Annotate-evidence policy paragraph in AGENTS.md                                                       | Low    | 15 min | new          |
| 36 | Note the 5-of-7 module-options asymmetry in README options table                                      | Low    | 10 min | new          |
| 37 | 0.2.0 CHANGELOG archaeology or honest shortening                                                      | Low    | 30 min | new          |
| 38 | Real commit messages for hand-work (daemon makes citation archaeology expensive)                      | Med    | —      | process      |
| 39 | Pre-release re-verification checklist for every 🟡 FEATURES status                                    | Med    | Low    | new          |
| 40 | Post-archive sanity: confirm docs-health ANNOTATE recognizes `docs/status/archived/` on future passes | Low    | 10 min | new          |
| 41 | systemd sleep.target hook (save on suspend/hibernate)                                                 | Med    | Med    | ROADMAP-fuel |
| 42 | Fuzz session.json parser (serde edge cases)                                                           | Low    | Med    | ROADMAP-fuel |
| 43 | Benchmark restore burst (20+ windows) for semaphore tuning                                            | Low    | Med    | ROADMAP-fuel |
| 44 | Session export/import CLI (backup before risky ops)                                                   | Low    | Med    | ROADMAP-fuel |
| 45 | Workspace-aware save throttling (skip saves when layout unchanged)                                    | Med    | Med    | ROADMAP-fuel |
| 46 | Evaluate niri upstream session features for overlap → possible non-goal additions                     | Med    | Low    | ROADMAP      |
| 47 | aarch64 / `--all-systems` flake check in CI                                                           | Low    | Low    | ROADMAP      |
| 48 | crates.io publish + release automation (cargo-release)                                                | Low    | Med    | ROADMAP      |
| 49 | DMS session-state display spike                                                                       | Low    | Med    | ROADMAP      |
| 50 | docs/DOMAIN_LANGUAGE.md if domain vocabulary grows beyond AGENTS.md                                   | Low    | Low    | defer        |

---

## g) QUESTIONS FOR YOU (cannot answer myself)

1. **Release policy:** Cut **0.4.1 now** so SystemNix can pin the 0.4.0 behavior fixes (dry-run contract, shutdown final save, flatpak terminal restore), or batch the release with the idempotent-restore work? This blocks TODO_LIST T5 and gates when downstream sees the fixes.
2. **Idempotent restore semantics:** When N windows of an app are saved and M are already running, should restore spawn the **first N−M saved entries** (my standing proposal), or match by **workspace first**? This unblocks the top TODO_LIST item (T1) and the per-app serialization design (T2).
3. **Terminal ground truth:** Which terminal emulators do you actually run on real hardware (kitty / foot / wezterm / ghostty / alacritty)? Those profiles become must-not-regress; I'd verify the rest against CLI docs only (unblocks T9 and would have answered this two sessions in a row).

---

_Point-in-time snapshot. Forward-looking items live in TODO_LIST.md / ROADMAP.md; if those weren't updated from this report's section (f), run docs-health HARVEST next. Items 1–31 already harvested; 32–50 need routing._
