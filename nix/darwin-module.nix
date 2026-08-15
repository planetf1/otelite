{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.otelite;
  dashboardSocket = "${cfg.address}:${toString cfg.port}";
  executable = lib.getExe cfg.package;
  dataDir = cfg.dataDir;
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
    "/private/tmp"
    "/var/tmp"
    "/private/var/tmp"
    "/run"
    "/var/run"
    "/private/var/run"
  ];
  isVolatileDataDir = builtins.any (
    root: canonicalDataDir == root || lib.hasPrefix "${root}/" canonicalDataDir
  ) volatileDataRoots;
  fixedReceiverWarning = ''
    services.otelite: the otlp grpc and http receivers always bind all interfaces
    on tcp ports 4317 and 4318; address only configures the dashboard and api.
  '';
  otherUsers = builtins.removeAttrs config.users.users [ cfg.user ];
  otherGroups = builtins.removeAttrs config.users.groups [ cfg.group ];
  uidConflicts = lib.attrNames (lib.filterAttrs (_: user: user.uid == cfg.uid) otherUsers);
  gidConflicts = lib.attrNames (lib.filterAttrs (_: group: group.gid == cfg.gid) otherGroups);
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
      description = "nonzero tcp port for the dashboard and api.";
    };

    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/otelite";
      example = "/Library/Application Support/otelite";
      description = "absolute persistent directory containing otelite.db and the service home.";
    };

    user = lib.mkOption {
      type = lib.types.strMatching "[_a-zA-Z][_a-zA-Z0-9-]*";
      default = "_otelite";
      description = ''
        unprivileged account that runs the launchd daemon. custom accounts are
        treated as operator-managed unless createUser is enabled.
      '';
    };

    group = lib.mkOption {
      type = lib.types.strMatching "[_a-zA-Z][_a-zA-Z0-9-]*";
      default = "_otelite";
      description = ''
        group that owns otelite state. custom groups are treated as
        operator-managed unless createUser is enabled.
      '';
    };

    createUser = lib.mkOption {
      type = lib.types.bool;
      default = cfg.user == "_otelite" && cfg.group == "_otelite";
      defaultText = lib.literalExpression ''
        config.services.otelite.user == "_otelite"
        && config.services.otelite.group == "_otelite"
      '';
      description = ''
        whether nix-darwin creates and manages the configured user and group.
        this defaults to true only for the default _otelite identity. when false,
        the user and group must already exist and their attributes remain unchanged.
      '';
    };

    uid = lib.mkOption {
      type = lib.types.ints.between 502 2147483647;
      default = 536;
      description = ''
        numeric id used only when createUser is true. 536 is a configurable
        project default, not a reserved macos id. it must be unique in the
        nix-darwin configuration and on the target host.
      '';
    };

    gid = lib.mkOption {
      type = lib.types.ints.between 502 2147483647;
      default = 536;
      description = ''
        numeric id used only when createUser is true. 536 is a configurable
        project default, not a reserved macos id. it must be unique in the
        nix-darwin configuration and on the target host.
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
      description = "rust log filter exported to the daemon as RUST_LOG.";
    };

    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "--log-format=json" ];
      description = ''
        additional arguments passed to otelite after managed serve arguments.
        each item is one literal launchd argument without shell expansion.
        --addr and --storage-path are managed here and cannot be overridden.
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
        message = "services.otelite.dataDir must be persistent and must not be under a temporary or runtime directory";
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
      {
        assertion = !cfg.createUser || uidConflicts == [ ];
        message = "services.otelite.uid conflicts with configured user(s): ${lib.concatStringsSep ", " uidConflicts}";
      }
      {
        assertion = !cfg.createUser || gidConflicts == [ ];
        message = "services.otelite.gid conflicts with configured group(s): ${lib.concatStringsSep ", " gidConflicts}";
      }
    ];

    warnings = [ fixedReceiverWarning ];

    # nix-darwin otherwise warns and skips an account whose on-host id differs,
    # leaving launchd configured with an unusable identity.
    system.checks.text = lib.mkIf cfg.createUser (
      lib.mkAfter ''
        otelite_user=${lib.escapeShellArg cfg.user}
        otelite_group=${lib.escapeShellArg cfg.group}
        otelite_uid=${toString cfg.uid}
        otelite_gid=${toString cfg.gid}

        otelite_parse_id_owners() {
          local expected_attribute="$1"
          local expected_id="$2"

          /usr/bin/awk -v attribute="$expected_attribute" -v id="$expected_id" '
            NF {
              if (
                NF != 4 || $2 != attribute || $3 != "=" ||
                $4 !~ /^[0-9]+$/ || $4 != id
              ) exit 2
              print $1
              count++
            }
            END { if (count != 1) exit 2 }
          '
        }
        # end otelite_parse_id_owners

        if ! user_results=$(/usr/bin/dscl /Search -search /Users RecordName "$otelite_user" 2>&1); then
          printf >&2 'services.otelite: directory-service user lookup failed: %s\n' "$user_results"
          exit 2
        fi
        if [[ -n "$user_results" ]]; then
          printf '%s\n' "$user_results" | /usr/bin/awk -v name="$otelite_user" '
            NF {
              if ($1 != name) exit 2
              count++
            }
            END { if (count != 1) exit 2 }
          ' >/dev/null || {
            printf >&2 'services.otelite: ambiguous directory-service result for user %s\n' "$otelite_user"
            exit 2
          }
          if ! existing_uid_record=$(/usr/bin/dscl /Search -read "/Users/$otelite_user" UniqueID 2>&1); then
            printf >&2 'services.otelite: directory-service uid lookup failed: %s\n' "$existing_uid_record"
            exit 2
          fi
          existing_uid=$(printf '%s\n' "$existing_uid_record" | /usr/bin/awk '
            $1 == "UniqueID:" && NF == 2 { print $2; count++ }
            END { if (count != 1) exit 2 }
          ') || {
            printf >&2 'services.otelite: ambiguous uid result for user %s\n' "$otelite_user"
            exit 2
          }
          if [[ ! "$existing_uid" =~ ^[0-9]+$ || "$existing_uid" -ne "$otelite_uid" ]]; then
            printf >&2 'services.otelite: existing user %s has uid %s, expected %s\n' \
              "$otelite_user" "$existing_uid" "$otelite_uid"
            exit 2
          fi
        fi

        if ! uid_results=$(/usr/bin/dscl /Search -search /Users UniqueID "$otelite_uid" 2>&1); then
          printf >&2 'services.otelite: directory-service uid collision lookup failed: %s\n' "$uid_results"
          exit 2
        fi
        if [[ -n "$uid_results" ]]; then
          uid_owners=$(printf '%s\n' "$uid_results" | otelite_parse_id_owners UniqueID "$otelite_uid") || {
            printf >&2 'services.otelite: ambiguous directory-service uid result for %s\n' "$otelite_uid"
            exit 2
          }
          while IFS= read -r uid_owner; do
            if [[ "$uid_owner" != "$otelite_user" ]]; then
              printf >&2 'services.otelite: uid %s is already used by user %s\n' \
                "$otelite_uid" "$uid_owner"
              exit 2
            fi
          done <<< "$uid_owners"
        fi

        if ! group_results=$(/usr/bin/dscl /Search -search /Groups RecordName "$otelite_group" 2>&1); then
          printf >&2 'services.otelite: directory-service group lookup failed: %s\n' "$group_results"
          exit 2
        fi
        if [[ -n "$group_results" ]]; then
          printf '%s\n' "$group_results" | /usr/bin/awk -v name="$otelite_group" '
            NF {
              if ($1 != name) exit 2
              count++
            }
            END { if (count != 1) exit 2 }
          ' >/dev/null || {
            printf >&2 'services.otelite: ambiguous directory-service result for group %s\n' "$otelite_group"
            exit 2
          }
          if ! existing_gid_record=$(/usr/bin/dscl /Search -read "/Groups/$otelite_group" PrimaryGroupID 2>&1); then
            printf >&2 'services.otelite: directory-service gid lookup failed: %s\n' "$existing_gid_record"
            exit 2
          fi
          existing_gid=$(printf '%s\n' "$existing_gid_record" | /usr/bin/awk '
            $1 == "PrimaryGroupID:" && NF == 2 { print $2; count++ }
            END { if (count != 1) exit 2 }
          ') || {
            printf >&2 'services.otelite: ambiguous gid result for group %s\n' "$otelite_group"
            exit 2
          }
          if [[ ! "$existing_gid" =~ ^[0-9]+$ || "$existing_gid" -ne "$otelite_gid" ]]; then
            printf >&2 'services.otelite: existing group %s has gid %s, expected %s\n' \
              "$otelite_group" "$existing_gid" "$otelite_gid"
            exit 2
          fi
        fi

        if ! gid_results=$(/usr/bin/dscl /Search -search /Groups PrimaryGroupID "$otelite_gid" 2>&1); then
          printf >&2 'services.otelite: directory-service gid collision lookup failed: %s\n' "$gid_results"
          exit 2
        fi
        if [[ -n "$gid_results" ]]; then
          gid_owners=$(printf '%s\n' "$gid_results" | otelite_parse_id_owners PrimaryGroupID "$otelite_gid") || {
            printf >&2 'services.otelite: ambiguous directory-service gid result for %s\n' "$otelite_gid"
            exit 2
          }
          while IFS= read -r gid_owner; do
            if [[ "$gid_owner" != "$otelite_group" ]]; then
              printf >&2 'services.otelite: gid %s is already used by group %s\n' \
                "$otelite_gid" "$gid_owner"
              exit 2
            fi
          done <<< "$gid_owners"
        fi
      ''
    );

    users.knownUsers = lib.optional cfg.createUser cfg.user;
    users.knownGroups = lib.optional cfg.createUser cfg.group;
    users.users = lib.mkIf cfg.createUser {
      ${cfg.user} = {
        uid = cfg.uid;
        gid = cfg.gid;
        home = cfg.dataDir;
        createHome = true;
        isHidden = true;
        shell = "/usr/bin/false";
        description = "otelite service account";
      };
    };
    users.groups = lib.mkIf cfg.createUser {
      ${cfg.group} = {
        gid = cfg.gid;
        description = "otelite service group";
      };
    };

    launchd.daemons.otelite = {
      environment = {
        HOME = dataDir;
        RUST_LOG = cfg.logLevel;
      };
      serviceConfig = {
        ProgramArguments = [
          executable
          "serve"
          "--addr"
          dashboardSocket
          "--storage-path"
          dataDir
        ]
        ++ cfg.extraArgs;
        RunAtLoad = true;
        KeepAlive = true;
        WorkingDirectory = dataDir;
        ProcessType = "Background";
        ThrottleInterval = 5;
        UserName = cfg.user;
        GroupName = cfg.group;
        # launchd property lists store this value in decimal: 23 is octal 0027.
        Umask = 23;
      };
    };
  };
}
