# BuildFlow Findings Remediation — Status Report

- **Date:** 2026-09-06 07:02 CEST
- **Scope of this report:** this session only — the BuildFlow failure report (`BuildFlow failed 32/37`, exit 1) handed to the agent at session start, and everything the agent did to run it down. Point-in-time snapshot; will go stale.
- **Concurrency note:** a second session was landing the capped exponential reconnect-backoff feature (`src/main.rs`, `.github/workflows/checks.yml`, `FEATURES.md`, 6 new tests) while this session ran. Its mid-flight states broke compilation twice and flapped 3 tests transiently. This session did **not** touch its files (repo rule: never mutate one file from two tools; never revert changes you didn't author). Final converged state: **114 tests, 0 failed**.

---

## a) FULLY DONE

1. **Session-start toolchain baseline re-verified** — `cargo build`, `cargo test` (108 passed at that time), `cargo clippy --all-features`, `cargo fmt --all -- --check`, `nix flake check` all green before any edit (AGENTS.md session rule).
2. **flake.nix deprecation fixed** — `getPlatform = p: p.hostPlatform.system` (top-level `hostPlatform` alias, warns on every eval) → `p.stdenv.hostPlatform.system` at flake.nix:31, overlay at :61. Verified: `nix eval .#checks.x86_64-linux.formatting.name` now warning-free; `nix fmt` no changes needed.
3. **cargo-deny licenses failure root-caused and fixed** (the BuildFlow report hid a REAL failure behind "no such command: deny"): three separate causes, all fixed —
   - own crate had no `license` field → `license = "GPL-3.0-only"` in Cargo.toml:5;
   - `niri-ipc 25.11.0` is `GPL-3.0-or-later`, not in allowlist → added to deny.toml;
   - `option-ext 0.2.0` (transitive via `dirs`) is `MPL-2.0`, not allowed → added to deny.toml;
   - plus `meta.license = lib.licenses.gpl3Only` in default.nix.
     Verified: `cargo deny check advisories licenses bans sources` → **exit 0, all four ok, zero rejections** (this is the exact CI command).
4. **cargo-audit verified clean** — `nix run nixpkgs#cargo-audit -- audit` → exit 0, 1239 advisories loaded, 140 dependencies scanned, zero vulnerable. BuildFlow's "4 findings" were purely missing-binary stderr noise.
5. **vulnix assessed with a real budget** — BuildFlow killed it at ~2 min; with 480 s it completes. Verdict: flags 20 derivations (binutils-2.46, bison-3.8.2, zlib-1.3.2 …) that are **build-time stdenv toolchain only**; the shipped runtime closure is **7 store paths and contains none of them** (`nix path-info -r ./result`). Disposition: accepted, not actionable at repo level; documented in AGENTS.md Known Issues.
6. **jscpd 6 clones judged and dispositioned** — all six are test-fixture similarity (proc.rs kitty/fish proc-tree setups ×5, fake_niri.rs two-IPC-test prefix ×1). Followed the deduplicate-code skill: **accepted as intentional** (each test's fixture shape is its input data; the env guard's lifetime is load-bearing; extraction would hide what makes each behavior test readable). One-line rationale comments added in both test modules ("do not extract").
7. **lychee link checker green** — `.lycheeignore` excludes `https://fsf.org/` (canonical GPLv3 text, "changing it is not allowed"; server itself TLS-broken); `lychee.toml` accepts 429 (gnu.org rate-limits checkers; the 429 hit was inside canonical license text at LICENSE.md:668) and throttles `www.gnu.org` (concurrency 1, 3 s interval). First config draft used an invalid `timeout` host key (lychee v0.24.2 schema) — removed, verified: **exit 0, 0 errors, 1 excluded**.
8. **markdown-lint 0 findings** — aggregated the 1650+40+36+9+2 findings into rule families: MD013/MD024/MD026/MD029 are style-opinion rules that contradict the repo's docs model (wide tables, long URLs, archived point-in-time status docs that must not be rewritten) → disabled in new `.markdownlint.json`; MD040 kept **on** and its 2 real findings fixed (`README.md:47`, `docs/benchmarks/restore-burst.md:16` → `` ```text ``). Verified via `buildflow -s markdown-lint --format finding`: **0 findings**.
9. **Devshell CI-parity** — `cargo-deny` + `cargo-audit` added to shell.nix so the supply-chain gates are runnable locally with the documented commands (they were previously not on PATH anywhere in the dev flow).
10. **Docs kept in sync** — CHANGELOG `[Unreleased]` → new `### Fixed` section (license declaration + flake warning); AGENTS.md → supply-chain policy bullet, lint-config bullet, and two Known Issues bullets (vulnix build-time-only, jscpd accepted clones).
11. **Final gates re-run** — `cargo test` **114 passed / 0 failed** (incl. the concurrent session's 6 new tests), `cargo clippy --all-features` clean, `rustfmt --check` clean on both files this session touched, `nix build` + `nix flake check` pass, `scripts/docs-citations.sh` passes, lychee exit 0, cargo-deny exit 0.

## b) PARTIALLY DONE

1. **BuildFlow report closure** — every failing/detect-only step was verified individually, but the **full `buildflow` pipeline was never re-run end-to-end** to confirm the new overall score/exit code. Open: one full run (~5–10 min; mind the vulnix timeout). No blocker; effort S.
2. **Crate-wide `cargo fmt --all -- --check`** — verified scoped to this session's two Rust files only. At handoff the concurrent session still had `src/main.rs` dirty, so crate-wide fmt state was **not re-confirmed after their work converged**. Open: one `cargo fmt --all -- --check` once the tree is quiet. Effort S.
3. **vulnix disposition** — assessed + documented as accepted, but not made _reproducibly green_: no whitelist file, no documented invocation that scans the runtime closure only. Open: decide between whitelist vs scan-scope fix vs accept-forever. Effort S/M.
4. **Concurrent session's landing** — their reconnect-backoff work is committed and its tests pass, but their `FEATURES.md`/`checks.yml` edits were still uncommitted at snapshot time and their fmt/CI state was deliberately not verified by this session (out of scope, two-writers rule).

## c) NOT STARTED

1. **BuildFlow configuration tuning** — vulnix needs >2 min (default timeout kills it) and `nix-build` enumerates foreign-system attrpaths (`aarch64-darwin` cannot realize on `x86_64-linux`). No project-level `.buildflow.yml` exists (searched repo root and `~/.config/buildflow` — only `device_id` + telemetry ack). Not started: awaiting user decision on whether repo-level BuildFlow config is wanted (see g2/g3).
2. **deny.toml stale allowlist cleanup** — 6 `license was not encountered` warnings (e.g. `Unicode-DFS-2016`, `CDLA-Permissive-2.0`) — harmless; deliberate keep-as-buffer vs prune is undecided.
3. **Platform-matrix decision for the flake `systems` input** — restricting from `nix-systems/default` (4 platforms) to actual consumer platforms would remove foreign-system checks entirely (and with them BuildFlow's platform-mismatch failures). Blocked on g3 (SystemNix consumption).
4. **markdownlint/lychee enforcement in repo CI** — configs now exist repo-side, but `.github/workflows/checks.yml` doesn't run them (and was being edited by the other session — hands off).
5. **Benchmark refresh** — `restore_burst` (`#[ignore]`d) not re-run after the backoff/`EventConnection` changes landed; docs/benchmarks/restore-burst.md numbers predate them.
6. **License intent review** — `GPL-3.0-only` was this session's conservative choice; matching `niri-ipc` would be `GPL-3.0-or-later`. Decision needed (g1); not started because it changes a published API-adjacent field.

## d) TOTALLY FUCKED UP

Nothing in this session's delta is broken — every gate listed in a11 is green and verified. But radical honesty, worst findings first:

1. **The licenses gate was red and nobody noticed.** `cargo-deny check licenses` was failing on the repo's own crate (missing `license` field) and on `niri-ipc`/`option-ext` before this session — meaning the supply-chain gate that CI runs has been passing-broken or CI hasn't run green since the relevant dependency versions landed. Root cause of the non-detection: BuildFlow reported cargo-deny as "no such command: deny" (missing binary) instead of a hard preflight failure, so a real policy failure masqueraded as tooling noise. Mitigation now in place: real binary run documented + tools in devshell. Residual risk: anything else that only CI exercises may be silently stale the same way. **Severity: high (process), mitigated.**
2. **BuildFlow's report was untrustworthy in both directions** — it screamed about 4 cargo-audit "findings" (actually missing binaries, zero real advisories) while burying the one real cargo-deny failure it should have surfaced. Working from that report alone would have "fixed" nothing. Lesson applied this session: every BuildFlow finding was re-derived with the real tool before acting. **Severity: medium (mitigated by verification discipline).**
3. **This session's handoff overclaimed one gate**: the final summary said "fmt ✅" while crate-wide `cargo fmt --all -- --check` had last been run against a tree containing the _other_ session's unformatted in-flight `main.rs`; only this session's files were re-checked. Honest status: my files clean, crate-wide unknown-at-the-time (see b2). **Severity: low, but it's exactly the "all green" lie AGENTS.md warns about.**
4. **Two concurrent writers on one tree is still ad-hoc.** This session saw the other session break the build twice mid-flight (E0499/E0597) and flap 3 IPC tests (JoinHandle double-poll ×2, vacuous overlap assertion). No code of this session's was involved (comments + config only), and the correct no-touch discipline held — but nothing in the repo prevents a future session from "fixing" the other writer's code and destroying work. **Severity: latent; process risk, not current breakage.**
5. **BuildFlow environment debt observed in preflight and ignored** (out of repo scope, but real): BuildFlow binary stale vs HEAD, 2.27 GB sqlite DB needing VACUUM, missing `go-licenses`. These degrade the reliability of the very report that started this session.

## e) WHAT WE SHOULD IMPROVE

1. **Missing-binary findings must fail preflight, not report noise** — BuildFlow should hard-fail when a tool (`cargo audit`, `cargo deny`) isn't on PATH instead of emitting 4 "findings" that bury real signal. Until then, the discipline that worked here: _never_ act on a BuildFlow finding without re-running the real tool.
2. **Verify-after-convergence, not verify-while-racing** — when a concurrent writer is landing, re-run the crate-wide gates once after their state settles, and don't claim gates verified that were only scoped (the fmt handoff slip).
3. **Supply-chain gates belong in the documented local loop** — AGENTS.md's Commands block should include the audit trio (`cargo deny check …`, `cargo audit`, optionally vulnix with a realistic timeout), now that the tools are in the devshell. A gate nobody can run locally is a gate that stays red unnoticed (see d1).
4. **Accepted-risk needs a reproducible artifact** — "vulnix findings are build-time-only" lives in AGENTS.md prose; a whitelist file or a documented runtime-closure invocation would make the acceptance checkable instead of lore.
5. **jscpd/markdown/lychee dispositions should live in repo config, not only in a status report** — done for markdownlint/lychee this session; jscpd's "accepted test fixtures" is currently only comments + AGENTS.md. If BuildFlow ever gates on jscpd, that decision needs config too.
6. **Concurrent-session protocol** — worked, but should be explicit: check `git status` for foreign dirty files before _any_ edit; scope fmt/deny checks to your delta; re-run suite after convergence; never format/revert a file another writer is mid-flight on.

## f) NEXT TASKS (ranked; Impact / Effort / Category)

| #  | Task                                                                                                                                                                  | Impact | Effort | Category      |
| -- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ | ------ | ------------- |
| 1  | Re-run full `buildflow` pipeline end-to-end; confirm exit code + score after this session's fixes                                                                     | High   | S      | Quality       |
| 2  | Re-run crate-wide `cargo fmt --all -- --check` once the concurrent session's `main.rs` settles                                                                        | High   | S      | Quality       |
| 3  | Add a project-level `.buildflow.yml`: vulnix timeout ≥5 min (or scan runtime closure), nix-build current-system only (needs g2/g3 answers)                            | High   | S      | Cleanup       |
| 4  | Decide license intent: keep `GPL-3.0-only` vs switch to `GPL-3.0-or-later` (matches niri-ipc) (g1)                                                                    | High   | S      | Decision      |
| 5  | Investigate why CI's cargo-deny licenses gate didn't catch the pre-session failure (last green run vs Cargo.lock bump date)                                           | High   | S      | Bug           |
| 6  | Add `cargo deny check advisories licenses bans sources` + `cargo audit` as a documented local command block in AGENTS.md Commands section                             | Medium | S      | Documentation |
| 7  | Add `.markdownlint.json` + `lychee` run to CI `checks.yml` so the new configs are enforced repo-side, not only BuildFlow-side                                         | Medium | S      | Quality       |
| 8  | Restrict flake `systems` input to actual consumer platforms, or keep 4-platform matrix knowingly (g3)                                                                 | Medium | S      | Decision      |
| 9  | Clean the 6 stale `license was not encountered` entries from deny.toml — or keep deliberately with a one-line rationale comment                                       | Low    | S      | Cleanup       |
| 10 | Make the vulnix acceptance reproducible: whitelist file for build-time stdenv advisories, or documented `vulnix` invocation over the runtime closure                  | Medium | S      | Quality       |
| 11 | Re-run `restore_burst` benchmark after the backoff/`EventConnection` landing; update docs/benchmarks/restore-burst.md numbers                                         | Medium | S      | Documentation |
| 12 | Run `deadnix` locally (CI gate) to confirm flake.nix/shell.nix changes introduced no dead Nix code                                                                    | Low    | S      | Quality       |
| 13 | Reconcile dprint markdown plugin (formats .md) with the new `.markdownlint.json` — confirm no format/lint ping-pong on the same files                                 | Medium | S      | Quality       |
| 14 | Add `meta.description` (+ consider `meta.platforms`) next to the new `meta.license` in default.nix                                                                    | Low    | S      | Cleanup       |
| 15 | Harvest this report's section (f) into TODO_LIST.md / ROADMAP.md via docs-health HARVEST (the skill's standing rule)                                                  | High   | S      | Documentation |
| 16 | Review the other session's `FEATURES.md`/`checks.yml` edits once committed; verify AGENTS.md cross-references still hold                                              | Medium | S      | Documentation |
| 17 | Stress the flappy IPC tests: run `cargo test fake_niri` 5–10× to confirm the concurrent session's JoinHandle/shutdown fixes hold under repetition                     | Medium | S      | Bug           |
| 18 | Verify the 3 transiently-failing tests from mid-session (`save_only_skips…`, `shutdown_aborts…`, `same_app_spawns…`) are stable in CI (nightly Rust)                  | Medium | S      | Bug           |
| 19 | BuildFlow housekeeping (global, user-side): rebuild/reinstall stale binary, VACUUM the 2.27 GB DB, resolve `GOEXPERIMENT=jsonv2` and `go-licenses` preflight warnings | Medium | S      | Cleanup       |
| 20 | Decide BuildFlow policy for detect-only tools (jscpd, markdown-lint, lychee): advisory forever, or gate after configs land (g2)                                       | Medium | S      | Decision      |
| 21 | Add a `.lycheeignore`/`lychee.toml` presence check to `scripts/docs-citations.sh` so link-check config can't silently vanish                                          | Low    | S      | Quality       |
| 22 | Consider lychee `--cache` in CI runs for speed + fewer rate-limit hits on gnu.org                                                                                     | Low    | S      | Quality       |
| 23 | Move/echo the AGENTS.md lint-config bullet into CONTRIBUTING.md if contributor-facing (doc placement judgment)                                                        | Low    | S      | Documentation |
| 24 | Confirm README states the license (GPLv3) now that the manifest declares it — add a License section if missing                                                        | Low    | S      | Documentation |
| 25 | Track zlib CVE-2026-27820 (CVSS 9.8, build-time only) upstream; revisit vulnix findings when nixpkgs bumps zlib/binutils                                              | Low    | S      | Cleanup       |
| 26 | Add `cargo-audit` (not just cargo-deny) consideration to CI — advisories DB is fresher than cargo-deny's pinned DB on slow weeks                                      | Low    | M      | Quality       |
| 27 | Pin or document the vulnix scan target (derivation vs out-path) so future runs are comparable                                                                         | Low    | S      | Documentation |
| 28 | Re-run lychee once outside gnu.org's rate-limit window to confirm steady-state behavior of the new config                                                             | Low    | S      | Quality       |
| 29 | Extract the "re-derive BuildFlow findings with the real tool" lesson into AGENTS.md session rules if not already implied by the verify-after-write rule               | Medium | S      | Documentation |
| 30 | Consider a flake `checks` entry running `cargo test`/clippy per system (currently checks = formatting only) — bigger change, decide deliberately                      | Medium | M      | Feature       |
| 31 | Sweep CHANGELOG `### Added` section (pre-existing formatting drift: one bullet sits outside a subsection header)                                                      | Low    | S      | Cleanup       |
| 32 | Re-verify `.markdownlint.json` picks up in the markdownlint _CLI_ (not only BuildFlow's runner) so humans get the same experience                                     | Low    | S      | Quality       |
| 33 | Document the jscpd acceptance in one line inside `CONTRIBUTING.md` (tests may keep explicit inline fixtures)                                                          | Low    | S      | Documentation |
| 34 | Check whether `dirs 5.0` (source of option-ext/MPL-2.0) has a maintained newer major with a smaller license surface                                                   | Low    | M      | Cleanup       |
| 35 | Re-run `cargo clippy --all-features` after the concurrent session's final main.rs state (last run was against their near-final tree)                                  | Medium | S      | Quality       |

_(Capped at 35 — the remainder would be filler; items 1–10 are the actionable core. Items marked "Decision" are blocked on section g.)_

## g) QUESTIONS I CANNOT ANSWER MYSELF

1. **License intent:** is `niri-session-manager` meant to be **GPL-3.0-only** (what I declared, conservative) or **GPL-3.0-or-later** (matching niri-ipc, more flexible for consumers)? I chose `-only` because LICENSE.md ships the plain v3 text and no source headers say "or any later version" — but this is an ownership/intent call only you can make, and it's now a published manifest field.
2. **BuildFlow configuration authority:** is there a supported way to configure BuildFlow per-project (e.g. commit a `.buildflow.yml`) that I'm allowed to add for this repo? I looked (repo root, `~/.config/buildflow`) and found nothing. Specifically: vulnix timeout ≥5 min, nix-build restricted to the current system, and whether jscpd/markdown-lint/lychee stay detect-only or become gates.
3. **Consumer platform matrix:** which systems does SystemNix actually consume this flake on — Linux-only, or also Darwin? Today the flake exposes 4 platforms, and BuildFlow's failed `nix build` of `checks.aarch64-darwin.formatting` is a direct consequence. Restricting `systems` would fix that class of failure permanently — but it's a breaking change for any consumer I can't see from here.

---

_Point-in-time snapshot written 2026-09-06 07:02 CEST. Format override note: written as Markdown per the user's explicit instruction; the status-report skill's canonical HTML dashboard format was skipped this once. Section (f) is the HARVEST input for TODO_LIST.md/ROADMAP.md._
