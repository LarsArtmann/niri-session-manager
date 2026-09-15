# Status Report — 2026-09-15 10:42 CEST

> Verification-debt execution round: every item the 09:47 report flagged as owed was
> paid down — including proving the focus race pre-existence with a worktree probe —
> plus the skill-mandated HARVEST of the previous report into the living docs. One
> real bug (Nix source filter vs the new guard test) was caught by the new test on
> its first `nix build`.
>
> **Scope:** this session's run only (the round after `2026-09-15_09-47_…`). No
> unrelated research. All gates re-verified on the final tree.
> **Format note:** written as Markdown at the user's explicit request (the
> status-report skill defaults to styled HTML); matches the repo's `docs/status/*.md`
> convention. One-off override — not a new default for the skill.

---

## Session at a glance

| Metric                        | Value                                                                          |
| ----------------------------- | ------------------------------------------------------------------------------ |
| Verification-debt items paid  | 10 of 10 from the 09:47 report (f1–f10) + f39 + f49                            |
| Focus-race pre-existence      | PROVEN (was "plausible, unverified"): 8/20 release + 7/20 debug probe failures |
| New tests                     | 2 (example-session guard, zero-delay retry e2e) → suite 120 (+1 ignored)        |
| Real bugs found by new tests  | 1 (`nix build` failed: source filter excluded `docs/example-session.json`)      |
| Suite stability               | 118×5 loop, then 120×3 after the additions — all green                          |
| CI-parity checks (first local run) | nixfmt / deadnix / statix / cargo-deny — all green                         |
| Benchmark after spawn_blocking change | Unchanged: 100.4 ms/window (3.013s; was 3.011s)                        |
| HARVEST                       | 09:47 report (f) routed: 6 TODO rows, ~12 ROADMAP ideas, stale entries fixed    |
| Report annotations            | 24 items resolved inline in the 09:47 report (sections b/c/f)                  |
| Git commits by me             | 0 (daemon made 6 heuristic commits this round — changesets stay entangled)      |

---

## a) FULLY DONE

### 1. Suite stability proven, not assumed (f1)

- 118 tests (+1 ignored) green on **5 consecutive full runs** (15.23–15.29s each) before
  any new test was added; after the two additions, 120 (+1) green on **3 further
  consecutive runs** plus targeted runs.
- The rule "one pass is not a verification for 2s-debounce, polling, and
  arrival-sensitive tests" is now written into AGENTS.md so it outlives this session.

### 2. Harness arrival-order audit (f2)

- Swept `fake_niri.rs` and `tests.rs` for order/id assertions: the fixed focus test was
  the **only** cross-app arrival-order coupling. Remaining `.last()`/`[0]` assertions are
  on single-element passes or deterministic (fake-server-stored) state;
  `plan_spawns` order tests are pure-function tests where saved order is the contract.
- Durable rule added to AGENTS.md Testing section: **never assert spawn arrival order or
  spawned ids; assert app identity via `FakeNiri::window_app_id(id)`**.

### 3. Focus-race pre-existence PROVEN (f3) — the round's headline

- Claim in the 09:47 report was "pre-existing by reasoning, not proof". Settled
  empirically: worktree at pre-change commit `8d2386b` (old inline-socket
  `spawn_single_window`, old id-based focus assertion), added a probe test — the old
  scenario on `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]` — and ran it 40×:
  - release: **8/20 failed**, debug: **7/20 failed** (`focus_actions == vec![2]` does not
    hold; ids are assigned in request-arrival order, which interleaves).
- Conclusion: production (`#[tokio::main]` = multi-thread) **never** had
  saved-order spawn arrivals; the old test passed only because the current-thread test
  runtime accidentally serialized arrivals. The race premise (explicit FocusWindow can be
  followed by later spawns) holds pre- and post-change. Honest caveat: the fake does not
  model niri's auto-focus-on-spawn, so the final user-visible "focus lands wrong" step
  still needs real hardware — the *ordering premise* is what is now proven.
- Worktree removed after evidence capture; probe not merged (it encodes a deliberately
  wrong assertion). Evidence recorded in the TODO_LIST focus row.

### 4. Example-session guard test — paid for itself on day one (f4)

- `example_session_doc_deserializes` (`src/tests.rs`): loads `docs/example-session.json`
  via `CARGO_MANIFEST_DIR`, deserializes through the current `SessionData` model, asserts
  versioned + layout present on every window. Closes the "doc can drift from serde
  silently" hole.
