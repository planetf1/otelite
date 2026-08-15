{
  config,
  lib,
  pkgs,
  utils,
  ...
}:
let
  cfg = config.services.otelite;
  dataDir = cfg.dataDir;
  dashboardSocket = "${cfg.address}:${toString cfg.port}";
  executable = lib.getExe cfg.package;
  ipv4Octet = "(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])";
  ipv6Match = builtins.match "[[]([0-9a-fA-F:]*)[]]" cfg.address;
  isValidAddress =
    builtins.match "${ipv4Octet}([.]${ipv4Octet}){3}" cfg.address != null
    || (
      ipv6Match != null
      && (builtins.tryEval (
        builtins.deepSeq (lib.network.ipv6.fromString (builtins.head ipv6Match)) true
      )).success
    );
  managedArgs = [
    "--addr"
    "--storage-path"
  ];
  isManagedArg =
    arg: builtins.any (managed: arg == managed || lib.hasPrefix "${managed}=" arg) managedArgs;
  pathComponents = lib.filter (component: component != "") (lib.splitString "/" dataDir);
  canonicalDataDir = "/${lib.concatStringsSep "/" pathComponents}";
  volatileDataRoots = [
    "/tmp"
    "/var/tmp"
    "/run"
    "/var/run"
  ];
  isVolatileDataDir = builtins.any (
    root: canonicalDataDir == root || lib.hasPrefix "${root}/" canonicalDataDir
  ) volatileDataRoots;
  isHomeDataDir =
    dataDir == "/home"
    || lib.hasPrefix "/home/" dataDir
    || dataDir == "/root"
    || lib.hasPrefix "/root/" dataDir
    || dataDir == "/run/user"
    || lib.hasPrefix "/run/user/" dataDir;
  fixedReceiverWarning = ''
    services.otelite: the otlp grpc and http receivers always bind all interfaces
    on tcp ports 4317 and 4318; address only configures the dashboard and api.
  '';
  usesDefaultStateDirectory = dataDir == "/var/lib/otelite";
