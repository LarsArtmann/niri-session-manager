# Status Report — 2026-09-15 18:16 CEST — Clippy Gate Cleanup & First-Ever CI Run

Session scope: two passes over `niri-session-manager` triggered by a BuildFlow/clippy
paste. Pass 1 triaged the paste (231 test-code clippy findings, devshell tool gaps,
jscpd/vulnix accepted findings). Pass 2 re-verified everything, ran the suite 3×,
ran the full dev-mode BuildFlow gate, fired and verified the fork's **first real CI
run**, and synced all living docs. No service code (`restore`/`save`/`session`/
`terminal`/`ipc`/`proc` logic) was touched — every change is test-module lint
attributes or docs.

**Honesty note up front (format override):** the status-report skill's canonical
output is a styled HTML dashboard; the user explicitly requested `.md`, so this file
is Markdown. One-off override, not a new default.

---

## Self-review: what did I forget / do worse? (asked explicitly)

**Forgot or got wrong during the session:**

1. **Miswrote a fact in AGENTS.md** — my first rewrite of the fork/Actions warning
   claimed "pushes to `main` only run CI once the fix lands upstream remote", which
   is wrong: with Actions now enabled, the workflow's `push:` triggers fire on the
   fork normally. Caught it seconds later and fixed it. Lesson: I edited CI-trigger
   semantics from memory instead of re-reading `checks.yml` (lines 3–9) first.
2. **Blind test-output capture** — first 3× suite loop piped `cargo test --quiet`
   through `tail -1` and captured blank lines (quiet mode ends with an empty line),
   producing zero usable evidence for a full suite run. Re-ran with `rg 'test result'`
   - `set -o pipefail`. This is exactly the pipeline-masking failure mode
     AGENTS.md §"Cross-Cutting Lessons" warns about — I repeated a known class.
3. **Parallel BuildFlow invocations** — running fast-mode and dev-mode back-to-back
   while a watch job was live tripped `SQLITE_BUSY` on the result-cache DB
   (`journal_mode=WAL: database is locked`), silently disabling resume/timing for
   that run. Harmless here, but self-inflicted.
4. **Memory-maintenance lag** — AGENTS.md says update the moment of discovery, yet
   two new operational gotchas (dprint-format rewrites Markdown mid-edit →
   "file modified" rejections; BuildFlow DB lock under parallel runs) are only being
   recorded now, in this report, not in AGENTS.md session rules.
5. **Missed an inconsistency while reading Cargo.toml** — the `[lints.clippy]`
   comment references "the project's MSRV (rust-version)" but Cargo.toml declares
   **no `rust-version` key** (item 34 below). Only surfaced while drafting this
   report.
6. **Didn't re-run `nix flake check` after the pass-2 doc edits** — docs-only, and
   treefmt doesn't cover Markdown, so risk ≈ 0, but pass 1 ended with that gate and
   pass 2 didn't; inconsistent rigor.

**Could have done better:**

- Verified the dispatched CI run's `headSha` **before** celebrating: the green run
  (`34990898129`) tested pushed HEAD `4256880`, which predates the lint fix. The run
  proves the CI pipeline works; it does **not** prove the fix under Actions.
- Read `.github/workflows/checks.yml` in full before the first edit touching its
  subject matter.
- Run one `nix develop -c buildflow --build-mode full` (~5–10 min) for the complete
  gate verdict (tests + coverage legs) instead of fast+dev only.
- Labeled background-job outputs so 4 concurrent shells stayed self-explanatory.

**Still improvable:** see (e) — the top items are push + CI verification of the fix,
switching CI clippy to `--all-targets`, and HARVESTing section (f) into
TODO_LIST/ROADMAP.

---

## a) FULLY DONE (verifiable, with evidence)

