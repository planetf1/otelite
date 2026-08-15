args@{
  pkgs,
  otelite,
  oteliteWorkspace,
  ...
}:
{
  inherit otelite;
  otelite-workspace = oteliteWorkspace;
  nixfmt = pkgs.runCommand "otelite-nixfmt-check" { nativeBuildInputs = [ pkgs.nixfmt-tree ]; } ''
    cp -r ${../..} source
    chmod -R u+w source
    cd source
    treefmt --ci
    touch $out
  '';
}
// pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
  nixos-module = import ./nixos-module.nix args;
}
// pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
  darwin-module = import ./darwin-module.nix args;
}
