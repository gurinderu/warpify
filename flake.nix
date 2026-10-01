{
  description = "warpify — zellij plugin and CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # master tracks nixos-unstable, like the nixpkgs input above; the module only touches
    # long-stable options (xdg.dataFile, home.packages, home.activation, programs.zellij.settings).
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, home-manager, rust-overlay, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      linux = [ "x86_64-linux" "aarch64-linux" ];
      forSystems = list: f: nixpkgs.lib.genAttrs list (system:
        f (import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; }));
      forAllSystems = forSystems systems;
      packagesFor = pkgs: import ./nix/packages.nix { inherit pkgs; };
      module = import ./nix/home-manager.nix { inherit self; };
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

      # warpify: the CLI; warpify-zellij: the plugin, as $out/lib/warpify-zellij.wasm.
      packages = forAllSystems (pkgs:
        let p = packagesFor pkgs; in p // { default = p.warpify; });

      homeManagerModules = {
        default = module;
        warpify = module;
      };

      checks = forSystems linux (pkgs:
        import ./nix/checks.nix {
          inherit pkgs home-manager;
          packages = self.packages.${pkgs.stdenv.hostPlatform.system};
          module = self.homeManagerModules.default;
        });
    };
}
