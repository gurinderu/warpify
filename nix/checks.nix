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
  mkHm = extra: home-manager.lib.homeManagerConfiguration {
    inherit pkgs;
    modules = [
      module
      {
        home = {
          username = "tester";
          homeDirectory = "/home/tester";
          stateVersion = "25.11";
        };
        programs.warpify.enable = true;
      }
      extra
    ];
  };
  hm = mkHm { programs.zellij.enable = true; };
  hmConnect = mkHm {
    programs.zellij.enable = true;
    programs.warpify = {
      onConnect = "new-tab";
      pin = true;
      title = {
        enable = true;
        prefix = "🟠 test";
      };
    };
  };
  # zellij disabled: home-manager writes no config.kdl, so the module must warn and not grant.
  hmNoZellij = mkHm { programs.zellij.enable = false; };
  zellijWarning = "programs.warpify needs programs.zellij.enable = true: home-manager only writes zellij's config.kdl (and so load_plugins) when zellij is enabled";
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

  home-manager-module-on-connect = pkgs.runCommand "warpify-hm-module-on-connect-check" { } ''
    cfg=${hmConnect.config.home-files}/.config/zellij/config.kdl
    cat "$cfg"
    # the defaults write no children; the options are children of our entry
    ! grep -F on_connect ${hm.config.home-files}/.config/zellij/config.kdl
    ! grep -F 'pin ' ${hm.config.home-files}/.config/zellij/config.kdl
    ! grep -F terminal_title ${hm.config.home-files}/.config/zellij/config.kdl
    kids=$(grep -F -A5 ${lib.escapeShellArg ''"file:${abs}"''} "$cfg")
    grep -F 'on_connect "new_tab"' <<<"$kids"
    grep -F 'pin "true"' <<<"$kids"
    grep -F 'terminal_title "true"' <<<"$kids"
    grep -F 'title_prefix "🟠 test"' <<<"$kids"
    ${if hmConnect.config.warnings == [ ] then "" else "echo ${lib.escapeShellArg (toString hmConnect.config.warnings)}; exit 1"}
    touch $out
  '';

  home-manager-module-no-zellij = pkgs.runCommand "warpify-hm-module-no-zellij-check" { } ''
    ${if builtins.elem zellijWarning hmNoZellij.config.warnings then "" else "echo ${lib.escapeShellArg (toString hmNoZellij.config.warnings)}; exit 1"}
    # no grant without a zellij config to load the plugin from
    ! grep -F __grant-permissions ${hmNoZellij.activationPackage}/activate
    touch $out
  '';
}
