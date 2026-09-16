{ pkgs }:
pkgs.mkShell {
  packages = builtins.attrValues {
    inherit (pkgs) # nix formatters
      nixfmt-rfc-style
      statix
      # docs
      markdownlint-cli
      # rust
      rustc
      cargo
      clippy
      rust-analyzer
      rustfmt
      cargo-deny
      cargo-audit
      ;
  };
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
}
