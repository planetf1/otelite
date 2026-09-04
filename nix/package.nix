{
  lib,
  clippy,
  fetchurl,
  openssl,
  pkg-config,
  rustPlatform,
  rustfmt,
  stdenv,
  workspaceCheck ? false,
}:
let
  root = ../.;
  rootString = toString root;
  workspace = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  swaggerUi = fetchurl {
    url = "https://github.com/swagger-api/swagger-ui/archive/refs/tags/v5.17.12.zip";
    hash = "sha256-HK4z/JI+1yq8BTBJveYXv9bpN/sXru7bn/8g5mf2B/I=";
  };
  src = lib.cleanSourceWith {
    name = "otelite-source";
    src = root;
    filter =
      path: type:
      let
        pathString = toString path;
        relative = lib.removePrefix "${rootString}/" pathString;
        topLevel = builtins.head (lib.splitString "/" relative);
        name = baseNameOf path;
        excludedDirectory =
          type == "directory"
          && builtins.elem name [
            ".cache"
            ".direnv"
            ".git"
            "build"
            "coverage"
            "debug"
            "dist"
            "release"
            "target"
          ];
      in
      lib.cleanSourceFilter path type
      && (
        pathString == rootString
        || builtins.elem topLevel [
          "Cargo.lock"
          "Cargo.toml"
          "LICENSE"
          "README.md"
          "clippy.toml"
          "crates"
          "rustfmt.toml"
        ]
      )
      && !excludedDirectory
      && !(name == "result" || lib.hasPrefix "result-" name)
      && !lib.hasSuffix ".swp" name
      && !lib.hasSuffix "~" name;
  };
in
rustPlatform.buildRustPackage {
  pname = if workspaceCheck then "otelite-workspace-check" else "otelite";
  version = workspace.workspace.package.version;

  inherit src;
  cargoLock.lockFile = ../Cargo.lock;

  # cargoInstallHook copies the binaries captured after cargoBuildHook; it does
  # not run cargo install or consume cargoInstallFlags.
  cargoBuildFlags =
    if workspaceCheck then
      [
        "--workspace"
        "--all-features"
      ]
    else
      [
        "-p"
        "otelite"
        "--bin"
        "otelite"
        "--all-features"
      ];
  cargoTestFlags =
    if workspaceCheck then
      [
        "--workspace"
        "--all-features"
      ]
    else
      [
        "-p"
        "otelite"
        "--all-features"
      ];
  doCheck = true;
  checkType = "debug";
  dontUseCargoParallelTests = true;
  preBuild = ''
    install -m 0644 ${swaggerUi} "$TMPDIR/swagger-ui.zip"
    export SWAGGER_UI_DOWNLOAD_URL="file:$TMPDIR/swagger-ui.zip"
  '';
  preCheck = ''
    install -m 0644 ${swaggerUi} "$TMPDIR/swagger-ui.zip"
    export SWAGGER_UI_DOWNLOAD_URL="file:$TMPDIR/swagger-ui.zip"
    export HOME="$TMPDIR/home"
    mkdir -p "$HOME"
  ''
  + lib.optionalString workspaceCheck ''
    cargo fmt --all -- --check
    cargo clippy \
      --target ${stdenv.hostPlatform.rust.rustcTarget} \
      --offline \
      --workspace \
      --all-targets \
      --all-features \
      -- -D warnings
    cargo build \
      --target ${stdenv.hostPlatform.rust.rustcTarget} \
      --offline \
      --workspace \
      --all-features
  '';

  # ps (procps) and lsof are shelled out to by the service-command tests
  # (is_otelite_process / local_otelite_pid); the Nix sandbox does not
  # provide them otherwise, so the check phase panics on missing binaries.
  nativeBuildInputs = [
    lsof
    pkg-config
    procps
  ]
  ++ lib.optionals workspaceCheck [
    clippy
    rustfmt
  ];
  buildInputs = [ openssl ];

  env = {
    OTELITE_GIT_SHA = "nix";
    SOURCE_DATE_EPOCH = "1";
    SWAGGER_UI_DOWNLOAD_URL = "file:${swaggerUi}";
  };

  postInstall = ''
    test -x "$out/bin/otelite"
    install -Dm644 LICENSE "$out/share/licenses/otelite/LICENSE"
  '';

  doInstallCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  installCheckPhase = ''
    runHook preInstallCheck
    "$out/bin/otelite" --version | grep -F "${workspace.workspace.package.version} (nix)"
    "$out/bin/otelite" --help | grep -F "Lightweight OpenTelemetry receiver and dashboard"
    "$out/bin/otelite" serve --help | grep -F -- "--storage-path"
    runHook postInstallCheck
  '';

  meta = {
    description = "lightweight OpenTelemetry receiver and dashboard for local development";
    homepage = workspace.workspace.package.homepage;
    changelog = "${workspace.workspace.package.repository}/blob/v${workspace.workspace.package.version}/CHANGELOG.md";
    license = lib.licenses.asl20;
    mainProgram = "otelite";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
      "aarch64-darwin"
    ];
  };
}
