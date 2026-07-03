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
    ++ lib.optional (cfg.maxBackupCount != null)
      "--max-backup-count ${toString cfg.maxBackupCount}"
    ++ lib.optional (cfg.spawnTimeout != null) "--spawn-timeout ${toString cfg.spawnTimeout}"
    ++ lib.optional (cfg.retryAttempts != null)
      "--retry-attempts ${toString cfg.retryAttempts}"
    ++ lib.optional (cfg.retryDelay != null) "--retry-delay ${toString cfg.retryDelay}"
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
  };
}
