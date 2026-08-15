{
  description = "otelite - lightweight OpenTelemetry receiver and dashboard";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    nix-darwin = {
      url = "github:LnL7/nix-darwin";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      flake =
        let
          nixosModule = import ./nix/nixos-module.nix;
          darwinModule = import ./nix/darwin-module.nix;
        in
        {
          nixosModules = {
            default = nixosModule;
            otelite = nixosModule;
          };
          darwinModules = {
            default = darwinModule;
            otelite = darwinModule;
          };
        };

      perSystem =
        { system, ... }:
        let
          pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.rust-overlay.overlays.default ];
          };
          lib = pkgs.lib;
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          otelite = pkgs.callPackage ./nix/package.nix { };
          oteliteWorkspace = pkgs.callPackage ./nix/package.nix { workspaceCheck = true; };
          packageOutputs = {
            inherit otelite;
            default = otelite;
          };
          appOutputs = {
            otelite = {
              type = "app";
              program = pkgs.lib.getExe otelite;
              meta.description = "run otelite";
            };
            default = {
              type = "app";
              program = pkgs.lib.getExe otelite;
              meta.description = "run otelite";
            };
          };
          checkOutputs = import ./nix/checks {
            inherit
              inputs
              system
              pkgs
              lib
              otelite
              oteliteWorkspace
              ;
          };
        in
        {
          packages = packageOutputs;

          apps = appOutputs;

          checks = checkOutputs;

          formatter = pkgs.nixfmt-tree;

          devShells.default = pkgs.mkShell {
            packages = [
              rustToolchain
              pkgs.nixfmt-tree
              pkgs.pkg-config
            ];
            buildInputs = [ pkgs.openssl ];
          };
        };
    };
}