| #  | Item                                                                                                                                                                                                                                                                                                                                  | Evidence                                                                                                                                             | Scope                                             |
| -- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| 1  | 231-error local clippy gate fixed: scoped exemptions (`pedantic`, `nursery`, `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`, `arithmetic_side_effects`, `as_conversions`) on the three test modules; footgun denies (`todo`/`unimplemented`/`exit`/`unreachable`/`string_slice`/`panic_in_result_fn`) still apply in tests | `cargo clippy --all-features --all-targets` → **0 warnings** (was 231 errors); commit `80dc523` (fake_niri.rs) + `5f1ec78` (tests.rs, proc.rs)       | `src/fake_niri.rs`, `src/tests.rs`, `src/proc.rs` |
| 2  | CI's exact clippy form still green                                                                                                                                                                                                                                                                                                    | `cargo clippy --all-features` → clean, `Finished dev profile`                                                                                        | whole crate                                       |
| 3  | Full test suite green, three consecutive runs                                                                                                                                                                                                                                                                                         | `124 passed; 0 failed; 1 ignored` × 3, exit 0 each (CI asserts `expected=124` — unchanged)                                                           | suite-wide                                        |
| 4  | First-ever real CI run on the fork: fired via `workflow_dispatch` and **passed**                                                                                                                                                                                                                                                      | run `34990898129`, green at `4256880`, 5m27s, all steps ✓ (build, test-count assertion, clippy, fmt, dry-run smoke, nix, cargo-deny, docs-citations) | `.github/workflows/checks.yml` on GitHub's runner |
| 5  | cargo-audit/cargo-deny "no such command" root-caused as environmental                                                                                                                                                                                                                                                                 | both present in devshell (`cargo-deny 0.20.2`, `cargo-audit 0.22.2` via `nix develop -c`); BuildFlow-in-devshell runs both ✔                         | environment, no repo change                       |
| 6  | "9 tools unavailable" identified as JS/TS/Python fleet census (jest, knip, madge, publint, svelte-check, vitest, vue-tsc, js-coverage, interrogate) — N/A for a Rust+Nix repo                                                                                                                                                         | BuildFlow `--verbose` health-check listing                                                                                                           | BuildFlow config, no repo change                  |
| 7  | jscpd (5) and vulnix (20) confirmed as documented-accepted findings; rationale comments intact                                                                                                                                                                                                                                        | AGENTS.md "Known Issues"; comments verified at `src/proc.rs:186-188`, `src/fake_niri.rs:738-740`                                                     | no change (deliberate)                            |
| 8  | Full dev-mode BuildFlow gate: pass                                                                                                                                                                                                                                                                                                    | exit 0, `44 success, 0 failed`, only the two accepted finding groups                                                                                 | fleet gate                                        |
| 9  | Formatting/citations/flake gates                                                                                                                                                                                                                                                                                                      | `cargo fmt --all -- --check` OK; `bash scripts/docs-citations.sh` OK (×2); `nix flake check` "all checks passed" (pass 1)                            | repo-wide                                         |
| 10 | Docs synced to current truth: AGENTS.md lint-policy bullet rewritten (scoped allows, `--all-targets` clean, devshell-only tools note); fork/Actions warning rewritten (enabled, first run green, triggers fire normally)                                                                                                              | commits `5f1ec78`, `15275fb` + pending daemon pickup                                                                                                 | `AGENTS.md`                                       |
| 11 | CHANGELOG `[Unreleased]` seeded with two entries (all-targets clippy gate; first real CI execution)                                                                                                                                                                                                                                   | `rg 'First real CI execution' CHANGELOG.md` → 1                                                                                                      | `CHANGELOG.md`                                    |
| 12 | TODO_LIST: "Enable GitHub Actions + fire/verify first run" closed per the DONE legend (removed from table, logged in CHANGELOG, footer updated)                                                                                                                                                                                       | Actions row gone (`rg` 0 hits); footer "Post-push discovery resolved 2026-09-15"                                                                     | `TODO_LIST.md`                                    |
| 13 | Daemon-race handled correctly: two "file modified" rejections (dprint reformatted Markdown mid-edit) → re-read fresh, re-applied, verified nothing half-applied                                                                                                                                                                       | edit-tool rejections + fresh `view` + marker greps                                                                                                   | `CHANGELOG.md`, `TODO_LIST.md`                    |
| 14 | Daemon commits audited: every auto-commit this session is exactly authored work (`80dc523` fake_niri, `5f1ec78` proc/tests/AGENTS, `c3e5913` CHANGELOG, `15275fb` docs) — no unexpected diffs accepted or reverted                                                                                                                    | `git show --stat` per commit                                                                                                                         | git history                                       |

