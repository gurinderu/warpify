# Builds both packages and a minimal home-manager configuration using the module.
{ pkgs, home-manager, packages, module }:
let
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
    cat "$cfg"
    grep -F '"zellij:link"' "$cfg"
    grep -F '"file:/home/tester/.local/share/warpify/warpify-zellij.wasm"' "$cfg"
    test "$(readlink "$files/.local/share/warpify/warpify-zellij.wasm")" = \
      ${packages.warpify-zellij}/lib/warpify-zellij.wasm
    grep -F -- '__grant-permissions zellij --wasm-path /home/tester/.local/share/warpify/warpify-zellij.wasm' \
      ${hm.activationPackage}/activate
    touch $out
  '';
}