- **It immediately caught a real bug:** `nix build` failed because `default.nix`'s
  source filter only admitted `.rs/.toml/.lock/.nix` — the example (and the whole
  `docs/` payload of the test) was absent in the Nix sandbox. Fixed precisely:
  `lib.hasSuffix "docs/example-session.json" path` exception in the filter
  (`default.nix:14`). `nix build` green afterwards.

### 5. Zero-delay retry path covered end-to-end (f10)

- `zero_retry_delay_retries_immediately_after_a_failed_spawn` (`src/fake_niri.rs`):
  `retry_delay = 0`, one injected spawn failure → immediate retry succeeds,
  `RestoreOutcome::Restored { spawned: 1 }`, no duplicate spawn recorded.

### 6. `--help` now tells the truth (f7)

- Doc comments on the five undocumented tunables (`src/config.rs`); verified by reading
  actual `--help` output. `--retry-delay` states: base delay, doubles per attempt,
  capped at 30, "0 retries immediately". The README/module.nix/`--help` split brain from
  the 09:47 report (b4) is closed.

### 7. Benchmark re-run after the spawn_blocking change (f5)

- `restore_burst` (`--release`, `--ignored`): 30 windows in **3.013s** (10 windows/s,
  100.4 ms/window) vs 3.011s recorded 2026-09-04 — unchanged, as expected (the 500ms
  poll quantum dominates; the blocking pool only moves socket waits off async workers).
- `docs/benchmarks/restore-burst.md` now carries the dated re-run line.

### 8. CI-parity checks, first local run (f6)

- Via the CI's own command shapes: `nixfmt-rfc-style --check` (exit 0; known upstream
  deprecation warning only), `deadnix` (clean), `statix check` (clean), `cargo-deny
  check advisories licenses bans sources` (all ok).
- Notes: `markdownlint` is **not** a CI step in this repo (the 09:47 wording was
  imprecise); `cargo-audit`'s advisory function is subsumed by `cargo-deny advisories`.

### 9. AGENTS.md hardened with this round's durable lessons

- Harness ordering rule + suite-loop guidance (Testing section).
- nixfmt-rfc-style evaluation warning recorded as an accepted upstream known-issue —
  ends the "next session re-investigates it" loop (f8, and 09:47 b9).
- Live test counts refreshed 118 → 120.

### 10. HARVEST of the 09:47 report executed (f49)

- **TODO_LIST.md**: 6 bounded Low rows added (health-check geometry coverage; restore/save
  hardening tests batch; CI test-count assertion + timing budget; stale-marker pruning on
  vanished session file; release-flow decision 0.5.1 vs 0.6.0) and the focus-race row
  upgraded with the probe evidence. Footer re-verified (120 tests, debt closed).
- **ROADMAP.md**: v5-capture item updated (capture shipped, *application* remains raw);
  Q4 marked superseded by v5; new raw ideas filed (geometry application, watch-niri-upstream,
  multi-monitor soak, event-flood valve, `--print-config`, per-window outcome summary,
  export/import scope, backup compression, `--retry-base-delay` rename question, config
  fuzzing, harness accessor).
- **CONTRIBUTING.md**: unit tests now correctly located in `src/tests.rs` (fixed on sight,
  f39).
- Resolved items were dropped or deduped, never re-labelled (docs-health discipline).

### 11. Previous report brought current (f50, partially — see b6)

- 24 items of the 09:47 report annotated **inline** via the docs-health annotate tooling:
  (b)4–10, (c)5/8/9/10, (f)1–10/39/49/50 — strikethrough + dated evidence, source file
  untouched otherwise. No appendix-only shortcuts.

### 12. Final verification, on the final tree

| Gate                                  | Result                                   |
| ------------------------------------- | ---------------------------------------- |
| `cargo fmt --all -- --check`          | clean                                    |
| `cargo clippy --all-features`         | 0 errors, 0 warnings (CI form)           |
| `cargo test` ×3                       | 120 passed, 1 ignored, 0 failed, each    |
| `nix build`                           | green (after the default.nix filter fix) |
| `nix flake check`                     | green (known upstream warning only)      |
| `bash scripts/docs-citations.sh`      | all citations and links resolve          |
| `nix fmt`                             | no changes needed                        |

---

## b) PARTIALLY DONE

1. **Test-count honesty:** I first wrote "121 tests" into TODO_LIST and the report
   annotation — that number came from `cargo test -- --list | grep -c`, which includes
   the ignored benchmark. Corrected to "120 passing (+1 ignored)" in the same session at
   final-gate recount. Nothing false shipped, but the error is exactly the class the
   repo's "verify, don't trust" rule targets — applied to my own outputs this time.