## b) PARTIALLY DONE (what works, what's open, blocker, effort)

| # | Item                          | Works now                                                          | Open                                                                                                                                           | Blocker                              | Effort |
| - | ----------------------------- | ------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ | ------ |
| 1 | All-targets lint gate         | Locally clean and committed (`80dc523`, `5f1ec78`)                 | **Unpushed**: `origin/main` still fails `clippy --all-targets` for fresh clones; CI hasn't tested the fix (green run tested pre-fix `4256880`) | Push requires explicit user go-ahead | S      |
| 2 | CI enablement/workflow health | Actions enabled; dispatch + first run verified green               | Node20-deprecated action targets, Magic Cache 400s, README badge, branch-protection decision (see c/d)                                         | none                                 | S–M    |
| 3 | BuildFlow gate evidence       | fast + dev modes verified in-devshell this session (exit 0, 44/44) | full mode (test-race + coverage legs, ~5–10 min) not re-run this session; last full evidence is the user's pasted run                          | none (just runtime)                  | M      |
| 4 | Docs sync                     | CHANGELOG / TODO_LIST / AGENTS current                             | HARVEST of section (f) into TODO_LIST/ROADMAP not yet run; CONTRIBUTING.md doesn't mention the test-module exemption policy                    | none                                 | S      |
| 5 | AGENTS.md session rules       | Policy sections updated                                            | The two new operational gotchas (dprint edit-race, BuildFlow SQLITE_BUSY) not yet in the session-rules list                                    | none                                 | S      |
| 6 | Suite confidence              | 3× green this session                                              | Repo convention is 5× after concurrency-affecting changes; my change was attribute-only, so 3× was insurance, but a 5× pre-push run is cheap   | none                                 | S      |

## c) NOT STARTED (planned, zero code this session)

| #  | Item                                                                                | Why not started                                                      | Still wanted?                 |
| -- | ----------------------------------------------------------------------------------- | -------------------------------------------------------------------- | ----------------------------- |
| 1  | Real-hardware soak test of reactive saves + idempotent restore (TODO_LIST High)     | Needs the daily-driver machine; fake server can't prove event timing | Yes — top remaining item      |
| 2  | Terminal ground truth (ROADMAP Q3): which terminals actually run daily              | 🔵 BLOCKED on maintainer input                                       | Yes                           |
| 3  | v5 layout-geometry _application_ in restore                                         | Deliberately waits for the soak test                                 | Yes, after (1)                |
| 4  | CI clippy step → `--all-targets`                                                    | Newly possible only since the gate is clean; policy decision         | Yes (cheap, locks in the fix) |
| 5  | Stable-toolchain CI leg (repo must compile on stable; CI builds nightly)            | Not yet designed; past release broke NixOS stable over a `let` chain | Yes                           |
| 6  | Bump Node20-targeting actions (checkout `11d5960a`, nix-installer `da36cb69`)       | Discovered this session via run annotations                          | Yes                           |
| 7  | Nix Magic Cache 400 investigation / fallback                                        | Upstream service flake; non-fatal                                    | Optional                      |
| 8  | README CI badge + "CI runs on the fork" note                                        | Awaits pushed green run for an honest badge                          | Yes, after push               |
| 9  | Branch protection / required Checks on fork `main`                                  | Owner preference                                                     | Ask                           |
| 10 | Non-Linux CI leg for the `#[cfg(target_os = "linux")]` portable fallback in proc.rs | Cost/benefit undecided                                               | Maybe                         |
| 11 | NixOS module vm-test                                                                | Large effort, undecided                                              | Roadmap fuel                  |
| 12 | v0.6.1 release decision                                                             | User cadence call                                                    | Ask (question 3)              |

## d) TOTALLY FUCKED UP

Radical honesty, sharpest first. **Nothing found at data-loss or user-facing
severity; the shipped daemon code is untouched and all runtime invariants hold.**
These are the genuinely broken/wrong things observed:

