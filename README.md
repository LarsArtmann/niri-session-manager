# Niri Session Manager

[![Checks](https://github.com/LarsArtmann/niri-session-manager/actions/workflows/checks.yml/badge.svg)](https://github.com/LarsArtmann/niri-session-manager/actions/workflows/checks.yml)

A session manager for the Niri Wayland compositor that automatically saves and restores your window layout across compositor restarts.

## Features

### Core

- **Reactive session saving** — subscribes to niri's event stream and saves shortly after layout activity settles (debounced), with an interval fallback when the stream is unavailable
- **Idempotent session restoration** — re-running a restore spawns only what is missing: windows are matched against running instances by workspace first, then capped at `saved − running` per app, so you never get duplicates
- **Per-app spawn serialization** — two windows of the same app are spawned one after another, eliminating the workspace-swap race
- **Focus restoration** — the window that was focused when the session was saved gets focus back
- **Terminal state recovery** — restores running commands inside terminals (e.g. `btop`, `nvim`, `ssh`) via `/proc` PID resolution, including the working directory
- **Backup management** with configurable retention and corrupt-file protection
- **Corrupted session recovery** — automatically falls back to the most recent valid `.bak` if `session.json` is corrupt
- **Dry-run mode** — preview what would be restored without spawning anything
- **Save on suspend** — a `sleep.target` hook saves the session right before the machine sleeps

### Reliability

- **Atomic session writes** (temp + fsync + rename + parent-directory fsync) survive crashes and power loss
- **Unchanged saves are skipped** — an idle desktop produces no backup churn and no journal noise
- **Rate-limited spawning** — max 5 concurrent spawns so niri IPC is never overwhelmed
- **Non-fatal restore** — if niri IPC isn't ready yet, logs the error and continues instead of crash-looping
- **Config validation** at startup with clear error messages
- **Structured logging** via `tracing` — journald-native output with timestamps and log levels (control verbosity with `RUST_LOG`)
- **Supply-chain checks** — cargo-deny (advisories, licenses, bans) and cargo audit in CI

### Behavior Notes

- **One restore per boot.** Restore is gated by a boot id (`/proc/sys/kernel/random/boot_id`) plus a `restore-marker` file next to `session.json`. Reboots always restore; restarting the service within the same boot does not re-spawn windows. Stale markers from previous boots are pruned automatically.
- **Dry-run changes nothing.** `--dry-run` prints what would be restored — it never spawns windows, never writes `session.json`, and never writes the restore marker.
- **Retries are within one restore.** `--retry-attempts` controls how often a failing restore retries before giving up (non-fatally); it does not re-attempt across service restarts within the same boot.
- **Restore is idempotent.** When M of N saved windows for an app are already running, restore matches them by workspace (name first, then index) and spawns at most `N − M` — a partial restore resumes instead of duplicating. Single-instance apps stay skipped while any instance runs.
- **Unchanged sessions are not re-written.** If the captured layout is byte-identical to the file on disk, neither the backup rotation nor the write happens.

## Usage

```bash
niri-session-manager [OPTIONS]
```

### CLI Options

```
--save-interval <MINUTES>     Fallback save interval in minutes (default: 15)
--max-backup-count <COUNT>    Number of backup files to keep (default: 5)
--spawn-timeout <SECONDS>     How long to wait for windows to spawn (default: 5)
--retry-attempts <COUNT>      Number of restore attempts (default: 3)
--retry-delay <SECONDS>       Delay between retry attempts (default: 2)
--max-restore-windows <N>     Sanity cap on windows a single restore may spawn (default: 100)
--dry-run                     Preview restore without spawning or saving
--config-file <PATH>          Override the app-config path (default: XDG config, config.toml)
--restore                     Restore the saved session, then exit
--save-only                   Skip the boot restore and only run the save loop
--save-once                   Save the current session once, then exit (used by the suspend hook)
--health-check                Report niri reachability, boot-gate state, and session-file status, then exit
--export <DIR>                Copy session.json plus all backups into DIR, then exit
--import <DIR>                Validate and import session.json (+ backups) from DIR, backing up the current session first
```

### NixOS Module Options

Tunable CLI options are mirrored as NixOS module options (`--dry-run` stays CLI-only by design):

```nix
services.niri-session-manager = {
  enable = true;
  saveInterval = 30;          # minutes (fallback save cadence)
  maxBackupCount = 3;
  spawnTimeout = 10;          # seconds
  retryAttempts = 5;
  retryDelay = 3;             # seconds
  maxRestoreWindows = 100;
  saveOnSuspend = true;       # sleep.target hook running --save-once
};
```

## Configuration

Configuration file location: `$XDG_CONFIG_HOME/niri-session-manager/config.toml`

```toml
# Apps that should only have one instance
[single_instance_apps]
apps = ["firefox", "zen"]

# Applications to skip during restore
[skip_apps]
apps = ["discord"]

# Map niri app IDs to actual launch commands
[app_mappings]
"vesktop" = ["flatpak", "run", "dev.vencord.Vesktop"]
"com.mitchellh.ghostty" = ["ghostty"]
"signal" = ["signal-desktop"]

# Terminal state recovery — restore running commands inside terminals
[terminal_state]
enabled = true
terminal_app_ids = ["kitty", "foot", "org.wezfurlong.wezterm", "com.mitchellh.ghostty", "alacritty"]
shell_names = ["fish", "bash", "zsh", "sh", "dash", "-fish", "-bash", "-zsh", "-sh", "sudo", "doas"]
helper_names = ["kitten"]
max_walk_depth = 20
```

If no configuration file exists, one will be created with example mappings.

### Terminal State Recovery

When enabled, the session manager walks the process tree of terminal windows via `/proc` to find foreground child processes (skipping shells like `fish`, `bash`, `zsh`). On restore, it re-launches the terminal with the original command and working directory.

For example, if `kitty` was running `btop` in `/home/user/projects`, the restored command becomes:

```bash
kitty --directory /home/user/projects sh -c "'btop'; exec $SHELL"
```

Terminal-specific flags are handled automatically and are verified against the official CLI documentation:

- **kitty**: `--directory`, positional command
- **foot**: `--working-directory`, positional command
- **wezterm**: `start --cwd ... -- sh -c ...`
- **ghostty**: `--working-directory=...`, `-e sh -c ...`
- **alacritty**: `--working-directory`, `-e sh -c ...`

This feature is Linux-only.

## Installation

### Using Nix Flakes

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    niri-session-manager.url = "github:LarsArtmann/niri-session-manager";
  };
  outputs = { self, nixpkgs, niri-session-manager, ... }: {
    nixosConfigurations.yourHost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        niri-session-manager.nixosModules.niri-session-manager
        {
          services.niri-session-manager.enable = true;
          # Optional overrides:
          # services.niri-session-manager.saveInterval = 30;
        }
      ];
    };
  };
}
```

The systemd user service is automatically configured to:

- Start after `niri.service` and `graphical-session.target`
- Restart with 2s delay and rate limiting (5 bursts per 60s)
- Use OOM score adjustment to avoid being killed first under memory pressure
- Save the session before suspend (`saveOnSuspend`, a `sleep.target` oneshot)

## Storage

- **Session file**: `$XDG_DATA_HOME/niri-session-manager/session.json`
- **Backups**: `$XDG_DATA_HOME/niri-session-manager/session-{timestamp}.bak`
- **Configuration**: `$XDG_CONFIG_HOME/niri-session-manager/config.toml`

The session format is versioned (currently v4; the version is descriptive, not enforced). Legacy formats are auto-detected via serde aliases and migrated on the next save. See `docs/example-session.json`.

## Development

```bash
cargo build                      # build
cargo test                       # run test suite (unit + fake-IPC integration tests)
cargo clippy --all-features      # lint (CI runs this exact form)
cargo fmt --all -- --check       # format check
nix build .#niri-session-manager # nix build
nix flake check                  # nix checks (includes treefmt)
bash scripts/docs-citations.sh   # verify docs citations and links
```

See `CONTRIBUTING.md` for the ground rules.
