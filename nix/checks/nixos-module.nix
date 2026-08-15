{
  inputs,
  system,
  pkgs,
  lib,
  ...
}:
let
  inherit (import ./lib.nix { inherit pkgs lib; }) mkEvalCheck evaluationFails;
  nixosModuleCheck =
    let
      nixosModule = import ../nixos-module.nix;
      evaluate =
        module:
        inputs.nixpkgs.lib.nixosSystem {
          inherit system;
          modules = [
            nixosModule
            {
              system.stateVersion = "26.05";
              fileSystems."/" = {
                device = "/dev/disk";
                fsType = "ext4";
              };
              boot.loader.grub.devices = [ "nodev" ];
            }
            module
          ];
        };
      default = (evaluate { services.otelite.enable = true; }).config;
      custom =
        (evaluate {
          users.groups.nginx = { };
          users.users.nginx = {
            isSystemUser = true;
            group = "nginx";
            home = "/var/empty";
            description = "pre-existing account";
          };
          services.otelite = {
            enable = true;
            user = "nginx";
            group = "nginx";
            dataDir = "/srv/otelite";
            openFirewall = true;
          };
        }).config;
      managedCustom =
        (evaluate {
          services.otelite = {
            enable = true;
            createUser = true;
            user = "otelite-custom";
            group = "otelite-custom";
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
    mkEvalCheck "otelite-nixos-module-eval" [
      (builtins.isString default.system.build.toplevel.drvPath)
      default.services.otelite.createUser
      (default.users.users.otelite.home == "/var/lib/otelite")
      (!default.users.users.otelite.createHome)
      (default.systemd.services.otelite.serviceConfig.StateDirectory == "otelite")
      (default.systemd.services.otelite.serviceConfig.StateDirectoryMode == "0750")
      (default.systemd.tmpfiles.settings.otelite or { } == { })
      (!custom.services.otelite.createUser)
      (builtins.hasAttr "nginx" custom.users.users)
      (builtins.hasAttr "nginx" custom.users.groups)
      (!builtins.hasAttr "otelite" custom.users.users)
      (!builtins.hasAttr "otelite" custom.users.groups)
      (custom.users.users.nginx.home == "/var/empty")
      (custom.users.users.nginx.description == "pre-existing account")
      ((custom.systemd.services.otelite.serviceConfig.StateDirectory or null) == null)
      (custom.systemd.tmpfiles.settings.otelite."/srv/otelite".d.user == "nginx")
      (builtins.all (port: builtins.elem port custom.networking.firewall.allowedTCPPorts) [
        3000
        4317
        4318
      ])
      managedCustom.services.otelite.createUser
      (managedCustom.users.users."otelite-custom".group == "otelite-custom")
      (builtins.hasAttr "otelite-custom" managedCustom.users.groups)
      (lib.hasInfix "/bin/otelite\" \"serve\"" default.systemd.services.otelite.serviceConfig.ExecStart)
      (!lib.hasInfix "\"start\"" default.systemd.services.otelite.serviceConfig.ExecStart)
      (default.systemd.services.otelite.environment.HOME == "/var/lib/otelite")
      default.systemd.services.otelite.serviceConfig.NoNewPrivileges
      (lib.hasInfix "[::1]:4000" ipv6.systemd.services.otelite.serviceConfig.ExecStart)

      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          address = "localhost";
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          extraArgs = [ "--addr" ];
        };
      }))
      (evaluationFails (evaluate {
        services.otelite = {
          enable = true;
          extraArgs = [ "--storage-path=/tmp/otelite" ];
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
          dataDir = "/srv/../run/otelite";
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
          dataDir = "/home/otelite";
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
          port = 4317;
        };
      }))
    ];
in
nixosModuleCheck