1. **`origin/main` is a broken-tree trap for the all-targets gate.** Anyone (or any
   agent) cloning fresh and running `cargo clippy --all-features --all-targets` —
   the form this repo should be judged by — gets **231 deny-level errors** at
   `4256880`. The fix exists only as ~5 unpushed local commits. Severity: blocks
   nobody in production, but it's a latent productivity tax and a split-brain risk
   (two machines, two verdicts). Root cause: deny list predates the test-exemption
   policy. Mitigation: push (needs user).
2. **CI runs on deprecated Node20 action targets** (annotations, run `34990898129`):
   `actions/checkout@11d5960a…`, `DeterminateSystems/nix-installer-action@da36cb69…`
   are force-executed on Node24. When GitHub drops the shim, **CI breaks without any
   repo change**. Severity: future CI outage. Mitigation: bump the pinned SHAs;
   check why `github-actions-pinning` repair didn't flag the staleness.
3. **Nix Magic Cache failed both restore and save with HTTP 400** ("Our services
   aren't available right now") in the first run — every CI run pays full cold
   builds until it heals. Severity: slow/flaky CI only. Mitigation: retry/accept;
   optional `actions/cache` fallback.
4. **BuildFlow result-cache DB locks under concurrent invocations**
   (`SQLITE_BUSY`, `journal_mode=WAL` pragma failed at 17:51) — resume/timing
   silently disabled for that run; a false-green-friendly failure mode if a gate
   were trusted without noticing the WARN. Severity: tooling honesty annoyance.
   Root cause: my own parallel runs. Mitigation: serialize BuildFlow invocations;
   document in AGENTS.md.

## e) WHAT WE SHOULD IMPROVE (process/design, not bugs)

1. **Make CI enforce what locals enforce.** CI's testless clippy form was the _only_
   green form while local `--all-targets` failed — a policy encoded in invocation
   flags instead of in code. The exemption attributes now make `--all-targets` the
   single source of truth; switch CI to it (item 4c/f-4).
2. **Verify claims at the artifact, not the queue.** I reported "CI green" before
   checking the run's `headSha`. Rule going forward: a verification claim about a
   remote run must cite its commit.
3. **Never trust an unstated invocation.** The paste's warnings-not-errors clippy
   output was BuildFlow's capped `--fix` run; the plain invocation was erroring.
   Reading the deny config first (2 minutes) would have explained the discrepancy
   before any fix.
4. **Format-before-edit discipline:** run the formatter (or avoid it) before manual
   Markdown edits in BuildFlow-covered repos, or expect "file modified" rejections;
   re-read, don't retry blind (this session handled it right once the rejection hit).
5. **Serialize BuildFlow.** One invocation at a time; the result cache is a shared
   SQLite DB and concurrent runs degrade silently (WARN only).
6. **Immediate memory writes.** The two gotchas should have landed in AGENTS.md
   session rules the minute they happened, not in a report hours later.
7. **Scoped `#[allow]` lists drift across three sites** (`fake_niri.rs`,
   `tests.rs`, proc.rs). Acceptable now (comment points at AGENTS.md), but if a
   fourth test module appears, consider a shared `#[cfg(test)]` lint-policy module
   or a doc-test'd checklist.
8. **MSRV honesty:** Cargo.toml's lints comment cites a `rust-version` that isn't
   declared. Declare it or fix the comment (item 34).
9. **Background-job output hygiene:** capture with explicit result-line greps and
   `set -o pipefail` from the start; `tail -1` on quiet test output captures nothing.

## f) NEXT 50 (ranked; Impact / Effort S<30min, M≤2h, L>2h / Category)

Feeds `docs-health` HARVEST into TODO_LIST (actionable) vs ROADMAP (idea fuel).

| #  | Task                                                                                                                                    | Impact   | Effort | Category      |
| -- | --------------------------------------------------------------------------------------------------------------------------------------- | -------- | ------ | ------------- |
| 1  | Push `main` to origin (5 local commits: lint fix + docs) — unblocks 2, 4, 8, 12, 27                                                     | Critical | S      | Cleanup       |
| 2  | After push: fire `workflow_dispatch` again; assert green on the _fixed_ head                                                            | High     | S      | Quality       |
| 3  | HARVEST this report's (f) into TODO_LIST.md / ROADMAP.md                                                                                | High     | S      | Documentation |
| 4  | Switch CI clippy step to `--all-features --all-targets` (tests linted in CI; locks in the exemption policy)                             | High     | S      | Quality       |
| 5  | Cut v0.6.1 carrying the two `[Unreleased]` entries; tag + push (tag trigger fires CI)                                                   | High     | S      | Release       |
| 6  | Bump `actions/checkout` + `nix-installer-action` past Node20 deprecation                                                                | High     | S      | Cleanup       |
| 7  | Real-hardware soak test: reactive saves + idempotent restore on the daily driver (pre-existing TODO #1)                                 | High     | M      | Quality       |
| 8  | Add CI badge + "CI verifiably runs" line to README                                                                                      | Medium   | S      | Documentation |
| 9  | Add the two new gotchas to AGENTS.md session rules (dprint edit-race; BuildFlow SQLITE_BUSY serialization)                              | Medium   | S      | Documentation |
| 10 | Investigate why `github-actions-pinning` repair didn't flag the stale Node20 SHAs (buildflow gap → upstream buildflow task)             | Medium   | S      | Cleanup       |
| 11 | Add stable-toolchain CI leg (`cargo +stable build/test`) to lock the NixOS-stable guarantee                                             | Medium   | M      | Quality       |
| 12 | Declare `rust-version` (MSRV) in Cargo.toml, or fix the lints comment that cites it                                                     | Medium   | S      | Cleanup       |
| 13 | Run one full-mode BuildFlow (test-race + coverage legs) for complete gate parity evidence                                               | Medium   | M      | Quality       |
| 14 | `nix flake check --all-systems` (aarch64-linux, aarch64/x86_64-darwin currently omitted from the check)                                 | Medium   | S      | Quality       |
| 15 | Re-run `restore_burst` benchmark (`--release -- --ignored`), compare vs 100.4 ms/window in docs/benchmarks                              | Medium   | S      | Quality       |
| 16 | Magic Cache 400s: retry once / accept / add `actions/cache` fallback — decide and document                                              | Medium   | S      | Quality       |
| 17 | CONTRIBUTING.md: document the test-module clippy exemption policy + how to extend it to a 4th test module                               | Medium   | S      | Documentation |
| 18 | Verify `.github/dependabot.yml` exists and matches real modules (buildflow dependabot-auto-configure ran; confirm output)               | Medium   | S      | Cleanup       |
| 19 | Confirm `cargo-deny` in CI fails closed on a new advisory (assert `Files > 0`-style honesty per the go-paperless lesson)                | Medium   | S      | Quality       |
| 20 | Add `--locked` to CI `cargo build/test/clippy` to catch Cargo.lock drift                                                                | Medium   | S      | Quality       |
| 21 | Verify `run_import` writes via `atomic_write` (crash-safe swap of the live session)                                                     | Medium   | S      | Quality       |
| 22 | Verify `--max-backup-count 0` handling is intentional (zero-arg rejection list covers save-interval + walk-depth; is 0 backups valid?)  | Medium   | S      | Quality       |
| 23 | Non-Linux CI leg (macOS) so the `#[cfg(target_os)]` proc.rs fallback keeps compiling                                                    | Medium   | M      | Quality       |
| 24 | Branch protection on fork `main` requiring the Checks workflow (owner decision)                                                         | Medium   | S      | Quality       |
| 25 | Terminal ground truth (ROADMAP Q3): maintainer names the daily terminals → must-not-regress soak coverage                               | Medium   | M      | Quality       |
| 26 | Post-soak: design v5 geometry _application_ (restore-side layout)                                                                       | Medium   | L      | Feature       |
| 27 | Verify `niri-ipc 25.5.1` is still the freshest compatible crate (cargo-update ran; confirm no held-back bump)                           | Medium   | S      | Cleanup       |
| 28 | Add a concurrency `concurrency:` group to the workflow (cancel superseded runs)                                                         | Low      | S      | Cleanup       |
| 29 | Nightly scheduled CI run for flake/advisory drift detection                                                                             | Low      | S      | Quality       |
| 30 | Fire the `v*` tag trigger once during the next release to verify that path end-to-end                                                   | Low      | S      | Quality       |
| 31 | Test `workflow_dispatch` from a non-`main` branch once                                                                                  | Low      | S      | Quality       |
| 32 | `--all-features` is currently vacuous (no `[features]` in Cargo.toml) — document that in AGENTS.md commands or drop the flag from docs  | Low      | S      | Documentation |
| 33 | Verify embedded `DEFAULT_APP_CONFIG_TOML` stays in sync with `AppConfig` (is there a test? add one if not)                              | Low      | S      | Quality       |
| 34 | Verify the reconnect-backoff reset (alive ≥5s) has a dedicated regression test; add if missing                                          | Low      | S      | Quality       |
| 35 | Confirm jscpd finding count didn't grow from the new attribute blocks (5 → ?); refresh the AGENTS.md accepted note if so                | Low      | S      | Quality       |
| 36 | NixOS module: comment documenting the 6-of-7 tunable mirror + why `dryRun` is CLI-only                                                  | Low      | S      | Documentation |
| 37 | README: document the five terminal profiles + the "adding a terminal" checklist (currently AGENTS-only)                                 | Low      | S      | Documentation |
| 38 | Annotate/archive superseded status reports under `docs/status/archived/` (docs-health ANNOTATE)                                         | Low      | S      | Documentation |
| 39 | ROADMAP: record the CI-never-ran discovery → resolution as a closed question                                                            | Low      | S      | Documentation |
| 40 | CHANGELOG: verify Keep-a-Changelog link definitions exist for `[Unreleased]`/`[0.6.1]` when cutting the next version                    | Low      | S      | Documentation |
| 41 | Pre-commit mode: consider adding the all-targets clippy gate to BuildFlow's pre-commit build mode                                       | Low      | S      | Quality       |
| 42 | Export/import design: should archives carry the restore marker / config? (ROADMAP question, not a bug)                                  | Low      | S      | Feature       |
| 43 | NixOS module vm-test (e.g. NixOS VM runner exercising the systemd unit)                                                                 | Low      | L      | Feature       |
| 44 | Periodic cadence: weekly `buildflow update` + `cargo update` + `cargo audit` review slot                                                | Low      | S      | Cleanup       |
| 45 | Replace the two accepted vulnix build-time advisory notes with a dated re-triage reminder (quarterly)                                   | Low      | S      | Documentation |
| 46 | Re-check the `nixfmt-rfc-style` upstream deprecation warning only when nixpkgs resolves it (AGENTS.md: accepted; do not re-investigate) | Low      | S      | Documentation |
| 47 | Add `--save-once` suspend-hook end-to-end documentation with the systemd slice it runs in                                               | Low      | S      | Documentation |
| 48 | Assert spawn-arrival-order independence stays true when tokio/runtime versions bump (re-run metering tests specifically)                | Low      | S      | Quality       |
| 49 | Consider `clippy --all-targets -- -D warnings` in CI instead of relying on Cargo.toml denies alone (belt-and-braces)                    | Low      | S      | Quality       |
| 50 | Clean session residue: `/tmp/bf-*.log`, `/tmp/test-loop.log`, `/tmp/ci-first-run.log` (or archive into docs if evidentiary)             | Low      | S      | Cleanup       |

## g) TOP 3 QUESTIONS (only you can answer)

1. **Push?** ~5 local commits (lint-gate fix + docs) sit unpushed; `origin/main`
   still fails `--all-targets` clippy and CI can't test the fix until it's there.
   I never push without an explicit go-ahead — **may I push `main` now?**
2. **CI lint policy:** once pushed, switch CI's clippy step to
   `--all-features --all-targets` so tests are linted remotely too (it's clean
   now)? Or keep CI on the testless form and treat `--all-targets` as a
   local-only gate?
3. **Release cadence:** cut **v0.6.1** from the two `[Unreleased]` entries soon
   (SystemNix pins tags), or hold and accumulate toward 0.7.0?

---

_Point-in-time snapshot at 2026-09-15 18:16 CEST, code at local `15275fb` (+pending
daemon pickup of the final doc polish), CI evidence at `4256880` (run
`34990898129`). Historical evidence only — re-verify before treating as current
truth. Section (f) is HARVEST input for TODO_LIST/ROADMAP._
