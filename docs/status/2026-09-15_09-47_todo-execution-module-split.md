# Status Report — 2026-09-15 09:47 CEST

> TODO-list execution session: four open items implemented, documented, and verified; one
> flaky test my own change introduced was caught and fixed before close-out.
>
> **Scope:** based only on this session's run. No unrelated research.
> **Format note:** written as Markdown at the user's explicit request (both the status-report
> and brutal-self-review skills default to styled HTML); the repo's own `docs/status/*.md`
> convention matches. The self-review questions were folded into sections (d)/(e) instead of
> a separate `docs/reviews/` file.

---

## Session at a glance

| Metric                     | Value                                                                     |
| -------------------------- | ------------------------------------------------------------------------- |
| TODO items executed        | 4 of 4 actionable (2 Medium, 2 Low)                                       |
| Files touched              | 10 Rust sources + 8 docs + Cargo.toml + module.nix + example-session.json |
| Test suite                 | 118 passed + 1 ignored (was 114) — green on 4 separate full runs          |
| New tests added            | 4 (retry backoff, layout capture e2e, v4-file compat, event relevance)    |
| Flaky test found + fixed   | 1 (`focus_is_restored_for_the_saved_focused_window`), 13/13 after the fix |
| Verification failures seen | 3 (one flaky test, one fmt drift, two self-inflicted build breaks)        |
| Git commits by me          | 0 (auto-commit daemon; 12 heuristic commits in 6h — changesets entangled) |

---

## a) FULLY DONE

### 1. Spawn-timeout exponential backoff (TODO item 7)

- `--retry-delay` is now the **base** delay; each failed restore attempt doubles it, capped at
  30s — mirroring the event-stream reconnect backoff. Implemented as `next_retry_delay`
  (`src/restore.rs`) with saturating arithmetic; a base of 0 still retries immediately.
- Unit test `retry_backoff_starts_at_the_base_and_caps` covers base growth, the cap as a fixed
  point, and the zero-base case.
- Docs: README CLI table, `module.nix` option description, AGENTS.md config surface.

### 2. Blocking niri I/O off the async runtime (TODO item 4)

