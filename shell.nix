{ pkgs }:
pkgs.mkShell {
  packages = builtins.attrValues {
    inherit (pkgs) # nix formatters
      nixfmt-rfc-style
      statix
      # rust
      rustc
      cargo
      clippy
      rust-analyzer
      rustfmt
      ;
  };
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
}