2. **The zero-delay test's "immediate" is by construction, not measurement:** v1 of the
   test asserted `elapsed < 500ms` and **failed on its first full-suite run** under
   parallel load. I removed the wall-clock bound (the flake class this round was supposed
   to be closing). The final test proves the zero-delay retry path recovers correctly;
   "no sleep" follows from `next_retry_delay(0, n) == 0` (unit-tested) plus the loop's
   use of it — but no test measures the wall clock, deliberately.
3. **CI-parity naming:** two of the five tools listed in the debt item were not literally
   run (`markdownlint` — not a CI step here; `cargo-audit` — subsumed by `cargo-deny
   advisories`). Substance complete; checklist satisfied by equivalence, not identity.
4. **Captured v5 geometry still has no consumer.** This round surfaced the gap honestly
   (new TODO row: `--health-check` coverage reporting) but shipped no consumer —
   application stays gated on the real-hardware soak, by design.
5. **Git history still entangled:** 6 more heuristic daemon commits this round, 0 commits
   by me (harness forbids without authorization). The verification work is reviewable as
   a set of gate results in this report, not as clean changesets.
6. **The 09:47 report's (f) is annotated, and (f50) asked for gates "after items 1–10"** —
   done and annotated; the report is now fully current as a snapshot, but it remains a
   point-in-time file: any future session must ANNOTATE it, never rewrite it.
7. **AGENTS module-map line counts remain `~` approximations.** I refreshed the real
   counts (test totals) but deliberately left module line-count approximations in place —
   they drift by design and a check would cost more than it protects.

---

## c) NOT STARTED

1. **v0.5.0 push** — blocked on maintainer go-ahead (tag exists locally at `00424ca`;
   SystemNix must be ready to pin). Unchanged from 09:47.
2. **Real-hardware soak test** — no live niri here (`$NIRI_SOCKET` unset); also awaiting
   the format decision (maintainer-driven / scripted harness / hybrid).
3. **Terminal ground truth (ROADMAP Q3)** — blocked on maintainer input. Unchanged.
4. **Applying captured geometry at restore** — deliberately gated on the soak test.
5. **The focus-steal fix itself** (final focus pass after all spawns settle) — the race is
   now *proven* and logged, but the fix needs an implement-vs-accept decision; it touches
   restore ordering, which is behavior.
6. **HARVEST of *this* report's section (f)** — the skill-mandated follow-up; deferred
   because the standing instruction for this round is "report, then WAIT".
7. **`nix flake check --all-systems` (aarch64)** — still open; every `nix flake check`
   run prints the reminder.
8. **`--retry-base-delay` rename decision** — now filed as a ROADMAP question; needs
   SystemNix coordination before any CLI change.
9. **Release flow: 0.5.1 vs 0.6.0** for the `[Unreleased]` section + Cargo.toml bump —
   TODO row added this round; needs a maintainer decision.
10. **Explicit per-task commits** — still awaiting authorization (see g).

---

## d) TOTALLY FUCKED UP!

1. **I fabricated a statistic through carelessness, then caught it myself.** "121 tests"
   in two docs was a `--list` count (includes the ignored benchmark) reported as
   "passing". Caught at final-gate recount, fixed in-session, nothing shipped false — but
   the verify-don't-trust rule applies to numbers I *generate*, not just numbers tools
   hand me. A reader who trusted the first draft would have been misled by me.
2. **I introduced the exact flake class this round existed to eliminate.** The new
   zero-delay test shipped with a `elapsed < 500ms` wall-clock assert and failed on its
   first full-suite run under parallel load — hours after the 09:47 report's section (d)
   declared "one green run is not a verification" the session's biggest lesson. Removed
   before it could become precedent; suite green ×3 afterwards. If the suite had been
   slightly faster that run, a latent flake would have entered the tree with my name on it.
3. **I broke `nix build` with the guard test.** I verified the new test via cargo only
   and did not think about the Nix source filter when the test reads a repo file by path —
   in this repo the cargo gate and the nix gate see *different file universes*, and I
   knew that. The guard did its job (that is why it exists), but I should have predicted
   the sandbox divergence when introducing a path-reading test, not been told by CI.
4. **Two wasted edit round-trips on "file modified since read" rejections** (fake_niri.rs,
   TODO_LIST.md) because my own `cargo fmt` and the daemon interleaved with my edits.
   This repo's AGENTS.md explicitly says to serialize writes and re-read after rejection;
   I paid the tax twice before switching to atomic scripted edits with grep verification.
5. **For the record — nothing is broken.** Final tree green on every gate; probe worktree
   removed; no user-visible regression; the count error and the timing assert never
   reached a reader or a commit beyond the daemon's.

