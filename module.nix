{
  config,
  lib,
  ...
}:
let
  inherit (lib)
    mkEnableOption
    mkPackageOption
    mkOption
    mkIf
    types
    getExe
    ;
  cfg = config.services.niri-session-manager;

  cliArgs = lib.concatStringsSep " " (
    lib.optional (cfg.saveInterval != null) "--save-interval ${toString cfg.saveInterval}"
    ++ lib.optional (cfg.maxBackupCount != null) "--max-backup-count ${toString cfg.maxBackupCount}"
    ++ lib.optional (cfg.spawnTimeout != null) "--spawn-timeout ${toString cfg.spawnTimeout}"
    ++ lib.optional (cfg.retryAttempts != null) "--retry-attempts ${toString cfg.retryAttempts}"
    ++ lib.optional (cfg.retryDelay != null) "--retry-delay ${toString cfg.retryDelay}"
    ++ lib.optional (
      cfg.maxRestoreWindows != null
    ) "--max-restore-windows ${toString cfg.maxRestoreWindows}"
  );
in
{
  options = {
    services.niri-session-manager = {
      enable = mkEnableOption "Niri Session Manager";
      package = mkPackageOption { } "Niri Session Manager" {
        nullable = true;
      };

      saveInterval = mkOption {
        type = types.nullOr types.ints.positive;
        default = null;
        description = "Session save interval in minutes (default: 15).";
      };

      maxBackupCount = mkOption {
        type = types.nullOr types.ints.positive;
        default = null;
        description = "Maximum number of backup files to keep (default: 5).";
      };

      spawnTimeout = mkOption {
        type = types.nullOr types.ints.positive;
        default = null;
        description = "Timeout in seconds to wait for a spawned window to appear (default: 5).";
      };

      retryAttempts = mkOption {
        type = types.nullOr types.ints.positive;
        default = null;
        description = "Number of retry attempts for session restore (default: 3).";
      };

      retryDelay = mkOption {
        type = types.nullOr types.ints.positive;
        default = null;
        description = "Delay in seconds between retry attempts (default: 2).";
      };

      maxRestoreWindows = mkOption {
        type = types.nullOr types.ints.positive;
        default = null;
        description = "Sanity cap on how many windows a single restore may spawn (default: 100).";
      };

      saveOnSuspend = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Save the session once before the machine suspends
          (a `sleep.target` hook running `--save-once`).
        '';
      };
    };
  };
  config = mkIf cfg.enable {
    systemd.user.services.niri-session-manager = {
      enable = true;
      description = "Niri Session Manager";

      wantedBy = [ "graphical-session.target" ];
      partOf = [
        "graphical-session.target"
        "niri.service"
      ];
      after = [
        "graphical-session.target"
        "niri.service"
      ];
      requires = [ "niri.service" ];

      unitConfig = {
        StartLimitIntervalSec = 60;
        StartLimitBurst = 5;
      };

      serviceConfig = {
        Type = "simple";
        ExecStart = "${getExe cfg.package} ${cliArgs}";
        Restart = "always";
        RestartSec = "2s";
        PrivateTmp = true;
        OOMScoreAdjust = -500;
      };
    };

    systemd.user.services.niri-session-manager-suspend = lib.mkIf cfg.saveOnSuspend {
      enable = true;
      description = "Niri Session Manager - save session before suspend";
      wantedBy = [ "sleep.target" ];
      before = [ "sleep.target" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${getExe cfg.package} --save-once";
        # The niri IPC socket lives in the user session environment; give the
        # one-shot save a fair chance to finish before the machine sleeps.
        TimeoutStartSec = "15s";
        OOMScoreAdjust = -500;
      };
    };
  };
}
