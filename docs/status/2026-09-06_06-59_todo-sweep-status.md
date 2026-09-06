# Status Report: TODO Sweep + Reactive Save-Loop Hardening

**Point-in-time:** 2026-09-06 06:59 CEST · HEAD `24c164b` (auto-commit daemon active; a
second session is committing in parallel — see "Environment notes")
**Scope of this report:** the session that executed the previous TODO_LIST's
actionable items (reactive save loop, event-stream shutdown/backoff, harness
tests, CI cache, upstream watch, living-docs sync). Historical evidence only —
not current truth.

**Verification snapshot at report time:** `cargo build` clean · `cargo test`
114 passed + 1 ignored benchmark (was 108) · `cargo clippy --all-features`
clean · `cargo fmt --all -- --check` clean · `bash scripts/docs-citations.sh`
all resolve · `cargo run -- --version` OK · `nix flake check` all checks passed.

---

## a) FULLY DONE

| Work | Evidence |
| ---- | -------- |
| Graceful shutdown for the event-stream reader — root cause turned out to be worse than "sloppy": on an idle desktop, SIGTERM left the `spawn_blocking` reader blocked on the socket, and tokio's runtime drop waits for blocking tasks, so exit could hang until systemd SIGKILL. Fixed at the root: `EventConnection` owns the socket plus a `try_clone`d shutdown handle; the drive loop shuts the socket down on exit; shutdown flows through a `watch` channel with a 5s grace and abort as deadline fallback | `EventConnection`, `drive_event_driven_saves`, `shutdown_with_final_save` in `src/main.rs`; test `shutdown_signal_unblocks_the_parked_event_reader` |
| Capped exponential reconnect backoff: 1s doubling to a 30s cap; a stream alive ≥5s resets it (one niri restart cannot poison later reconnects) | `RECONNECT_*` consts + `next_reconnect_delay`; unit test `reconnect_backoff_doubles_and_caps` |
| Polling-fallback branch made testable: injectable interval via `run_reactive_save_session` (prod wrapper keeps the 60s-min `save_interval` math); the fallback's accepted probe subscription is now *used* instead of discarded | `run_reactive_save_session`; test `polling_fallback_saves_when_event_stream_refused_then_recovers` (refusal injection → fallback saves → recovery onto the same stream, asserted `event_stream_connections() == 1`) |
| Fake niri server upgraded: `refuse_event_streams(n)` failure injection, per-app in-flight concurrency metering, accepted-stream counter | `src/fake_niri.rs` (`refuse_event_streams`, `max_concurrent_spawns_per_app`, `event_stream_connections`) |
| Same-app spawn sequencing test: editors strictly sequential (per-app max in-flight == 1) while different apps genuinely overlap (global max ≥ 2, proving the assertion non-vacuous) | `same_app_spawns_are_strictly_sequential_while_apps_overlap` |
| Window-closed-between-restores test: closing one of two same-app windows respawns exactly the deficit, placed back on its saved workspace | `window_closed_between_restores_respawns_exactly_it_on_its_workspace` |
| `--save-only` end-to-end test: `run_service_loop` extracted from `main` with an injectable shutdown signal; asserts no restore spawns, no marker write, final save snapshots the live desktop | `run_service_loop` + `save_only_skips_boot_restore_and_runs_the_save_loop` |
| CI cargo caching: `Swatinem/rust-cache` pinned by commit SHA (v2.9.2, `6323deb102c322ba6fcbdcafc7e3dddab59af2b6`), placed after the nightly install so the cache key includes rustc version | `.github/workflows/checks.yml` |
| niri upstream watch RESOLVED: window geometry IPC **landed upstream** — `niri_ipc::Window.layout` (`WindowLayout`: tile/window size, scroll-layout position) since v25.08; verified against the resolved 25.11 crate source on disk, not just release notes | ROADMAP idea + non-goal updated with evidence; TODO_LIST now carries the follow-up feature |
| Living docs synced: TODO_LIST rewritten (8 items resolved this round), CHANGELOG `[Unreleased]` extended, ROADMAP (2 ideas shipped, 1 non-goal superseded), FEATURES (fallback row → FULLY_FUNCTIONAL, 2 new rows, footer), AGENTS.md (counts, architecture flow, 4 new testing gotchas, known-issues refresh) | the five docs; `docs-citations.sh` green after the edits |
| Full verification battery at session end | see snapshot above |