---

## e) WHAT WE SHOULD IMPROVE!

### What did I forget?

- **A standing ban on wall-clock assertions in concurrency tests** — I reached for
  `Instant::elapsed` reflexively; nothing in AGENTS forbids it, and it cost a failed run.
- **The Nix sandbox sees different files.** Any new test that reads a repo path must be
  checked against `default.nix`'s filter, not just cargo.
- **What "count" means before writing it down.** `--list` counts, "passing" counts, and
  "ignored" counts are three different numbers; I conflated two.
- **Per-section dry-runs with the annotate tool.** I dry-ran section (b) as the shape
  check and then went straight at (c)/(f); it worked, but the skill's rule exists because
  sometimes it does not.
- **Stale `/tmp` scratch from the prior session** (`main_rs_backup.rs`, loop logs,
  flake-fix captures) — harmless, but I left them there and even added new ones
  (`nsm_*.log`, probe files in the removed worktree aside).

### What could I have done better?

- **Loop the probe from the start.** My first probe run was a single release-mode run
  (failed), then 8 debug runs passed 8/8 — a lazier reading stops at "debug-only" or
  "flaky, unreproducible". The 20×20 loop gave the honest 35–40% failure rate. The
  first single run was luck, not method.
- **Batch the count fixes with the original edits.** I had both numbers (list count and
  suite result) in the same session where I wrote "121"; the fix should never have been
  needed.
- **Reason about suite parallelism before running it.** A 500ms bound in a suite that
  runs 120 tests in parallel was fragile on paper; I did not need a failed run to know that.
- **Predict the two-gate divergence.** Introducing a path-reading test in a repo whose
  Nix build filters by extension should trigger a filter check as part of the same edit,
  not a red build.

### What could still improve (project-level)?

- **AGENTS: codify the two new rules** — no wall-clock bounds in concurrency tests;
  path-reading tests must be checked against the Nix source filter. (Both are in (f).)
- **Example-session guard could be stronger:** assert byte-identical round-trip
  (`to_string` of the parsed-then-reserialized model == file content) so formatting and
  field drift both fail. Cheap, strictly better.
- **A tiny scripts/ stress helper** (`loop-suite N`) to make the 5×-loop rule one
  command instead of a remembered ritual.
- **Suite diet:** the default retry-recovery test still sleeps the real 1s base delay;
  a 0-delay harness default for that one test would shave ~1s (~6%) off every suite run
  — at the cost of exercising the default constant, so it is a tradeoff, not a fix.
- **AGENTS module-map counts** — accepted drift for now; revisit only if a session
  actually trusts a `~` number and gets burned.

### Ghost systems / split brains status after this round

- **Closed:** `--help`/README/module.nix retry-delay semantics split brain; benchmark
  doc staleness; example-session drift hole (guarded); "pre-existing race" unproven
  claim (proven); nixfmt warning re-investigation loop (documented as accepted).
- **Open (deliberate):** geometry write-only until soak; `[Unreleased]` version
  decision; entangled git history.
- **No new ghost code introduced.** The two new tests assert real behavior; the
  `default.nix` exception ships one doc file into the package source, which is the
  guard's data, not dead weight.

---

## f) Up to 50 things we should get done next

