{
  inputs,
  system,
  pkgs,
  lib,
  ...
}:
let
  inherit (import ./lib.nix { inherit pkgs lib; }) mkEvalCheck evaluationFails;
  darwinModuleCheck =
    let
      darwinModule = import ../darwin-module.nix;
      evaluate =
        module:
        inputs.nix-darwin.lib.darwinSystem {
          modules = [
            darwinModule
            {
              system.stateVersion = 6;
              nixpkgs.hostPlatform = system;
            }
            module
          ];
        };
      default = (evaluate { services.otelite.enable = true; }).config;
      defaultPlist = default.environment.launchDaemons."org.nixos.otelite.plist".text;
      custom =
        (evaluate {
          users.groups.nginx = {
            gid = 600;
            description = "pre-existing group";
          };
          users.users.nginx = {
            uid = 600;
            gid = 600;
            home = "/var/empty";
            description = "pre-existing account";
          };
          services.otelite = {
            enable = true;
            user = "nginx";
            group = "nginx";
            dataDir = "/Library/Application Support/otelite-custom";
          };
        }).config;
      managedCustom =
        (evaluate {
          services.otelite = {
            enable = true;
            createUser = true;
            user = "_otelite_custom";
            group = "_otelite_custom";
            uid = 700;
            gid = 701;
          };
        }).config;
      ipv6 =
        (evaluate {
          services.otelite = {
            enable = true;
            address = "[::1]";
            port = 4000;
          };
        }).config;
    in
    mkEvalCheck "otelite-darwin-module-eval" [
      (builtins.isString default.system.build.toplevel.drvPath)
      default.services.otelite.createUser
      (default.users.users._otelite.uid == 536)
      (default.users.groups._otelite.gid == 536)
      (builtins.elem "_otelite" default.users.knownUsers)
      (builtins.elem "_otelite" default.users.knownGroups)
      (lib.hasInfix "uid %s is already used" default.system.checks.text)
      (lib.hasInfix "gid %s is already used" default.system.checks.text)
      (default.launchd.daemons.otelite.serviceConfig.StandardOutPath == null)
      (default.launchd.daemons.otelite.serviceConfig.StandardErrorPath == null)
      (!lib.hasInfix "<key>StandardOutPath</key>" defaultPlist)
      (!lib.hasInfix "<key>StandardErrorPath</key>" defaultPlist)
      (!lib.hasInfix "otelite.log" default.system.activationScripts.launchd.text)
      (!lib.hasInfix "otelite.error.log" default.system.activationScripts.launchd.text)
      (!lib.hasInfix "touch" default.system.activationScripts.launchd.text)
      (!lib.hasInfix "chown" default.system.activationScripts.launchd.text)
      (!lib.hasInfix "chmod" default.system.activationScripts.launchd.text)
      (lib.hasInfix "/usr/bin/dscl /Search -search /Users UniqueID" default.system.checks.text)
      (lib.hasInfix "/usr/bin/dscl /Search -search /Groups PrimaryGroupID" default.system.checks.text)
      (lib.hasInfix "uid collision lookup failed" default.system.checks.text)
      (lib.hasInfix "gid collision lookup failed" default.system.checks.text)
      (!lib.hasInfix "otelite_uid_owner=$(dscl ." default.system.checks.text)
      (!custom.services.otelite.createUser)
      (builtins.hasAttr "nginx" custom.users.users)
      (builtins.hasAttr "nginx" custom.users.groups)
      (!builtins.hasAttr "_otelite" custom.users.users)
      (!builtins.hasAttr "_otelite" custom.users.groups)
      (custom.users.users.nginx.home == "/var/empty")
      (custom.users.users.nginx.description == "pre-existing account")
      (custom.users.groups.nginx.description == "pre-existing group")
      (!builtins.elem "nginx" custom.users.knownUsers)
      (!builtins.elem "nginx" custom.users.knownGroups)
      (!lib.hasInfix "otelite_user='nginx'" custom.system.checks.text)
      (!lib.hasInfix "/usr/bin/dscl /Search" custom.system.checks.text)
      (custom.launchd.daemons.otelite.serviceConfig.StandardOutPath == null)
      (custom.launchd.daemons.otelite.serviceConfig.StandardErrorPath == null)
      (!lib.hasInfix "otelite.log" custom.system.activationScripts.launchd.text)
      (!lib.hasInfix "otelite.error.log" custom.system.activationScripts.launchd.text)

      managedCustom.services.otelite.createUser
      (managedCustom.users.users._otelite_custom.uid == 700)
      (managedCustom.users.users._otelite_custom.gid == 701)
      managedCustom.users.users._otelite_custom.createHome
      (managedCustom.users.groups._otelite_custom.gid == 701)
      (builtins.elem "_otelite_custom" managedCustom.users.knownUsers)
      (builtins.elem "_otelite_custom" managedCustom.users.knownGroups)
      (default.launchd.daemons.otelite.environment.HOME == "/var/lib/otelite")
      (
        default.launchd.daemons.otelite.serviceConfig.ProgramArguments == [
          (lib.getExe default.services.otelite.package)
          "serve"
          "--addr"
          "127.0.0.1:3000"
          "--storage-path"
          "/var/lib/otelite"
        ]
      )
      (
        ipv6.launchd.daemons.otelite.serviceConfig.ProgramArguments == [
          (lib.getExe ipv6.services.otelite.package)
          "serve"
          "--addr"
          "[::1]:4000"
          "--storage-path"
          "/var/lib/otelite"
        ]
      )
      default.launchd.daemons.otelite.serviceConfig.RunAtLoad
      default.launchd.daemons.otelite.serviceConfig.KeepAlive
      (evaluationFails (evaluate {
        users.users.conflict = {
          uid = 536;
          gid = 600;
        };
        users.groups.conflict.gid = 600;
        services.otelite.enable = true;
      }))
      (evaluationFails (evaluate {
        users.users.conflict = {
          uid = 600;
          gid = 536;
        };
        users.groups.conflict.gid = 536;
        services.otelite.enable = true;
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          address = "localhost";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          extraArgs = [ "--addr=0.0.0.0:3000" ];
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          extraArgs = [ "--storage-path" ];
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          dataDir = "relative/state";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          dataDir = "/tmp/otelite";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          dataDir = "/var/tmp/otelite";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          dataDir = "/Library/../private/tmp/otelite";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          dataDir = "/private/tmp/otelite";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          dataDir = "/private/var/tmp/otelite";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          dataDir = "/run/otelite";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          dataDir = "/var/run/otelite";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          dataDir = "/private/var/run/otelite";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          user = "root";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          group = "wheel";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          port = 0;
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          port = 4318;
        };
      }))
    ];
in
darwinModuleCheck
