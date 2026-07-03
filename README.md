# Niri Session Manager

A session manager for the Niri Wayland compositor that automatically saves and restores your window layout across compositor restarts.

## Features

### Core
- **Periodic session saving** with configurable interval and atomic writes
- **Automatic session restoration** on startup with retry logic
- **Backup management** with configurable retention
- **Corrupted session recovery** — automatically falls back to most recent valid `.bak` if `session.json` is corrupt
- **Terminal state recovery** — restores running commands inside terminals (e.g. `btop`, `nvim`, `ssh`) via `/proc` PID resolution
- **Rate-limited window spawning** — max 5 concurrent spawns to avoid overwhelming niri IPC
- **Dry-run mode** — preview what would be restored without spawning anything

### Reliability
- **Atomic session writes** (temp + fsync + rename) prevent corruption on crash
- **Non-fatal restore** — if niri IPC isn't ready yet, logs the error and continues instead of crash-looping
- **Config validation** at startup with clear error messages
- **Structured logging** via `tracing` — journald-native output with timestamps and log levels (control verbosity with `RUST_LOG`)
- **Startup ordering** — restore completes before periodic save starts, preventing partial-state snapshots

## Usage

```bash
niri-session-manager [OPTIONS]
```

### CLI Options
```
--save-interval <MINUTES>     How often to save the session (default: 15)
--max-backup-count <COUNT>    Number of backup files to keep (default: 5)
--spawn-timeout <SECONDS>     How long to wait for windows to spawn (default: 5)
--retry-attempts <COUNT>      Number of restore attempts (default: 3)
--retry-delay <SECONDS>       Delay between retry attempts (default: 2)
--dry-run                     Preview restore without spawning or saving
```

### NixOS Module Options

All CLI options are also available as NixOS module options:

```nix
services.niri-session-manager = {
  enable = true;
  saveInterval = 30;       # minutes
  maxBackupCount = 3;
  spawnTimeout = 10;       # seconds
  retryAttempts = 5;
  retryDelay = 3;          # seconds
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

Terminal-specific flags are handled automatically:
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

## Storage

- **Session file**: `$XDG_DATA_HOME/niri-session-manager/session.json`
- **Backups**: `$XDG_DATA_HOME/niri-session-manager/session-{timestamp}.bak`
- **Configuration**: `$XDG_CONFIG_HOME/niri-session-manager/config.toml`

Session format is versioned (currently v3). Legacy formats are auto-detected and migrated on next save.

## Development

```bash
cargo build          # build
cargo test           # run 57 tests
cargo clippy         # lint
cargo fmt            # format
nix build .#niri-session-manager  # nix build
nix flake check                  # nix checks
```