- `spawn_single_window` and `apply_window_placement` now route through `niri_send`
  (`spawn_blocking`) instead of issuing inline `Socket::connect().send()` on the runtime — the
  exact serialization the TODO described ("a current-thread runtime serializes all spawns
  behind it").
- `apply_window_placement` became async; both call sites await it.
- Side benefit: `Reply::Err` on a move is now surfaced as a warning where it was previously
  swallowed silently.
- Regression recorded: AGENTS.md gained the invariant **"Blocking niri IPC happens only inside
  `niri_send`"**, and the resolved Known-Issues entry was removed.

### 3. Window layout capture, session format v5 (TODO item 6)

- **Verified the upstream claim first** (pinned `niri-ipc 25.11.0` source, `Cargo.lock`):
  `Window.layout` is _not_ `Option<WindowLayout>` (lib.rs:1301), and its real shape is
  `pos_in_scrolling_layout: Option<(usize, usize)>`, `tile_size: (f64, f64)`,
  `window_size: (i32, i32)`, `tile_pos_in_workspace_view`, `window_offset_in_tile` — richer
  and different from what the TODO text assumed.
- Model: `SavedWindowLayout { scroll_position: Option<ScrollPosition{column, tile_in_column}>,
  tile_width, tile_height }` in `src/session.rs`; captured in `capture_session_json` via
  `SavedWindowLayout::from_niri`. Viewport-relative and Wayland-internal fields are deliberately
  dropped (durable restore-relevant data only).
- `SESSION_FORMAT_VERSION = 5`; pre-v5 files load untouched (`#[serde(default)]` +
  `skip_serializing_if`).
- Tests: `capture_records_window_layout_in_the_session_file` (end-to-end through the fake IPC
  server), `v4_session_without_layout_still_loads_with_layout_none`,
  `layout_relevant_covers_geometry_changes`, plus `arb_window_layout` wired into the round-trip
  property tests.
- `docs/example-session.json` rewritten to v5 (jq-validated).
- **Found and fixed a real format bug while doing this:** serde_json's default float parser is
  off-by-1-ULP lossy for some `f64`s. The new property tests failed on
  `1801.8231164676984` vs `…86`, so `serde_json` now enables `float_roundtrip` (Cargo.toml).
  Without that, every session file carrying tile sizes would silently drift.

### 4. `src/main.rs` module split, behavior-frozen (TODO item 5)

- 3.7k-line `main.rs` → `main` (184), `ipc` (38), `session` (569), `terminal` (203), `config`
  (284), `restore` (651), `save` (315), plus a dedicated `tests.rs` (1593); `proc`/`fake_niri`
  unchanged.
- Behavior-frozen proof: function-name diff of the moved test body (**101 = 101, zero diff**),
  and the full suite passes at the same 118 + 1 count before and after.
- AGENTS.md architecture section rewritten as a module map; stale `src/main.rs` references fixed
  in CONTRIBUTING.md and ROADMAP.md.
- **Not achieved:** the TODO asked for this as "a dedicated behavior-frozen changeset". I
  sequenced it last, but I cannot commit — the auto-commit daemon's heuristic commits entangled
  items 7/4/6/5. See (b)1 and (g).

### 5. Flaky test root-caused, fixed, stress-verified

- `focus_is_restored_for_the_saved_focused_window` asserted `focus_actions == vec![2]` — an id
  that only holds when spawn requests arrive in saved order. Pre-change, inline blocking I/O on
  a current-thread test runtime _accidentally_ serialized them. My spawn_blocking change removed
  that accident and the test became order-dependent (failed 1 of 4 runs).
- Fixed to assert the order-independent truth: exactly one focus action, and it targets the
  window whose `app_id == "chromium"` — via a new documented harness getter
  `FakeNiri::window_app_id`. 13/13 stress runs green.
- The latent product-level race it exposed (a later spawn can steal restored focus) is logged in
  TODO_LIST as a bounded Low item — see (b)6.

### 6. Documentation sync (docs-health BUILD/VERIFY discipline)

- `CHANGELOG.md`: new `[Unreleased]` section (2 Added, 3 Changed) — the session's only surviving
  change log, since the done TODO rows are deleted, not re-labelled.
- `TODO_LIST.md`: 4 done rows removed, 1 new row added (focus race), evidence paths repointed
  (`reactive_save_session` → `src/save.rs`), footer re-verified against 118 tests.
- `FEATURES.md`: session-format row v4→v5; "Window size / column-width capture" ⚪ PLANNED →
  🟢 FULLY_FUNCTIONAL.
- `README.md`: v5 format paragraph, new layout-capture feature bullet, retry-delay semantics.
- `AGENTS.md`: module map, new IPC invariant, session-format/float_roundtrip notes, testing
  notes (tests.rs, spawn_blocking, 118), resolved Known-Issue removed, stale path generalized.
- `ROADMAP.md`: split + backoff marked done inline; `plan_spawns` path corrected.
- `docs/example-session.json`: v5 with layout on all three windows.

### 7. Verification actually executed (green, on the final tree)

| Gate                                       | Result                          |
| ------------------------------------------ | ------------------------------- |
| `cargo test` ×4 (2 final, after flake fix) | 118 passed, 1 ignored, 0 failed |
| Focus test stress (13 runs)                | 13/13 pass                      |
| `cargo clippy --all-features`              | 0 errors, 0 warnings (CI form)  |
| `cargo fmt --all -- --check`               | clean                           |
| `nix build`                                | built                           |
| `nix flake check`                          | all checks passed               |
| `bash scripts/docs-citations.sh`           | all citations and links resolve |
| `jq` on `docs/example-session.json`        | v5 shape valid                  |

---

## b) PARTIALLY DONE

1. **The module split's "own changeset" requirement — git history does not show it.** The TODO's
   explicit demand was a reviewable, isolated split. In the working tree it _is_ isolated (done
   last, no logic changes), but the daemon's 12 heuristic commits in 6 hours swallowed all four
   changesets into one blob. Fixing this needs your explicit commit authorization (see g-3).
2. **Captured geometry is write-only data.** v5 records layout, but nothing reads it: restore
   doesn't apply it, `--health-check` doesn't report it, export/import just copy it. Deliberate
   (application is gated on the soak test), but until then it is data with no consumer — the
   ghost-data risk this repo's own philosophy warns about.
3. **AGENTS.md module-map line counts are approximations** (`~185`, `~570`, …). They were exact
   at write time and will drift on the next edit.
4. ~~**`--retry-delay` now lies slightly.** Its semantics changed to "base delay" but tunable CLI~~ done (--help text added to the five undocumented tunables (clap doc comments); verified in --help output)
   ~~flags have no `--help` text at all, so `--help` is silent; only README/module.nix state the~~
   ~~exponential behavior. A documentation split brain, and arguably a naming lie.~~
5. ~~**`--retry-delay 0` is unit-tested only** — no integration test exercises the immediate-retry~~ done (zero_retry_delay_retries_immediately_after_a_failed_spawn covers the immediate-retry path end-to-end)
   ~~path through the retry loop.~~
6. ~~**The focus-steal race is "pre-existing" by reasoning, not by proof.** I did not run the~~ done (PROVEN 2026-09-15 at 8d2386b — old id-order assertion failed 8/20 (release) and 7/20 (debug) runs on a multi_thread runtime)
   ~~pre-change tree (worktree at an older commit) to demonstrate the race exists there too. The~~
   ~~claim is plausible (production ran multi-thread runtimes, where the race can occur) but~~
   ~~unverified — exactly the kind of claim this repo's `verify-external-claims` discipline exists~~
   ~~for.~~
7. ~~**`docs/example-session.json` has no guard.** jq validates the JSON and I hand-checked the~~ done (example_session_doc_deserializes now guards the doc against serde drift)
   ~~shape, but no test or script deserializes it — it can drift from `serde` silently. (AGENTS~~
   ~~lists it under the docs map without an owner for its validity.)~~
8. ~~**Benchmark doc not re-run.** The ignored `restore_burst` benchmark measures restore~~ done (re-run unchanged (100.4 ms/window); restore-burst.md carries the dated result)
   ~~throughput; the spawn path changed materially (spawn_blocking). `docs/benchmarks/restore-burst.md`~~
   ~~numbers may now be stale.~~
9. ~~**Nix evaluation warning observed, not recorded.** `nix flake check` emits~~ done (recorded in AGENTS.md known-issues)
   ~~"nixfmt-rfc-style is now the same as pkgs.nixfmt" — judged an upstream/treefmt-nix~~
   ~~deprecation (the repo's `flake.nix` already uses the current `nixfmt.enable`), but it is not~~
   ~~written into AGENTS.md known-issues, so the next session will re-investigate it.~~
10. ~~**CI-parity checks not run locally.** `markdownlint`, `deadnix`, `statix check`, `cargo-deny`,~~ done (local CI-parity green — nixfmt --check, deadnix, statix check, cargo-deny (advisories/licenses/bans/sources))
    ~~`cargo-audit` were not run this session (they live in the devshell / CI). The docs I edited~~
    ~~could trip markdownlint rules that I did not exercise.~~

---

## c) NOT STARTED

1. **v0.5.0 tag push** — blocked on maintainer go-ahead (I never push unprompted).
2. **Real-hardware soak test** — reactive saves + idempotent restore on the daily driver; no live
   niri in this environment (`$NIRI_SOCKET` unset, no `niri` process).
3. **Terminal ground truth (ROADMAP Q3)** — which terminal profiles get must-not-regress status;
   blocked on maintainer input.
4. **Applying captured geometry at restore** — deliberately gated on the soak test.
5. ~~**HARVEST of this report's section (f) into TODO_LIST/ROADMAP** — the skill-mandated follow-up;~~ done (HARVEST executed 2026-09-15 into TODO_LIST.md and ROADMAP.md)
   ~~not done because you asked me to wait for instructions after the report.~~
6. **`nix flake check --all-systems` (aarch64)** — pre-existing ROADMAP item, untouched.
7. **Explicit per-task commits** — needs authorization; nothing was committed by me.
8. ~~**`--help` text for the tunables** — never existed for `--retry-delay`/`--spawn-timeout`/etc.~~ done (--help now documents all tunables (output verified))
9. ~~**markdownlint / deadnix / statix / cargo-deny local runs** — see (b)10.~~ done (nixfmt, deadnix, statix, cargo-deny all green locally 2026-09-15)
10. ~~**Benchmark re-run** — see (b)8.~~ done (benchmark re-run 2026-09-15, unchanged (100.4 ms/window))

---

## d) TOTALLY FUCKED UP!

I am not going to soften this: **my verification standard was too weak for a concurrency change,
and I nearly ended the session declaring a false green.**

1. **I shipped a latent flake and called item 4 "done".** After replacing the spawn I/O substrate
   I ran the suite, saw 118 passed, and moved on. The very next full run failed
   (`focus_is_restored_for_the_saved_focused_window`). My change did not _create_ the
   order-dependence, but it _removed the accident that hid it_ — which means I owed that test an
   inspection **before** touching the spawn path, and owed the suite repeated runs (16s each!) to
   declare done. The repo's own AGENTS.md session-rules list warns precisely against trusting a
   single green result; I read that file at session start and still did it. This is the single
   biggest miss of the session.
2. **I broke the build twice with blind bulk sed sweeps.** First `pub(crate)` onto trait-impl
   methods (`Default::default`, `Display::fmt`) — illegal; then the struct-field sweep matched
   _wrapped function-signature parameters_ in `restore.rs`/`save.rs`. Each cost a build cycle and
   manual line-by-line repair. The fix was a dry-run plus targeted line addresses; I did neither
   up front.
3. **I claimed "Done — all four items executed and verified" when fmt had already drifted.** I ran
   `cargo fmt --all`, then ran several seds that shortened lines, and the formatting went stale
   again — caught only in the final sweep. Same class as #1: a "verified" claim not backed by a
   check at claim time.
4. **Nothing is broken in the shipped tree.** For the record: no user-visible damage, no incorrect
   data written, no test left red. Final tree is green on every gate (a7). The fuckups are process
   failures with finite cost — but #1 is the kind that erodes the value of every "done" I report.

---

## e) WHAT WE SHOULD IMPROVE!

### Answering your three questions head-on

**What did you forget?**

- A guard test for `docs/example-session.json` — should have been part of item 6 ("extend the
  property tests").
- Re-running the ignored benchmark after changing the spawn path; the benchmark doc is now
  possibly stale.
- `--help` text for the tunables, so the retry-delay semantic change is visible where users look
  first.
- Local CI-parity checks (markdownlint/deadnix/statix/cargo-deny) after touching docs/manifests.
- Proving the "pre-existing race" claim instead of reasoning about it.
- Recording the nixfmt deprecation warning somewhere durable.
- The HARVEST follow-up that the status-report skill explicitly mandates.
- Running the suite enough times for a suite containing 2s-debounce, 500ms-poll, and
  arrival-order-sensitive tests: **once is not a verification**.
- Checking whether `apply_window_placement`'s new `Reply::Err`-to-warning path changed any
  documented behavior (I reasoned it, didn't test it explicitly).

**What could you have done better?**

- **Loop the suite ≥5×** (~80s total) before marking any concurrency-affecting change done.
- **Read the ordering-sensitive tests first.** Grep the harness for id/order assertions before
  changing anything that can reorder IPC arrivals.
- **Dry-run every bulk edit** (print the matched lines) and follow with `cargo build` in the same
  command — I followed the repo rule for _markers_ but not for _syntax_.
- **Stage the four changesets as separate explicit commits** — the only way the module split is
  independently reviewable, as the TODO demanded. This needs your authorization; the daemon will
  not do it for you.
- **Verify rather than reason** for any "pre-existing"/"upstream"/"not mine" classification
  (the focus race, the nixfmt warning).
- **Add the doc-artifact test while touching the format** instead of leaving an unguarded example.

**What could you still improve?**

- Add `example_session_doc_deserializes()` (cheap, closes a real drift hole).
- Add `#[arg(help = …)]` to the six tunables; while there, decide whether `--retry-delay` should
  be renamed to `--retry-base-delay` (a SystemNix-visible breaking rename — coordinate).
- Re-run and refresh the benchmark doc; add its numbers to CI or mark them dated.
- Make the fixture-harness rule explicit in AGENTS: _never assert spawn arrival order or spawned
  window ids; assert app identity_ (now possible via `window_app_id`).
- Audit `fake_niri.rs` for any other latent arrival-order coupling (one was caught by luck).
- Propose the focus-steal fix (final focus pass after all spawns settle) and decide with you
  whether to implement or document-and-accept.
- Surface v5 geometry somewhere (health check or a summary log) while application is gated, so the
  data is not write-only.
- Watch AGENTS.md growth: the module map will drift; either drop the counts or add a check.
- Add a timing-budget note/guard for the suite (~16s today; sleeps are the risk).

### Ghost systems / split brains / legacy found this session

- **Ghost data:** captured `layout` has no consumer yet (b2) — intentional, but must not be
  forgotten at the soak-test review.
- **Split brain (docs):** `--retry-delay` semantics live only in README/module.nix, not in
  `--help` (b4).
- **Split brain (verification):** CI's clippy gate excludes tests by design, while `--all-targets`
  reports ~200 test-only findings — documented in AGENTS, but it means "clippy clean" means less
  than it sounds; the canonical command is the contract, and I used it.
- **No new legacy code introduced.** The split strictly reduced file sizes and left no re-export
  shims (paths were fixed properly instead).

---

## f) Up to 50 things we should get done next

Ordered roughly by impact within each group. Everything unverified is marked; items 1–10 are the
ones I would do first.

### Verification debt created this session

1. ~~**Loop the suite 5× and record the result** — establish that today's 118 are stable, not just~~ done (5 consecutive green runs 2026-09-15 (118 each; 120 after the new tests))
   ~~green once. (Low effort)~~
2. ~~**Audit `fake_niri.rs` for arrival-order coupling** beyond the one fixed test. (Low)~~ done (audit clean — no order-coupled assertions beyond the fixed focus test; rule recorded in AGENTS)
3. ~~**Prove or retract the "focus race is pre-existing" claim** — run a worktree at a pre-change~~ done (proven at 8d2386b — 8/20 release, 7/20 debug failures on a multi_thread runtime)
   ~~commit and try to trigger it. (Low–Medium)~~
4. ~~**`example_session_doc_deserializes()` test** over `docs/example-session.json`. (Low)~~ done (example_session_doc_deserializes added)
5. ~~**Re-run the ignored benchmark** and refresh `docs/benchmarks/restore-burst.md`. (Low)~~ done (re-run unchanged (100.4 ms/window); restore-burst.md dated)
6. ~~**Run markdownlint / deadnix / statix / cargo-deny / cargo-audit locally** (devshell) to match~~ done (nixfmt, deadnix, statix, cargo-deny green locally (markdownlint is not a CI step; advisories covered by cargo-deny))
   ~~CI before the next "done". (Low)~~
7. ~~**Add `#[arg(help = …)]` to all six tunables**, stating the exponential retry semantics. (Low)~~ done (done as clap doc comments on the five undocumented tunables; --help output verified)
8. ~~**Record the nixfmt-rfc-style deprecation** in AGENTS known-issues (upstream; accepted). (Low)~~ done (in AGENTS.md known-issues)
9. ~~**Add the harness rule to AGENTS**: never assert spawn arrival order or spawned ids. (Low)~~ done (in AGENTS.md testing section)
10. ~~**Test the `--retry-delay 0` immediate-retry path end-to-end.** (Low)~~ done (zero_retry_delay_retries_immediately_after_a_failed_spawn)

### Product work (real features / fixes)

11. **Soak-test harness**: scripted checklist or helper for real-hardware reactive-save
    verification (blocked on your input for how it should run).
12. **Final focus pass** after all spawns settle — fixes the logged focus-steal race. (Low)
13. **Apply captured v5 geometry at restore** — decide resize vs. column-index navigation first.
    (Medium, gated on soak)
14. **Terminal ground truth**: mark must-not-regress profiles. (blocked on your input)
15. **`--retry-base-delay` rename** (or documented acceptance of the current name) before SystemNix
    pins behavior. (Low, consumer-visible)
16. **Surface geometry coverage** in `--health-check` (e.g. "N of M windows carry layout"). (Low)
17. **`--migrate` command** to explicitly rewrite session files to the newest format. (Low–Medium)
18. **Config hot-reload via inotify** (ROADMAP). (Medium)
19. **Per-app restore delay tuning** (ROADMAP). (Medium)
20. **SSH suspend guard integration** (ROADMAP). (Medium)
21. **journald log-volume review** — per-window restore lines can be noisy. (Low)
22. **Cross-platform CI job** (macOS build-only; proc module is linux-gated). (Medium)
23. **`nix flake check --all-systems`** (aarch64). (Low)
24. **Debounce constant exposure/config** (`SAVE_DEBOUNCE_SECS = 2` is hard-coded). (Low)
25. **Event-flood safety valve** in the save loop (cap saves/sec under churn). (Medium)
26. **Export/import scope decision**: include the restore marker? document what is excluded. (Low)
27. **Backup compression** or retention policy review. (Low)
28. **Session-dir permission documentation** (`$XDG_DATA_HOME/niri-session-manager`). (Low)
29. **`--print-config` mode** for support/debugging. (Low)
30. **Per-window outcome summary log** at restore end. (Low)
31. **Stale-marker pruning when the session file vanishes** — currently only boot-id-based. (Low)
32. **Multi-monitor soak**: output rename/reorder scenarios. (Medium)
33. **Watch niri upstream IPC** for window-position application support. (Low, ongoing)
34. **Forward-compat property test**: session JSON with unknown future keys still loads. (Low)
35. **Config TOML structural fuzzing** beyond arbitrary-input no-panic. (Low)
36. **Corrupt-backup + valid-session interplay test.** (Low)
37. **Marker-pruning test when `boot_id` is unreadable.** (Low)
38. **Extend the `--version` CI smoke** to a `--dry-run` no-op exit-code assertion. (Low)
39. ~~**CONTRIBUTING: document where tests live** in the post-split layout. (Low)~~ done (src/tests.rs is now named as the unit-test home)
40. **ROADMAP dedupe pass** — done items keep accumulating in the ideas list. (Low)
41. **AGENTS module-map drift**: drop the `~` counts or add a cheap check. (Low)
42. **CHANGELOG release flow**: decide 0.5.1 vs 0.6.0 for `[Unreleased]` and bump Cargo.toml. (Low)
43. **Push v0.5.0** and notify SystemNix to pin. (blocked on you)
44. **Per-task explicit commits** going forward (needs your authorization). (Low)
45. **Doc-comment the remaining harness getters** for consistency with `window_app_id`. (Low)
46. **Consider a broader harness window-accessor** if more tests need identity lookups. (Low)
47. **Suite timing budget** note/guard (~16s; sleeps are the failure mode). (Low)
48. **Test-count assertion in CI** (fail if tests are silently dropped by a refactor — the split
    proved this is a real risk: fn-diff was manual this time). (Low)
49. ~~**HARVEST this report's list** into TODO_LIST/ROADMAP (skill-mandated; most of 18–47 are~~ done (executed 2026-09-15 — bounded items to TODO_LIST.md, ideas to ROADMAP.md, resolved items dropped)
    ~~ROADMAP fuel, not commitments). (Low)~~
50. ~~**Re-run this report's gates after items 1–10** and annotate it (docs-health ANNOTATE mode)~~ done (gates re-run 2026-09-15 — 120 passed x3, clippy clean, fmt clean, nix build, nix flake check, docs-citations; annotated inline)
    ~~rather than rewriting it. (Low)~~

---

## g) Ask me up to 3 questions you CANNOT figure out yourself

1. **Push v0.5.0 now?** The tag exists locally (`00424ca`, `v0.5.0`) and the tree is green, but
   I never push unprompted, and SystemNix has to be ready to pin it. Push, or hold?
2. **How should the real-hardware soak test run?** Options: (a) you drive the daily driver
   interactively while I prepare a checklist + log-watching commands, (b) I write a scripted soak
   harness (e.g. scripted window churn + assertions over the journal) that you launch, or (c) a
   hybrid. I cannot design the acceptance criteria without knowing how disruptive the soak can be
   on your working session.
3. **Which terminals do you actually run daily?** (ROADMAP Q3) — that subset becomes
   must-not-regress; I cannot infer it from the repo or this machine.

**One decision I would like in the same breath:** do you want me to make **explicit per-task
commits** when authorized? The module split's "reviewable on its own" requirement is currently
unmet in git history because the auto-commit daemon batches everything into heuristic commits.
With a one-line authorization I can commit per changeset for future work (and can split the
current blob if you want it reconstructed).

---

_Report written 2026-09-15 09:47 CEST. Point-in-time snapshot; state it describes was verified at
`ee659b5` + one uncommitted `TODO_LIST.md` edit. Nothing committed by me — the auto-commit daemon
owns the history._
