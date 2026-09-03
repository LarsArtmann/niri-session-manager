# Contributing to niri-session-manager

Thanks for helping make session restore trustworthy. This project's bar:
**restores correctly, never duplicates windows, never loses data, and every
claim is tested.**

## Environment

The project is Nix-first (NixOS module + flake). Dependencies and toolchains
come from the flake — no global installs.

```bash
direnv allow          # or: nix develop
cargo test            # full suite (unit + fake-IPC integration tests)
cargo clippy --all-features   # pedantic+nursery denies are enforced; keep it green
cargo fmt --all -- --check
nix build             # package builds
nix flake check       # flake + module + formatting
```

## Ground rules

1. **Tests are the contract.** Restore/save behavior changes need tests:
   pure logic gets unit tests, IPC paths get tests against the fake niri
   server in `src/fake_niri.rs` (it speaks the real protocol over a Unix
   socket — set `$NIRI_SOCKET` via `FakeNiri::env()`).
2. **Keep clippy green.** `[lints.clippy]` in `Cargo.toml` denies pedantic,
   nursery, `unwrap_used`, `panic`, `as_conversions`, and friends. Use
   `context`/`bail`, `saturating_*`, `try_from`, and `Option` combinators.
3. **Verify before you trust.** A green claim from an earlier session, a
   stale LSP diagnostic, or your own memory of "I already applied that" is
   not evidence. Re-run the gate. After scripted/bulk edits, assert the edit
   actually landed (grep the expected marker) — silently lost edits have
   happened here.
4. **Dry-run stays honest.** `--dry-run` must never spawn windows and never
   write files (including the restore marker). Regression tests guard this;
   keep them passing.
5. **Restore is idempotent by design.** Re-running a restore spawns only the
   deficit (`saved − running` per app, workspace-first matching). Don't
   regress this for convenience.
6. **Docs are load-bearing.** `README.md` sells the tool; `TODO_LIST.md`,
   `FEATURES.md`, `ROADMAP.md`, `CHANGELOG.md` track reality. If you cite a
   file:line in docs, `scripts/docs-citations.sh` (CI) will verify it —
   keep citations fresh.

## Single source of truth

- The version lives **only** in `Cargo.toml`; `default.nix` reads it.
- Terminal CLI profiles live **only** in `TerminalProfile` (`src/main.rs`);
  they are verified against the terminals' official docs.

## Releases

1. Update `CHANGELOG.md` (Keep a Changelog style; reference fix commits).
2. Bump `Cargo.toml` version.
3. `cargo test && cargo clippy --all-features && cargo fmt --all -- --check && nix build && nix flake check`
4. Tag `vX.Y.Z` (annotated) and push `main` + the tag.

## Commit style

First line under 72 characters, imperative, explains *why* it matters —
readable by someone who has never seen the codebase. Example:
`release: v0.4.1 — deliver 0.4.0 behavior fixes to flake consumers`.
