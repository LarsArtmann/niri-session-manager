# 2026-09-16 14:40 — BuildFlow triage + repo-tail batch (probe, hardening, alacritty bug)

Status report for the follow-up session to the v0.6.1 fix batch
(`2026-09-16_12-48_v0.6.1-fix-batch-green-soak-session-report.md`). Trigger:
the user pasted a BuildFlow run (34/39, 38 findings) with the standing
directive to execute and verify until done.

## a) What this session did

### BuildFlow findings triage (the pasted run)

- **shfmt/dprint repair fallout**: formatting-only (soak script + markdown
  reflow); no Cargo.lock/flake.lock/source changes. Re-verified gates green
  after.
- **shellcheck SC2046** in `scripts/soak-test.sh` (socket discovery): fixed
  with `find -print -quit`, re-verified with nixpkgs shellcheck (0 findings)
  after buildflow's own re-run reported a suspicious `filesScanned: 0`.
- **jscpd** (5 findings, proc.rs): confirmed all inside the documented
  test-fixture clone region — accepted, unchanged (AGENTS.md policy).
- **cargo-audit/deny "no such command"**: devshell-only tools, by design;
  covered by running `nix develop -c cargo deny check` — all green
  (advisories/bans/licenses/sources ok).
- **vulnix**: accepted build-time toolchain advisories (AGENTS.md), unchanged.

### Repo-tail batch (12-48 report section f, items 5–22)

All repo-owned items closed (17 rows annotated in the 12-48 report; see it
for per-item evidence). Highlights:

- `--protocol-probe` shipped and **live-verified** against the real niri
  (unstable 2026-08-02): 6 burst lines, no drift, exit 0.
- Health-check staleness warning (2 × save interval) + rate-limited
  flapping summary (one WARN per 10 stream deaths), both unit-tested.
- Drift-guard canary workflow (weekly, continue-on-error, repins newest
  niri-ipc); markdownlint enforced (CI step + devshell, repo lints clean
  after fixing 9 MD036s and realigning tables with display-width padding).
- Tests 131 → **139** (fuzz reader proptest, wire-format pins, socketpair
  round-trip, pin-the-pin, `--save-once` end-to-end). CI `expected=139`.
- Benchmark re-verified: 100.4 ms/window unchanged after the IPC rewrite.

### The alacritty bug (found by the new carrier coverage)

`CARRIER=alacritty` phase C failed twice for two real, distinct product bugs:

1. **Cold-start race (test-side)**: alacritty's first launch outlived the
   script's fixed 3s pre-capture sleep → capture missed the carrier. Fix:
   the script now waits app-specifically for the carrier window to appear.
2. **Product bug**: niri reports alacritty's app_id as `Alacritty`
   (capital), but the defaults only knew lowercase — terminal state was
   never captured, and restore used the app_id verbatim as the launch
   command (`Alacritty`, no such binary) → 5s spawn timeout, "Restored 0".
   Fix: both spellings in `default_terminal_app_ids` + `"Alacritty" =
   ["alacritty"]` in the default template. **Existing config.toml files
   need the entry added by hand** (noted in TODO_LIST + CHANGELOG).

Final carrier results: kitty 12/12, foot 12/12, alacritty 13/13.
wezterm not installed on this machine.

### Durability soak (local leg)

36 min `--save-only` against the live desktop: 9 event-driven saves,
backup rotation working, RSS flat (5.8 → 5.9 MB), zero WARN/unparsable
lines, clean SIGTERM → final save → "Shutdown complete" within 3s.
Evidence: `/tmp/nsm-durability/`.

## b) Gates at close

fmt clean · clippy `--all-features --all-targets` clean · **139 tests + 1
ignored** (six full-suite runs green today, 3+ on final code) · `nix build`
and `nix flake check` green · docs-citations green · markdownlint clean ·
actionlint clean (both workflows) · cargo-deny green via devshell ·
`--version` = 0.6.1.

## c) Open (unchanged, user-gated)

1. **Tag + push v0.6.1** (origin/main + tag; never pushed without explicit
   go). Note: [Unreleased] now carries post-0.6.1 work — tag as-is for the
   hotfix, or fold [Unreleased] into a 0.6.2 later.
2. **SystemNix re-pin + restart** (the deployed service still runs v0.6.0
   with a stale session.json since 08:25 — F1 recurrence).
3. `/mnt/buildcache` policy (now 91 % — someone freed ~20 G during this
   session; zram swap is unaffected by the old swapfile-emergency).
4. Terminal close-after-exit UX (ROADMAP open question 5).
5. wezterm carrier (blocked on install) + overnight deployed soak.
