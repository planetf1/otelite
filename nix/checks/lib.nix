{ pkgs, lib }:
{
  mkEvalCheck =
    name: assertions:
    let
      failures = lib.pipe assertions [
        (lib.imap0 (
          index: assertion: {
            inherit assertion index;
          }
        ))
        (builtins.filter (entry: !entry.assertion))
        (map (entry: toString entry.index))
      ];
    in
    assert lib.assertMsg (
      failures == [ ]
    ) "${name} failed assertions: ${lib.concatStringsSep ", " failures}";
    pkgs.runCommand name { } "touch $out";

  evaluationFails =
    systemConfig: !(builtins.tryEval systemConfig.config.system.build.toplevel.drvPath).success;
}
