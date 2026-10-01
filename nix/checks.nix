# Builds both packages and a minimal home-manager configuration using the module.
{ pkgs, home-manager, packages, module }:
let
  inherit (pkgs) lib;
  # The path `warpify install` uses on this system (BaseDirs::data_dir): the XDG data dir, or
  # ~/Library/Application Support on macOS.
  rel =
    if pkgs.stdenv.hostPlatform.isDarwin
    then "Library/Application Support/warpify/warpify-zellij.wasm"
    else ".local/share/warpify/warpify-zellij.wasm";
  abs = "/home/tester/${rel}";
  hm = home-manager.lib.homeManagerConfiguration {
    inherit pkgs;
    modules = [
      module
      {
        home = {
          username = "tester";
          homeDirectory = "/home/tester";
          stateVersion = "25.11";
        };
        programs.zellij.enable = true;
        programs.warpify.enable = true;
      }
    ];
  };
in
{
  inherit (packages) warpify warpify-zellij;

  home-manager-module = pkgs.runCommand "warpify-hm-module-check" { } ''
    files=${hm.config.home-files}
    cfg=$files/.config/zellij/config.kdl
    # the zellij in nixpkgs matches the plugin's zellij-tile: no version warning
    ${if hm.config.warnings == [ ] then "" else "echo ${lib.escapeShellArg (toString hm.config.warnings)}; exit 1"}
    cat "$cfg"
    grep -F '"zellij:link"' "$cfg"
    grep -F ${lib.escapeShellArg ''"file:${abs}"''} "$cfg"
    test "$(readlink ${lib.escapeShellArg "${hm.config.home-files}/${rel}"})" = \
      ${packages.warpify-zellij}/lib/warpify-zellij.wasm
    grep -F -- ${lib.escapeShellArg "__grant-permissions zellij --wasm-path ${lib.escapeShellArg abs}"} \
      ${hm.activationPackage}/activate
    # a failed grant must warn, not break the switch
    grep -F "zellij will ask on first load" ${hm.activationPackage}/activate
    # the other OS's location is not used
    test ! -e ${lib.escapeShellArg "${hm.config.home-files}/${if pkgs.stdenv.hostPlatform.isDarwin then ".local/share/warpify" else "Library"}"}
    touch $out
  '';
}
