# home-manager module: the warpify CLI plus the plugin, loaded by zellij and pre-granted its
# permissions (graph @nick/warpify, node #18).
{ self }:
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.warpify;
  inherit (pkgs.stdenv.hostPlatform) system;
  # zellij keys its permission cache by this path, so it stays put while the store path moves.
  wasm = "${config.xdg.dataHome}/warpify/warpify-zellij.wasm";
  # lib.hm.generators.toKDL writes attribute names verbatim: pre-quote them to get
  # `"file:/…" ` child nodes with no arguments.
  node = name: { "\"${name}\"" = { }; };
in
{
  options.programs.warpify = {
    enable = lib.mkEnableOption "warpify, the zellij plugin and CLI for binding clients to tabs";
    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${system}.warpify;
      defaultText = lib.literalExpression "warpify.packages.\${system}.warpify";
      description = "The warpify CLI.";
    };
    zellijPlugin = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${system}.warpify-zellij;
      defaultText = lib.literalExpression "warpify.packages.\${system}.warpify-zellij";
      description = "The zellij plugin; its `lib/warpify-zellij.wasm` is linked into the data directory.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.dataFile."warpify/warpify-zellij.wasm".source = "${cfg.zellijPlugin}/lib/warpify-zellij.wasm";

    # load_plugins replaces zellij's defaults, so zellij:link is repeated here.
    programs.zellij.settings.load_plugins = node "zellij:link" // node "file:${wasm}";

    home.activation.warpifyGrantPermissions = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      run ${cfg.package}/bin/warpify __grant-permissions zellij --wasm-path ${lib.escapeShellArg wasm}
    '';
  };
}