Ordered by impact. Items 1–12 are the actionable cluster (several already have TODO_LIST
rows from this round's HARVEST — cited). Items 13+ are ROADMAP pointers and hygiene;
most were routed to ROADMAP in the harvest and are listed here for completeness, not as
commitments. **HARVEST of this list is pending your go-ahead (you said WAIT).**

1. **Decide the focus-race fix**: implement the final focus pass after all spawns settle,
   or document-and-accept the proven race until the soak test. (TODO row exists, evidence
   now empirical — 35–40% order-flip rate pre-change.)
2. **Soak test on real hardware** — the High-impact row; format decision needed first
   (see g2). TODO row exists.
3. **Push v0.5.0 + notify SystemNix** — blocked on you. TODO row exists.
4. **Terminal ground truth (Q3)** — blocked on you. TODO row exists.
5. **Authorize explicit per-task commits** (g) so future changesets are independently
   reviewable; the daemon entangled 6 more commits this round.
6. **HARVEST this report's (f)** into TODO_LIST/ROADMAP once you authorize continued work.
7. **AGENTS: ban wall-clock assertions in concurrency tests** (this round's d2 lesson).
8. **AGENTS/tests: rule for path-reading tests — check the Nix source filter in the same
   change** (this round's d3 lesson).
9. **CI: assert the test count** so a refactor cannot silently drop tests + record the
   ~16s timing budget. (TODO row exists; the module split proved the risk.)
10. **Implement `--health-check` geometry coverage** ("N of M windows carry layout") so
    captured v5 data stops being write-only. (TODO row exists.)
11. **Restore/save hardening tests batch**: unknown-future-keys load, corrupt-backup +
    valid-session interplay, marker pruning when `boot_id` is unreadable, `--dry-run`
    exit-code smoke in CI. (TODO row exists.)
12. **Stale-marker pruning when the session file vanishes.** (TODO row exists.)
13. Strengthen the example-session guard to byte-identical round-trip. (New this round.)
14. Release flow: decide 0.5.1 vs 0.6.0 for `[Unreleased]`, bump Cargo.toml. (TODO row exists.)
15. `scripts/` stress helper: `loop-suite N` for the 5×-loop rule. (New this round.)
16. Suite diet: 0-delay harness default for the retry-recovery test (tradeoff noted).
17. Re-annotate this report after the next work round (docs-health ANNOTATE, never rewrite).
18. Clean stale `/tmp` scratch files from both rounds.
19. `nix flake check --all-systems` (aarch64). (ROADMAP.)
20. macOS build-only CI job. (ROADMAP.)
21. Apply captured geometry at restore — decide resize vs column-index navigation; gated
    on soak. (ROADMAP.)
22. Watch niri upstream IPC for window-position application support. (ROADMAP.)
23. Multi-monitor soak scenarios (output rename/reorder). (ROADMAP.)
24. Event-flood safety valve in the save loop. (ROADMAP.)
25. Adaptive debounce beyond the fixed 2s. (ROADMAP, pre-existing.)
26. `--migrate` explicit session-format upgrade command. (ROADMAP.)
27. Config hot-reload via inotify. (ROADMAP.)
28. Per-app restore delay tuning. (ROADMAP.)
29. SSH suspend guard integration. (ROADMAP.)
30. journald log-volume review. (ROADMAP.)
31. `--print-config` support/debug mode. (ROADMAP.)
32. Per-window outcome summary log at restore end. (ROADMAP.)
33. Export/import scope: decide/document restore-marker inclusion. (ROADMAP.)
34. Backup compression / retention-policy review. (ROADMAP.)
35. `--retry-base-delay` rename decision (coordinate with SystemNix). (ROADMAP.)
36. Config TOML structural fuzzing. (ROADMAP.)
37. Broader harness window accessor. (ROADMAP.)
38. Doc-comment the remaining harness getters for consistency with `window_app_id`.
39. Restore-knowledge of focus across a multi-monitor focus history. (ROADMAP, pre-existing.)
40. Dry-run output for humans *and* machine diffing. (ROADMAP, pre-existing.)
41. systemd `Type=notify` readiness. (ROADMAP, pre-existing.)
42. IPC health/status endpoint beyond `--health-check`. (ROADMAP, pre-existing.)
43. Coverage reporting in CI. (ROADMAP, pre-existing.)
44. Duplicate-window dedup on save (distinct from single-instance dedup). (ROADMAP, pre-existing.)
45. Output matching by EDID/position (blocked: niri IPC exposes no output positions). (ROADMAP.)
46. Re-run the benchmark at the next behavioral change to the restore path (standing rule
    from restore-burst.md, not a new task).
47. Re-check the niri non-goals on the next major niri release (standing ROADMAP rule).
48. Watch treefmt-nix upstream for the nixfmt wrapper rename (warning disappears upstream;
    no in-repo action).
49. Consider dropping AGENTS' `~` module line-counts if they ever mislead a session.
50. After items 1–12: re-run all gates and ANNOTATE this report with outcomes.

---

## g) Ask me up to 3 questions you CANNOT figure out yourself

1. **The focus race is now proven — fix it or accept it?** Implementing a final focus
   pass after all spawns settle is low-effort but changes restore ordering (behavior);
   accepting means documenting it until the soak test. Which way?
2. **How should the real-hardware soak run?** (a) you drive the daily driver with a
   checklist + log-watching commands from me, (b) I write a scripted soak harness you
   launch, (c) hybrid. Acceptance criteria depend on how disruptive it may be.
3. **Push v0.5.0 now?** Tag exists locally (`00424ca`), tree green on all gates; push
   awaits your go-ahead and SystemNix's readiness to pin. (Same breath: authorize
   explicit per-task commits, and future history stays reviewable.)

---

_Report written 2026-09-15 10:42 CEST. Point-in-time snapshot of the verification-debt +
HARVEST round; state described was verified on the final tree (daemon commit `bf36cd1`
at write time). Nothing committed by me — the auto-commit daemon owns the history._
