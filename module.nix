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
      partOf = [ "graphical-session.target" ];
      wants = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      serviceConfig = {
        Type = "simple";
        Restart = "always";
        ExecStart = "${getExe cfg.package}";
        PrivateTmp = true;
      };
    };
  };
}
