{
  description = "warpify — zellij plugin and CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, rust-overlay, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system:
        f (import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; }));
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          # Toolchain comes from rust-toolchain.toml; mkShell's stdenv supplies the C linker
          # that dependency build scripts need.
          # openssl + pkg-config: zellij-utils (dev-dependency of warpify-proto, for the
          # permission-names test) pulls curl/openssl-sys on the host.
          packages = [
            pkgs.openssl
            pkgs.pkg-config
            ((pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
              extensions = [ "clippy" "rustfmt" "rust-src" ];
            })
          ];
        };
      });
    };
}
