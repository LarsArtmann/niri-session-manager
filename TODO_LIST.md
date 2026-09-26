# TODO List

> Short-term, actionable, bounded work items, verified against the actual code.
> For long-term vision and unrefined ideas, use ROADMAP.md.
> Items are ranked by impact. Status is verified, not assumed.

## Status legend

| Status           | Meaning                                                     |
| ---------------- | ----------------------------------------------------------- |
| 🔴 `TODO`        | Not started. Needs doing.                                   |
| 🟡 `IN_PROGRESS` | Actively being worked on.                                   |
| 🔵 `BLOCKED`     | Cannot proceed, external dependency or decision needed.     |
| 🟢 `DONE`        | Completed. Remove from this list and log in `CHANGELOG.md`. |

## High Impact

| Task                                                                                                        | Status      | Impact | Effort | Evidence                                                                                                                  |
| ----------------------------------------------------------------------------------------------------------- | ----------- | ------ | ------ | ------------------------------------------------------------------------------------------------------------------------- |
| Bump the Node20-pinned GitHub Actions (`actions/checkout@11d5960a`, `nix-installer-action@da36cb69`) past the Node20 deprecation — when GitHub drops the shim, CI breaks with zero repo changes | 🔴 `TODO`   | High   | S      | run annotations on the first CI run (`34990898129`); pins still current in `.github/workflows/checks.yml:19,60` (verified 2026-09-26) |

## Medium Impact

| Task                                                                                                                                                                                                      | Status       | Impact | Effort | Evidence                                                                                                                              |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ | ------ | ------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| Terminal ground truth (ROADMAP Q3): which terminals run daily? ghostty/kitty/foot/alacritty all have live carrier-restore coverage; wezterm blocked on install. Daily-driver picks become must-not-regress | 🔵 `BLOCKED` | Medium | Low    | `CARRIER=... scripts/soak-test.sh c` 2026-09-16: kitty 12/12, foot 12/12, alacritty 13/13 (after fixing the `Alacritty` app_id casing bug, see CHANGELOG [0.6.2]); maintainer input still needed |
| Stable-toolchain CI leg (`cargo +stable build/test`) — the code must compile on stable Rust (a past release broke NixOS stable over a `let` chain), but CI builds nightly only                             | 🔴 `TODO`    | Medium | M      | `.github/workflows/checks.yml` installs nightly only; AGENTS.md stable-RUST constraint (verified 2026-09-26)                           |

## Low Impact

| Task                                                                                                                                        | Impact | Effort | Evidence                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------------------- | ------ | ------ | -------------------------------------------------------------------------------------------------------------- |
| Soak script: end-of-run carrier cleanup (trap/EXIT), not only at the start of the next run                                                   | Low    | S      | `scripts/soak-test.sh` has start-of-run cleanup only (verified 2026-09-26; leftover carriers were hand-closed late once) |
| Promote the display-width markdown-table realigner into `scripts/` (one-off heredocs rot)                                                    | Low    | S      | written for the 2026-09-16 markdownlint pass; `scripts/` holds only `docs-citations.sh` + `soak-test.sh`        |
| Actionlint as a CI step (currently devshell/manual only)                                                                                     | Low    | S      | no actionlint in `.github/workflows/*.yml` (verified 2026-09-26)                                             |
| Switch the CI clippy step to `--all-features --all-targets` so tests are linted in CI too (local gate is already clean)                       | Low    | S      | `.github/workflows/checks.yml:52` still runs the testless form (verified 2026-09-26)                          |
| Add `--locked` to CI `cargo build/test/clippy` to catch Cargo.lock drift                                                                     | Low    | S      | no `--locked` in `.github/workflows/checks.yml` (verified 2026-09-26)                                         |
| Add a `concurrency:` group to the workflow (cancel superseded runs)                                                                          | Low    | S      | no concurrency group in `.github/workflows/checks.yml` (verified 2026-09-26)                                  |
| Clap-level conflict tests for mode flags (`--protocol-probe` vs `--restore`/`--save-only`/...)                                               | Low    | S      | `conflicts_with` attrs exist (`src/config.rs:217-238`) but no test asserts the rejections (verified 2026-09-26) |
| Dedicated regression test for the reconnect-backoff reset (stream alive ≥5s resets to 1s)                                                    | Low    | S      | `reconnect_backoff_doubles_and_caps` pins the ramp, not the reset (verified 2026-09-26)                       |
| Widen the embedded-template test beyond the `Alacritty` pins to full `DEFAULT_APP_CONFIG_TOML` ↔ `AppConfig` sync                            | Low    | S      | `src/tests.rs:629` asserts one mapping; no full-shape sync test                                                |
| Dedicated test for the `--health-check` layout-coverage line fields                                                                          | Low    | S      | line shipped (`src/main.rs:129`); health tests assert pass/fail only, not the reported fields                  |
| Config-version hint when shipped defaults change (existing config.toml files missed the `Alacritty` mapping until hand-edited)                | Low    | S      | CHANGELOG [0.6.2] Fixed: "existing config.toml files must add the entry by hand"                               |
| Fixture-refresh procedure in AGENTS (probe → capture fixture → pin bump) for when niri adds event variants                                    | Low    | S      | AGENTS.md says "re-verify against `src/testdata/` fixtures" but not the capture-refresh procedure               |
| CHANGELOG compare-links footer (`[0.6.2]: .../compare/...`) for Keep-a-Changelog completeness                                                | Low    | S      | no link definitions at the foot of `CHANGELOG.md` (verified 2026-09-26)                                        |
| Quarterly dated re-triage reminder for the accepted vulnix build-time advisories                                                            | Low    | S      | AGENTS.md Known Issues records the acceptance without a re-check cadence                                       |
| module.nix comment documenting the 6-of-7 tunable mirror and why `dryRun` is CLI-only                                                        | Low    | S      | no such comment in `module.nix` (verified 2026-09-26)                                                          |

---

_Verified 2026-09-26 against code at 139 passing tests (+1 ignored benchmark, full suite green). **The v0.6.2 deploy chain is closed:** tag `v0.6.2` pushed (`89a3c6d`), SystemNix re-pinned, and the deployed service runs `niri-session-manager-0.6.2` with a fresh `session.json` (mtime current at verification; multi-day backup trail since the ~2026-09-25 restart) — the 08:25 v0.6.0 deploy alert from 2026-09-16 is resolved. CI is green on `main` and on the tag (run `35101221728`), and the drift-guard canary's first weekly scheduled run went green 2026-09-21 (run `35559642666`) after the resolver fix (`348d5a8`). `/mnt/buildcache` is back to 51%. Remaining user-side questions live in ROADMAP Q3 (daily-driver terminals) and Q5 (terminal close-after-exit)._

_History: the 2026-09-16 repo-tail batch (`--protocol-probe`, staleness warning, flapping summary, drift-guard canary, markdownlint enforcement, wire-format/fuzz/pin tests, `--save-once` e2e, foot/alacritty carrier coverage + the `Alacritty` casing fix) shipped in [0.6.2]; the 2026-09-15 rounds shipped [0.6.0] (final focus pass, marker pruning, layout capture v5, retry backoff, module split) and the first real CI run. See `CHANGELOG.md` for the full chain._
