# The CLI and the zellij plugin, built with the same Rust as the devShell.
{ pkgs }:
let
  toolchain = pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;
  rustPlatform = pkgs.makeRustPlatform { cargo = toolchain; rustc = toolchain; };
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).workspace.package.version;
  src = pkgs.lib.cleanSource ../.;
  cargoLock.lockFile = ../Cargo.lock;
in
{
  # `-p warpify-cli` leaves out the openssl-needing dev-dependency of warpify-proto.
  warpify = rustPlatform.buildRustPackage {
    pname = "warpify";
    inherit version src cargoLock;
    cargoBuildFlags = [ "-p" "warpify-cli" ];
    doCheck = false;
    meta.mainProgram = "warpify";
  };

  warpify-zellij = pkgs.stdenv.mkDerivation {
    pname = "warpify-zellij";
    inherit version src;
    cargoDeps = rustPlatform.importCargoLock cargoLock;
    nativeBuildInputs = [ rustPlatform.cargoSetupHook toolchain ];
    buildPhase = ''
      runHook preBuild
      cargo build -p warpify-plugin --target wasm32-wasip1 --release --offline
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      install -Dm644 target/wasm32-wasip1/release/warpify-zellij.wasm $out/lib/warpify-zellij.wasm
      runHook postInstall
    '';
  };
}