## b) PARTIALLY DONE

- **Graceful-shutdown path is the norm, but the abort deadline path still leaks
  the parked reader** if the save task wedges >5s (abort skips the socket-shutdown
  cleanup). Documented in AGENTS Known Issues; acceptable last-resort, not fixed.
- **Reconnect/backoff testing is shallow by design of the fake**: the fake server
  cannot kill a live event stream (no EOF/restart injection), so the
  *stream-death* half of the loop (backoff after healthy-stream death, reset
  logic in situ) has no integration test — only the pure `next_reconnect_delay`
  function is unit-tested. The fallback+refusal path is well covered; the death
  path is not.
- **Docs phasing**: the shutdown-hang fix is logged under CHANGELOG `Added`;
  arguably belongs in `Fixed`. Cosmetic.
- **README** not updated: graceful shutdown (no more 90s SIGKILL waits on idle
  desktops) is user-visible under systemd and worth a line.
- **main.rs split**: deliberate call made (yes — but as a dedicated
  behavior-frozen changeset). Decided, not executed; documented in TODO_LIST.
- **Parallel session's leftovers absorbed**: jscpd annotations, license
  metadata (`GPL-3.0-only`, `meta.license`), lychee/markdownlint config landed
  mid-session from a concurrent session; combined tree verified green. Not my
  work, not fully reviewed line-by-line.

## c) NOT STARTED (remains open, verified against code today)

- **Cut release 0.5.0** — BLOCKED on maintainer go-ahead (tag/push).
- **Real-hardware soak test** of reactive saves + idempotent restore — needs
  the daily driver; the fake cannot prove niri-event timing.
- **Terminal ground truth (ROADMAP Q3)** — needs maintainer input on daily
  drivers.
- **`spawn_single_window` blocking IPC on the async runtime** — new finding this
  session (it serializes all spawns on current-thread runtimes; fine on the
  production multi-thread runtime). Logged, not fixed.
- **WindowLayout capture (session-format v5)** — unblocked upstream, needs
  design. Not started.
- **Spawn-timeout exponential backoff** in the restore retry loop (ROADMAP).
- **Benchmark re-run**: `restore_burst` was not re-executed this session
  (restore path untouched, but the claim "no perf regression" is asserted from
  reasoning, not measurement).

## d) TOTALLY FUCKED UP

Nothing shipped broken — final tree is green across all seven gates — but three
design/implementation flaws were introduced and caught **this close** to
shipping, which is too close:

1. **Backoff reset logic was self-defeating**: the first version reset
   `reconnect_delay` to the initial value after every successful subscribe,
   which erased the doubling before it was ever read — behaviorally identical
   to the old fixed 1s. Caught only by a rustc `unused_assignments` *warning*,
   which nothing denies. If I hadn't manually grepped warnings, a silent no-op
   "feature" would have shipped with a green test suite.
2. **`JoinHandle` double-poll panic**: `timeout(grace, &mut handle)` followed by
   `save_task.await` panics ("JoinHandle polled after completion"). Caught by
   the existing shutdown test — the safety net worked, but I wrote it wrong
   first.
3. **Discarded-probe event race**: the initial fallback design dropped the
   accepted probe subscription and reconnected, which would let the fake's
   push-loop swallow queued events and flake the new test. Caught in
   self-review *before* running tests.

Plus honest friction: 6 clippy errors introduced (all fixed same session) and
2 rejected edit batches from racing the auto-commit daemon (the edit tool's
freshness tracker does not count bash reads as "reading the file"; one rejection
was actually my own `cargo fmt` touching the file mid-edit). No data lost, but
the write discipline cost round trips.

## e) WHAT WE SHOULD IMPROVE

**Process**

- Run `cargo clippy --all-features` after every behavioral edit batch, not just
  at the end — 6 pedantic violations accumulated before the first clippy run.
- Gate on rustc warnings locally (`cargo build 2>&1 | rg '^warning'` or
  `RUSTFLAGS=-Dwarnings` on a pinned toolchain). Warning-only flaws (see d1)
  survive otherwise. CI does not fail on warnings today.
