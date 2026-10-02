# home-manager module: the warpify CLI plus the plugin, loaded by zellij and pre-granted its
# permissions (graph @nick/warpify, node #18).
{ self }:
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.warpify;
  inherit (pkgs.stdenv.hostPlatform) system isDarwin;
  # Where `warpify install` puts the plugin (`directories::BaseDirs::data_dir()` + warpify/): the
  # XDG data dir on Linux, ~/Library/Application Support on macOS. zellij keys its permission
  # cache by this path, so it stays put while the store path moves.
  wasm =
    if isDarwin
    then "${config.home.homeDirectory}/Library/Application Support/warpify/warpify-zellij.wasm"
    else "${config.xdg.dataHome}/warpify/warpify-zellij.wasm";
  # The zellij major.minor the plugin is built for: derived from Cargo.lock's zellij-tile at eval time.
  tileVersion =
    (lib.findFirst (p: p.name == "zellij-tile") null
      (builtins.fromTOML (builtins.readFile ./../Cargo.lock)).package).version;
  builtFor = lib.concatStringsSep "." (lib.take 2 (lib.splitString "." tileVersion));
  # lib.hm.generators.toKDL writes attribute names verbatim: pre-quote them to get
  # `"file:/…" ` child nodes; `children` become the plugin's configuration (graph @nick/warpify,
  # node #16: the plugin reads `on_connect` and `pin` in `load`).
  node = name: children: { "\"${name}\"" = children; };
  pluginConfig =
    lib.optionalAttrs (cfg.onConnect == "new-tab") { on_connect = "new_tab"; }
    // lib.optionalAttrs cfg.pin { pin = "true"; };
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
    onConnect = lib.mkOption {
      type = lib.types.enum [ "none" "new-tab" ];
      default = "none";
      description = ''
        What the plugin does for a client that connects: `new-tab` leaves the first client of a
        session on its tab and gives every later client a new tab of its own; `none` does nothing.
      '';
    };
    pin = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Keep a client bound on connect on its tab (needs `onConnect = \"new-tab\"`).";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    # force: an earlier `warpify install` copy must not make activation fail on clobber.
    xdg.dataFile."warpify/warpify-zellij.wasm" = lib.mkIf (!isDarwin) {
      source = "${cfg.zellijPlugin}/lib/warpify-zellij.wasm";
      force = true;
    };
    home.file."Library/Application Support/warpify/warpify-zellij.wasm" = lib.mkIf isDarwin {
      source = "${cfg.zellijPlugin}/lib/warpify-zellij.wasm";
      force = true;
    };

    warnings =
      lib.optional (!config.programs.zellij.enable)
        "programs.warpify needs programs.zellij.enable = true: home-manager only writes zellij's config.kdl (and so load_plugins) when zellij is enabled"
      ++ lib.optional
        (config.programs.zellij.enable
          && !(lib.hasPrefix "${builtFor}." config.programs.zellij.package.version))
        "warpify's zellij plugin is built for zellij ${builtFor}; programs.zellij.package is ${config.programs.zellij.package.version} — the plugin may not load"
      ++ lib.optional (cfg.pin && cfg.onConnect == "none")
        "programs.warpify.pin has no effect with programs.warpify.onConnect = \"none\"";

    # load_plugins replaces zellij's defaults, so zellij:link is repeated here.
    programs.zellij.settings.load_plugins = node "zellij:link" { } // node "file:${wasm}" pluginConfig;

    home.activation.warpifyGrantPermissions = lib.mkIf config.programs.zellij.enable (lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      # Never break `home-manager switch` over this: zellij asks on first load instead.
      run ${cfg.package}/bin/warpify __grant-permissions zellij --wasm-path ${lib.escapeShellArg wasm} \
        || warnEcho "warpify: couldn't grant the plugin's permissions (see above); zellij will ask on first load"
    '');
  };
}
