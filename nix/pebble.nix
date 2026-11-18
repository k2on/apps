# The Pebble watchapps in nix (docs/north-star.md, steps 2 and 5): the SDK,
# pebble-tool, and the two `.pbw`s built from ui/pebble.
#
# - **The SDK** is sdk-core 4.44 from sdk.repebble.com (open source, as
#   PebbleOS is), without its emulator images (`<platform>/qemu`, 108 MB of
#   its 114): the emulator is qemu-pebble, which comes only in the SDK's
#   prebuilt x86_64 toolchain and is not packaged here. Beside it the npm
#   modules its JavaScript bundler is (webpack 1, `nix/pebble/`'s lockfile,
#   as `pebble sdk install` writes it), and the Python its waf runs on.
# - **The compiler** is nixpkgs' arm-none-eabi GCC (`pkgsCross.arm-embedded`,
#   13.3, substituted for x86_64 and aarch64 alike), not the SDK's 570 MB
#   xPack toolchain nor `gcc-arm-embedded` (1.2 GB with its multilibs). It
#   is built without multilib, so its libgcc and newlib are Arm-state code a
#   Cortex-M3 cannot run: a watchapp must take nothing from them (the Rust
#   staticlibs carry compiler_builtins, libpebble the C library), and each
#   build below fails if its link took any member of those archives.
# - **pebble-tool** is PyPI's, with libpebble2 and sourcemap beside it, on
#   nixpkgs' Python: `nix run .#pebble -- install --phone <ip> <pbw>`. The
#   emulator's own dependencies (pypkjs, websockify) are left out, and so
#   is the SDK: it installs, logs, takes screenshots and pings a watch
#   through the phone; its `build` is the `.pbw`s here.
# - **The watchapps**: each one's Rust staticlib built by cargo for
#   thumbv7m-none-eabi with the workspace's watch toolchain (`watchRust`),
#   then the SDK's waf over its wscript, handed the library (the wscripts'
#   `PROBE_RUST_LIB` / `NOW_PLAYING_RUST_LIB`).
{ pkgs, lib, only, watchRust }:
let
  sdkVersion = "4.44";
  python = pkgs.python3;

  # sdk-core, its emulator images taken out as it is fetched, so the store
  # holds 6 MB of it rather than the 50 MB tarball and its 114 MB tree.
  # (The tarball's sha256 is f9ce98a3…8003fd7; this is the tree's.)
  sdkCore = pkgs.fetchzip {
    name = "pebble-sdk-core-${sdkVersion}";
    url = "https://sdk.repebble.com/releases/${sdkVersion}/sdk-core.tar.gz";
    hash = "sha256-Cm5JStU+eL6SzZWZUJ1E67pZAOEc/pMoCiEyG+sNQ9s=";
    postFetch = ''
      rm -rf $out/pebble/*/qemu
    '';
  };

  # The bundler `enableMultiJS` asks for, at the versions `pebble sdk
  # install` resolved for sdk-core's package.json.
  nodeModules = pkgs.buildNpmPackage {
    pname = "pebble-sdk-node-modules";
    version = sdkVersion;
    src = ./pebble;
    npmDepsHash = "sha256-QORsBq6DX2z1idRD4VGI6tXVAviEdis9kYjpDooDkhI=";
    dontNpmBuild = true;
    installPhase = ''
      mkdir -p $out
      cp -r node_modules $out/
    '';
  };

  # What the SDK's waf imports (its requirements.txt).
  sdkPython = python.withPackages (ps: [ ps.freetype-py ps.sh ps.pypng ]);

  armGcc = pkgs.pkgsCross.arm-embedded.buildPackages.gcc;
  armBinutils = pkgs.pkgsCross.arm-embedded.buildPackages.binutils;

  # The SDK as pebble-tool lays it out: sdk-core, its node_modules beside
  # it, and waf's library unpacked once, here, as waf does on its first run
  # (it cannot in the store).
  sdk = pkgs.runCommand "pebble-sdk-${sdkVersion}" { nativeBuildInputs = [ sdkPython ]; } ''
    mkdir -p $out
    cp -r ${sdkCore} $out/sdk-core
    chmod -R u+w $out/sdk-core
    patchShebangs $out/sdk-core
    (cd $out/sdk-core/pebble && python3 ./waf --version)
    ln -s ${nodeModules}/node_modules $out/node_modules
  '';

  # The waf build of an app directory (its package.json, wscript, src/)
  # with its Rust library given by `libVar`. `extra` is copied into the
  # build tree too (the now-playing app's C shim, ../c from it).
  pbw = { name, app, libVar, rustLib, extra ? [ ] }:
    pkgs.stdenvNoCC.mkDerivation {
      inherit name;
      src = only ([ app ] ++ extra);
      nativeBuildInputs = [ sdkPython armGcc armBinutils pkgs.nodejs-slim ];
      dontConfigure = true;
      # The SDK's flags alone: none of the wrapper's hardening (its `-z
      # relro`, its stack protector), and no fixup of Arm ELFs.
      hardeningDisable = [ "all" ];
      dontFixup = true;
      buildPhase = ''
        runHook preBuild
        export ${libVar}=${rustLib}/${rustLib.file}
        export NODE_PATH=${sdk}/node_modules NOCLIMB=1 HOME=$TMPDIR
        cd ${app}
        # The link's map, to say what it took from which archive.
        export LDFLAGS="-Wl,-Map=$TMPDIR/app.map"
        python3 ${sdk}/sdk-core/pebble/waf configure build
        # The toolchain has no Thumb-only multilib (above): nothing of its
        # libgcc or newlib, which are Arm-state code, may have been linked.
        taken=$(sed -n '/^Archive member included/,/^Discarded input sections/p' $TMPDIR/app.map \
          | grep -E '/lib(gcc|c|g|m|nosys|c_nano|g_nano)\.a\(' || true)
        if [ -n "$taken" ]; then
          echo "the app took these from the toolchain's libraries, which are Arm-state code a Cortex-M3 cannot run:" >&2
          echo "$taken" >&2
          exit 1
        fi
        grep -q "^Archive member included" $TMPDIR/app.map
        runHook postBuild
      '';
      installPhase = ''
        runHook preInstall
        mkdir -p $out
        cp build/*.pbw $out/
        cp build/emery/pebble-app.elf $out/emery.elf
        cp build/emery/pebble-app.bin $out/emery.bin
        cp $TMPDIR/app.map $out/emery.map
        runHook postInstall
      '';
    };

  # The host workspace's crates from crates.io (the test upstream's
  # WebSocket), which cargo resolves even for a build that does not use
  # them, as its lockfile has them.
  hostVendor = pkgs.rustPlatform.importCargoLock { lockFile = ../ui/pebble/host/Cargo.lock; };
  cargoHome = pkgs.writeTextDir "config.toml" ''
    [source.crates-io]
    replace-with = "vendored-sources"
    [source.vendored-sources]
    directory = "${hostVendor}"
  '';

  # The host workspace's crates and the kernel they link, without the
  # READMEs or the C; and what the demo's screen is written out with at
  # build time (ui/docs/pebble.md): arkui's view model and `#[view]`
  # (ark-app, its macros, ark and its module port), the generator and the
  # glyphs it flattens, which run on the build machine only.
  hostCrates = [
    "rust/Cargo.toml" "rust/kernel" "ui/pebble/host/Cargo.toml" "ui/pebble/host/Cargo.lock"
    "ui/pebble/host/rust" "ui/pebble/host/demo/app" "ui/pebble/host/demo/view" "ui/pebble/host/demo/watch"
    "ui/pebble/host/upstream"
    "rust/ark" "rust/ark-macros" "ui/rust/app" "ui/rust/module" "ui/rust/arkui-macros" "ui/rust/gen" "ui/rust/glyphs"
  ];

  # A staticlib for the watch, built by cargo offline with the watch's
  # profile. With a C compiler: what runs on the build machine (the demo's
  # build script, which writes its screen, and the proc macros under it)
  # is linked by it.
  watchLib = { name, dirs, manifest, package ? null, file }:
    pkgs.stdenv.mkDerivation {
      inherit name;
      passthru = { inherit file; };
      # An Arm archive: nothing of the build machine's fixup applies.
      dontFixup = true;
      src = only dirs;
      nativeBuildInputs = [ watchRust ];
      buildPhase = ''
        export CARGO_HOME=$TMPDIR/cargo
        mkdir -p $CARGO_HOME && cp ${cargoHome}/config.toml $CARGO_HOME/
        cargo build --release --offline --locked --target thumbv7m-none-eabi \
          --manifest-path ${manifest} ${lib.optionalString (package != null) "-p ${package}"} \
          --target-dir $TMPDIR/target
      '';
      installPhase = ''
        mkdir -p $out
        cp $TMPDIR/target/thumbv7m-none-eabi/release/${file} $out/
      '';
    };

  probeLib = watchLib {
    name = "pebble-probe-rust";
    dirs = [ "ui/pebble/probe/rust" ];
    manifest = "ui/pebble/probe/rust/Cargo.toml";
    file = "libprobe.a";
  };
  nowPlayingLib = watchLib {
    name = "pebble-now-playing-rust";
    dirs = hostCrates;
    manifest = "ui/pebble/host/Cargo.toml";
    package = "now-playing-watch";
    file = "libnow_playing_watch.a";
  };

  # PyPI's sdists by their files' own addresses (fetchPypi's
  # /packages/source/ redirect does not reach every builder).
  pypi = path: hash: pkgs.fetchurl { url = "https://files.pythonhosted.org/${path}"; inherit hash; };

  libpebble2 = python.pkgs.buildPythonPackage {
    pname = "libpebble2";
    version = "0.0.34";
    pyproject = true;
    src = pypi "packages/25/6e/1afaa34c1f15811b921b4facafc43c35195630dce4279ec464b9bb32a65d/libpebble2-0.0.34.tar.gz" "sha256-3hwu3ytlWOvoQ9lMpUvlrH6sC6NpqeAZcONdlODVey4=";
    build-system = [ python.pkgs.setuptools ];
    dependencies = with python.pkgs; [ websocket-client pyserial six ];
    doCheck = false;
    pythonImportsCheck = [ "libpebble2" ];
  };
  sourcemap = python.pkgs.buildPythonPackage {
    pname = "sourcemap";
    version = "0.2.1";
    pyproject = true;
    src = pypi "packages/63/12/0dba762927b6a763fbb33f8a96f5080b0745c7493360c9ead6449c9a9d70/sourcemap-0.2.1.tar.gz" "sha256-vgCpAYXnoWuHu+YqaP/V44vEOO9HAIBtm5DkTYAneHw=";
    build-system = [ python.pkgs.setuptools ];
    doCheck = false;
    pythonImportsCheck = [ "sourcemap" ];
  };
in
rec {
  inherit sdk nodeModules probeLib nowPlayingLib;

  pebble = python.pkgs.buildPythonApplication {
    pname = "pebble-tool";
    version = "5.2.0";
    pyproject = true;
    src = pypi "packages/fc/ca/a55ba32df19ea871d7608aaea9d0aec205367afa1dd09a7abcc18b6fd14b/pebble_tool-5.2.0.tar.gz" "sha256-AYA993TstllxvuTE2mOFAFraAlIKCTTGDQaD3LdJWo8=";
    build-system = with python.pkgs; [ setuptools hatchling ];
    dependencies = (with python.pkgs; [
      cobs colorama google-auth-oauthlib google-auth httplib2 oauth2client packaging pillow
      progressbar2 pyasn1-modules pyasn1 pypng pyqrcode pyserial requests rsa six
      websocket-client wheel
    ]) ++ [ libpebble2 sourcemap ];
    # The emulator's (pypkjs runs the phone's JavaScript beside it,
    # websockify its VNC) and the SDK build's (freetype-py): neither is
    # what this one is for. nixpkgs 24.11 is a little behind its lower
    # bounds, which it does not need.
    pythonRemoveDeps = [ "pypkjs" "websockify" "freetype-py" ];
    pythonRelaxDeps = true;
    doCheck = false;
    # It makes its settings' directory as it is imported.
    preBuild = "export HOME=$TMPDIR";
    pythonImportsCheck = [ "pebble_tool" ];
    meta.mainProgram = "pebble";
  };

  pebble-probe = pbw {
    name = "pebble-probe";
    app = "ui/pebble/probe";
    libVar = "PROBE_RUST_LIB";
    rustLib = probeLib;
  };

  # The host's tests (the disk, the link, the text, the glyphs' painting,
  # the demo's domain, and its screen as each toolkit is handed it: the
  # watch's layers and the phones' views, held to their snapshots) on the
  # build machine, and the test upstream built beside them, as
  # ui/pebble/host/README.md runs them.
  pebble-host = pkgs.stdenv.mkDerivation {
    name = "pebble-host-tests";
    src = only hostCrates;
    nativeBuildInputs = [ watchRust ];
    buildPhase = ''
      export CARGO_HOME=$TMPDIR/cargo
      mkdir -p $CARGO_HOME && cp ${cargoHome}/config.toml $CARGO_HOME/
      set -o pipefail
      cargo test --offline --locked --manifest-path ui/pebble/host/Cargo.toml \
        -p ark-pebble -p now-playing -p now-playing-view -p now-playing-upstream --target-dir $TMPDIR/target 2>&1 | tee $TMPDIR/log
    '';
    installPhase = "cp $TMPDIR/log $out";
  };

  # The now-playing app fits the watch with room to grow: its image with
  # its bss (what PebbleOS loads and zeroes, `virtual_size` in the app's
  # header) is at most 96 KB of the 128 KB an app gets for code and heap.
  pebble-size = pkgs.runCommand "pebble-now-playing-size" { nativeBuildInputs = [ python ]; } ''
    python3 - ${pebble-now-playing}/demo.pbw <<'EOF' > $out
    import struct, sys, zipfile
    budget = 96 * 1024
    with zipfile.ZipFile(sys.argv[1]) as pbw:
        app = pbw.read("emery/pebble-app.bin")
    assert app[:8] == b"PBLAPP\0\0", "emery/pebble-app.bin is not an app"
    major, minor = app[8], app[9]
    load = struct.unpack_from("<H", app, 0x0E)[0]
    virtual = struct.unpack_from("<H", app, 0x80)[0]
    if (major, minor) >= (0x10, 0x01):
        load |= app[0x82] << 16
        virtual |= app[0x83] << 16
    print(f"now playing on the watch: an image of {load} bytes, {virtual} with its bss, of the {budget} allowed")
    if virtual > budget:
        sys.exit(f"the now-playing app is {virtual - budget} bytes past its 96 KB (ui/pebble/host/README.md)")
    EOF
    cat $out
  '';

  pebble-now-playing = pbw {
    name = "pebble-now-playing";
    app = "ui/pebble/host/demo";
    extra = [ "ui/pebble/host/c" ];
    libVar = "NOW_PLAYING_RUST_LIB";
    rustLib = nowPlayingLib;
  };
}