- When a "simple" refactor touches async shutdown semantics, write the
  regression test *first* — the double-poll bug existed only between edit and
  test run.
- Remember the edit-tool freshness rule in this repo: the daemon (and `cargo
  fmt`) rewrite files under you; always `view` immediately before `edit`, and
  grep-assert markers after batched edits (this session's AGENTS rule held up).

**Code/test architecture**

- The fake server needs a **stream-death injection** (`kill_event_streams()` /
  EOF) — the reconnect half of the save loop is the least-tested code we ship.
- Test code is **lint-dark in CI** (`cargo clippy --all-features` without
  `--tests`). Add a `clippy.toml` (`allow-unwrap-in-tests`,
  `allow-expect-in-tests`, `allow-panic-in-tests`) and a
  `cargo clippy --all-features --tests` CI step so harness code gets linted.
- The hand-rolled JSON-line framing (`request_reply`/`event_reader`) duplicates
  niri-ipc internals because `Socket` never exposes its fd. Cleanest long-term
  fix is upstream (expose fd or accept a stream), not a growing local fork.
- `watch<bool>` works as a shutdown token, but a dedicated type
  (`CancellationToken` via tokio-util, or a small newtype) would make shutdown
  signatures self-documenting. Low priority.
- The healthy-stream threshold (5s) and grace (5s) are magic consts with no
  end-to-end test pinning their behavior.

**Docs**

- Add README line for the shutdown improvement; move the CHANGELOG entry to
  `Fixed` when the release notes are tidied.
- Prose line-counts in AGENTS (`~3600 lines`) drift immediately; consider
  dropping counts or letting the citations script own them.

## f) UP TO 50 THINGS TO GET DONE NEXT

Prioritized; grouped; each is bounded.

**Release & validation (P0)**
1. Get maintainer go-ahead, then cut **release 0.5.0** (tag + push; SystemNix pins it).
2. Tidy `[Unreleased]` before cutting: move shutdown-hang entry to `Fixed`; update `Cargo.toml` version.
3. Verify the **CI run with rust-cache** goes green (first real run proves the new step + SHA pin).
4. Add `actionlint` (or `nix run nixpkgs#actionlint`) for `checks.yml` syntax/lint.
5. Re-run `restore_burst` benchmark; refresh `docs/benchmarks/restore-burst.md` numbers post-changes.
6. Real-hardware **soak test**: reactive saves + idempotent restore as the daily driver for a week (user-run; provide a checklist).
7. Smoke the systemd stop path on hardware: SIGTERM on an idle desktop must exit < ~6s (proves the graceful shutdown fix where it matters).

**Test coverage gaps found this session (P1)**
8. Fake server: **stream-death injection** (drop/EOF the event connection on demand).
9. Test: reconnect after stream death — backoff doubles across consecutive quick deaths (8).
10. Test: healthy stream (≥5s) resets backoff to 1s (8).
11. Test: shutdown via **sender drop** (no explicit send) still stops the loop.
12. Test: event burst during debounce produces exactly one save (coalescing assertion via backup count).
13. Test: `reactive_save_session` wrapper rejects/uses `save_interval` math (wrapper ↔ core contract).
14. Test: shutdown **grace expiry** path — task wedged → abort → final save still runs.
15. Test: refused stream *never* writes garbage (fallback saves stay valid JSON).
16. Enable `clippy.toml` test allowances + run `cargo clippy --all-features --tests` in CI (kills lint-dark test code).
17. CI: `-D warnings` for rustc on the pinned nightly (catches unused-assignment class flaws).
18. Consider a stress/property test: N concurrent restores against the fake → idempotency invariants hold.

**Code quality (P1–P2)**
19. Move `spawn_single_window`'s blocking niri I/O to `spawn_blocking` (or async client).
20. File upstream niri-ipc issue/PR: expose fd / accept a `UnixStream` / return a shutdown handle (follows verify-before-filing; removes our protocol fork).
21. Deduplicate the fallback-loop warn/fallback config formatting (two warn! sites share the "…min" message).
22. Introduce a named shutdown-token type (newtype over `watch::Receiver<bool>` or tokio-util `CancellationToken`).
23. Review `SAVE_TASK_SHUTDOWN_GRACE` vs systemd `TimeoutStopSec` interaction; document expected stop latency in module.nix comments.
24. Consider exposing health-check info about the save loop (last save age already there; add "event stream state" if cheap).
25. main.rs module split (dedicated changeset, per the deliberate call): types / restore / save / cli.
26. After split: move the corresponding in-file unit tests with their modules; keep `cargo test` count green.

**Upstream watch / features (P2)**
27. Evaluate `focus_timestamp` (niri-ipc v25.11, already in our pinned crate) for smarter focus restoration (tie-break most recent).
28. Design **session-format v5**: capture `WindowLayout` (size/scroll position) with serde aliases + property tests (extend the round-trip/legacy-alias suite).
29. Investigate restore-time use of geometry: `Action::SetWindowWidth/Height` (does restore want sizes at all in a scrolling layout? write the analysis down either way).
30. Evaluate niri overview-state IPC (v25.05) for anything session-relevant.
31. Re-verify the "no native niri session support" non-goal on the next major niri release (v26.04 checked last).
32. Duplicate-window dedup on save (ROADMAP idea).
33. Spawn-timeout exponential backoff in restore retry (ROADMAP idea).
34. Config hot-reload via inotify (ROADMAP idea).
35. Per-app restore delay tuning / `--migrate` command (ROADMAP ideas).

**Ops / Nix / CI (P2–P3)**
36. Soak-test gating: add a `--save-once`-based cron/timer dry-run check for the soak week (cheap signal while user drives).
37. `nix flake check --all-systems` (aarch64) (ROADMAP).
38. Cross-platform CI job: macOS build-only (proc module is linux-gated) (ROADMAP).
39. Review journald log volume: per-window restore info lines (ROADMAP).
40. Decide third-party-CI-action policy (rust-cache vs Nix-native caching only) — see questions.
41. Consider cargo-audit/cargo-deny freshness: the parallel session added both to shell.nix — confirm CI's cargo-deny invocation covers the same deny.toml (it does today; re-check after their changes settle).
42. Add `.markdownlint`/lychee runs from the parallel session into CI if they're meant to be enforced (they're configured but CI doesn't call them).

**Docs (P3)**
43. README: graceful-shutdown line + expected stop latency.
44. CHANGELOG: reclassify shutdown-hang fix under `Fixed` during release tidy.
45. AGENTS: drop drift-prone line-count prose; keep citations-script-owned refs.
46. FEATURES: add row for per-app spawn metering invariant test (evidence trail for `SpawnLimiter`).
47. Consider `docs/DOMAIN_LANGUAGE.md` sketch while terms are fresh (deferred 2026-09-04; revisit only if vocab confusion recurs).
48. Record this session's three near-misses in AGENTS "session rules" (already partially done) — add the warning-gating rule.

**Park / explicitly not now**
49. tokio-util dependency for cancellation tokens — only if #22's newtype feels insufficient.
50. Publishing to crates.io / DMS integration — stay parked per ROADMAP non-goals.

## g) QUESTIONS I CANNOT ANSWER MYSELF

1. **Release go-ahead**: May I tag and push **0.5.0** now (tag on the current
   green state, including graceful shutdown + backoff), or do you want a soak
   period between this session's behavioral changes and the release SystemNix
   will pin?
2. **Hardware**: For the soak test and ROADMAP Q3 — will you run the
   daily-driver soak yourself this week, and which terminals are your daily
   drivers (so those flag profiles get must-not-regress soak coverage)?
3. **CI policy**: Is a SHA-pinned third-party action (`Swatinem/rust-cache`)
   acceptable in this repo's supply-chain posture, or should cargo caching be
   Nix-native only (magic-nix-cache) even if the ~2-min rebuild stays?

---

_Environment notes: an auto-commit daemon committed continuously (9+ commits
this session), and a second concurrent session landed jscpd annotations,
GPL licensing metadata, lychee/markdownlint config, and devshell tooling
mid-session; the combined tree passed the full verification battery at report
time. Uncommitted small deltas in `shell.nix`/docs at report time belong to
that session, not this one._
