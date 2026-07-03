{
  config,
  lib,
  ...
}:
let
  inherit (lib)
    mkEnableOption
    mkPackageOption
    mkIf
    getExe
    ;
  cfg = config.services.niri-session-manager;
in
{
  options = {
    services.niri-session-manager = {
      enable = mkEnableOption "Niri Session Manager";
      package = mkPackageOption { } "Niri Session Manager" {
        nullable = true;
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
        ExecStart = "${getExe cfg.package}";
        Restart = "always";
        RestartSec = "2s";
        PrivateTmp = true;
        OOMScoreAdjust = -500;
      };
    };
  };
}
