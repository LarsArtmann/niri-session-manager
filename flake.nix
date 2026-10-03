{

  description = "niri-session-manager";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    treefmt-nix.url = "github:numtide/treefmt-nix";
  };

  outputs =
    {
      nixpkgs,
      treefmt-nix,
      self,
    }:
    let
      # Inline systems: nixpkgs 26.11 dropped x86_64-darwin, which github:nix-systems/default still lists.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems =
        function: nixpkgs.lib.genAttrs systems (system: function nixpkgs.legacyPackages.${system});
      treefmtEval = forAllSystems (
        pkgs:
        treefmt-nix.lib.evalModule pkgs (_: {
          programs = {
            nixfmt.enable = true;
            statix.enable = true;
          };
          projectRootFile = "flake.nix";
        })
      );
      getPlatform = p: p.stdenv.hostPlatform.system;
    in
    {
      formatter = forAllSystems (pkgs: treefmtEval.${getPlatform pkgs}.config.build.wrapper);

      checks = forAllSystems (pkgs: {
        formatting = treefmtEval.${getPlatform pkgs}.config.build.check self;
      });

      nixosModules = {
        niri-session-manager =
          { pkgs, ... }:
          {
            imports = [
              ./module.nix
            ];
            services.niri-session-manager.package = self.packages.${getPlatform pkgs}.niri-session-manager;
          };
      };

      packages = forAllSystems (pkgs: {
        default = self.packages.${getPlatform pkgs}.niri-session-manager;
        niri-session-manager = pkgs.rustPlatform.callPackage ./default.nix { };
      });

      devShells = forAllSystems (pkgs: {
        default = import ./shell.nix { inherit pkgs; };
      });

      overlays.niri-session-manager = _final: prev: {
        inherit (self.packages.${prev.stdenv.hostPlatform.system}) niri-session-manager;
      };
    };
}
