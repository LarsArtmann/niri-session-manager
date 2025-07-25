{ buildRustPackage }:
buildRustPackage (
  finalAttrs:
  let
    cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
  in
  {
    pname = "niri-session-manager";
    src = ./.;
    inherit (cargoToml.package) version;

    cargoLock = {
      lockFile = ./Cargo.lock;
    };

    meta.mainProgram = "niri-session-manager";
  }
)