in
{
  options.services.otelite = {
    enable = lib.mkEnableOption "the otelite telemetry receiver and dashboard";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "the otelite package to run.";
    };

    address = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      example = "0.0.0.0";
      description = ''
        numeric ip address for the dashboard and api. enclose an ipv6 address in
        brackets. this does not configure the otlp receivers, which always bind
        to all interfaces on tcp ports 4317 and 4318.
      '';
    };

    port = lib.mkOption {
      type = lib.types.ints.between 1 65535;
      default = 3000;
      example = 8080;
      description = "tcp port for the dashboard and api.";
    };

    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/otelite";
      example = "/srv/otelite";
      description = "absolute runtime directory containing otelite.db and the service home.";
    };

    user = lib.mkOption {
      type = lib.types.strMatching "[_a-zA-Z][_a-zA-Z0-9-]*";
      default = "otelite";
      description = ''
        unprivileged account that runs otelite. custom accounts are treated as
        operator-managed unless createUser is enabled.
      '';
    };

    group = lib.mkOption {
      type = lib.types.strMatching "[_a-zA-Z][_a-zA-Z0-9-]*";
      default = "otelite";
      description = ''
        group that owns otelite state. custom groups are treated as
        operator-managed unless createUser is enabled.
      '';
    };

    createUser = lib.mkOption {
      type = lib.types.bool;
      default = cfg.user == "otelite" && cfg.group == "otelite";
      defaultText = lib.literalExpression ''
        config.services.otelite.user == "otelite"
        && config.services.otelite.group == "otelite"
      '';
      description = ''
        whether to create and manage the configured user and group. this defaults
        to true only for the default otelite identity. when false, the user and
        group must already exist and the module leaves their attributes unchanged.
      '';
    };

    logLevel = lib.mkOption {
      type = lib.types.enum [
        "trace"
        "debug"
        "info"
        "warn"
        "error"
      ];
      default = "info";
      example = "debug";
      description = "rust log filter exported to the service as RUST_LOG.";
    };

    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "--log-format=json" ];
      description = ''
        additional arguments passed to otelite after managed serve arguments.
        each item is one literal argument without shell expansion. --addr and
        --storage-path are managed by this module and cannot be overridden.
      '';
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        open the dashboard port and fixed otlp tcp ports 4317 and 4318. the
        dashboard remains unreachable remotely when address is loopback; the
        otlp receivers always listen on all interfaces.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = isValidAddress;
        message = "services.otelite.address must be a numeric ipv4 address or bracketed ipv6 address";
      }
      {
        assertion = lib.hasPrefix "/" dataDir;
        message = "services.otelite.dataDir must be an absolute path";
      }
      {
        assertion = dataDir != "/";
        message = "services.otelite.dataDir must not be the filesystem root";
      }
      {
        assertion =
          dataDir == canonicalDataDir
          && !builtins.any (
            component:
            builtins.elem component [
              "."
              ".."
            ]
          ) pathComponents;
        message = "services.otelite.dataDir must be a canonical absolute path without . or .. components";
      }
      {
        assertion = !isVolatileDataDir;
        message = "services.otelite.dataDir must be persistent and must not be under /tmp, /var/tmp, /run, or /var/run";
      }
      {
        assertion = !isHomeDataDir;
        message = "services.otelite.dataDir must not be under /home, /root, or /run/user";
      }
      {
        assertion = cfg.user != "root";
        message = "services.otelite.user must be an unprivileged account";
      }
      {
        assertion =
          !builtins.elem cfg.group [
            "root"
            "wheel"
          ];
        message = "services.otelite.group must be an unprivileged group";
      }
      {
        assertion =
          !builtins.elem cfg.port [
            4317
            4318
          ];
        message = "services.otelite.port must not conflict with fixed otlp ports 4317 or 4318";
      }
      {
        assertion = !builtins.any isManagedArg cfg.extraArgs;
        message = "services.otelite.extraArgs must not override --addr or --storage-path";
      }
    ];

    warnings = [ fixedReceiverWarning ];

    users.groups = lib.mkIf cfg.createUser { ${cfg.group} = { }; };
    users.users = lib.mkIf cfg.createUser {
      ${cfg.user} = {
        isSystemUser = true;
        group = cfg.group;
        home = dataDir;
        description = "otelite service account";
      };
    };

    systemd.tmpfiles.settings.otelite = lib.mkIf (!usesDefaultStateDirectory) {
      ${dataDir}.d = {
        mode = "0750";
        user = cfg.user;
        group = cfg.group;
      };
    };

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall (
      lib.unique [
        cfg.port
        4317
        4318
      ]
    );

    systemd.services.otelite = {
      description = "otelite telemetry receiver and dashboard";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      environment = {
        HOME = dataDir;
        RUST_LOG = cfg.logLevel;
      };
      unitConfig.RequiresMountsFor = [ dataDir ];
      serviceConfig = {
        Type = "simple";
        ExecStart = utils.escapeSystemdExecArgs (
          [
            executable
            "serve"
            "--addr"
            dashboardSocket
            "--storage-path"
            dataDir
          ]
          ++ cfg.extraArgs
        );
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = dataDir;
        StateDirectory = lib.mkIf usesDefaultStateDirectory "otelite";
        StateDirectoryMode = lib.mkIf usesDefaultStateDirectory "0750";
        Restart = "on-failure";
        RestartSec = "5s";
        StandardOutput = "journal";
        StandardError = "journal";
        UMask = "0027";

        AmbientCapabilities = "";
        CapabilityBoundingSet = "";
        LockPersonality = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectSystem = "strict";
        ReadWritePaths = [ dataDir ];
        RemoveIPC = true;
        RestrictAddressFamilies = [
          "AF_UNIX"
          "AF_INET"
          "AF_INET6"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
      };
    };
  };
}
