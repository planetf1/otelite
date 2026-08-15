# Nix Support

The flake provides source-built packages, development shells, checks, and
service modules for NixOS and nix-darwin.

## Packages

Run or build Otelite for the current system:

```bash
nix run github:planetf1/otelite
nix build github:planetf1/otelite#otelite
```

The source-built `otelite` package is available on `x86_64-linux`,
`aarch64-linux`, and `aarch64-darwin` and is the default package on each system.

## Development and Checks

Clone the repository and run `nix develop` for the Rust toolchain and native
build dependencies. Flake checks cover workspace formatting, Clippy, builds, tests,
and service module evaluation on supported systems.

## Services

Both modules provision their default unprivileged account and run `otelite
serve` in the foreground.

Options use these defaults:

| Option | NixOS | nix-darwin | Effect |
| --- | --- | --- | --- |
| `enable` | `false` | `false` | enable the service |
| `package` | flake package | flake package | executable to run |
| `address` | `127.0.0.1` | `127.0.0.1` | dashboard/API numeric IP only |
| `port` | `3000` | `3000` | dashboard/API TCP port, 1-65535 |
| `dataDir` | `/var/lib/otelite` | `/var/lib/otelite` | persistent SQLite state and home |
| `user` / `group` | `otelite` | `_otelite` | unprivileged service identity |
| `createUser` | default identity: `true`; custom: `false` | default identity: `true`; custom: `false` | provision the configured identity |
| `uid` / `gid` | N/A | `536` / `536` | IDs for managed accounts only |
| `logLevel` | `info` | `info` | `RUST_LOG` value |
| `extraArgs` | `[]` | `[]` | literal unmanaged `serve` arguments |
| `openFirewall` | `false` | N/A | open dashboard and OTLP TCP ports |

`dataDir` must be an absolute, canonical, persistent path, not `/tmp`,
`/var/tmp`, `/run`, `/var/run`, or the corresponding macOS `/private` aliases.
NixOS uses `StateDirectory` for the default path and tmpfiles for custom paths;
nix-darwin creates a managed account's home. Set `user` and `group` to use custom
existing identities; `createUser` then defaults to false and the operator remains
responsible for the account and state directory. nix-darwin checks the full
directory-service search path for UID/GID conflicts before creating an account;
its configurable defaults are project choices, not reserved macOS IDs.

NixOS logs to journald (`journalctl -u otelite`). nix-darwin leaves launchd's
stdout/stderr paths unset so macOS handles daemon output without privileged file
operations inside service-owned state. `address` and `port` configure only the
dashboard/API. The upstream OTLP gRPC and HTTP receivers always listen on all
interfaces on TCP ports 4317 and 4318; `openFirewall` opens those fixed ports and
the dashboard port only on NixOS.

### NixOS

```nix
{
  inputs.otelite.url = "github:planetf1/otelite";
  imports = [ inputs.otelite.nixosModules.default ];

  services.otelite = {
    enable = true;
    address = "127.0.0.1";
    port = 3000;
    openFirewall = false;
  };
}
```

### nix-darwin

```nix
{
  inputs.otelite.url = "github:planetf1/otelite";
  imports = [ inputs.otelite.darwinModules.default ];

  services.otelite = {
    enable = true;
    # Set unused host IDs if the configurable project defaults collide.
    uid = 536;
    gid = 536;
  };
}
```
