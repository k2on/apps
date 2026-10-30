{
  description = "ArkDB: the specification and runtime in Rust, and harken on it";

  # FlakeHub rather than github: for every input, because a machine that can
  # reach FlakeHub and cache.nixos.org but not api.github.com (this one, for
  # instance) must still be able to build. The lock pins the tarballs.
  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.2411.*.tar.gz";
    rust-overlay.url = "https://flakehub.com/f/oxalica/rust-overlay/0.1.*.tar.gz";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

    # The classical catalogue (github.com/k2on/classical): what the demo's
    # library is made of (harken/iced/src/catalogue.json, the copy
    # `nix run .#update-catalogue` writes and checks.catalogue holds to
    # this). Over git for the reason above; plain files, built by its
    # catalogue.nix.
    classical = {
      url = "git+https://github.com/k2on/classical?ref=main";
      flake = false;
    };

    # The pinned previous revisions of this repository (nix/versions.nix),
    # one input each, so that the lockfile pins them and an old build is
    # evaluated offline from it. Over git rather than `github:` for the
    # reason above: the API is the part that may be unreachable. Each
    # follows this flake's nixpkgs and toolchain overlay, so an old
    # revision is the old code under today's compiler — what a matrix of
    # the code, rather than of the compilers, wants. None at the moment
    # (nix/versions.nix is empty); one is written as
    #
    #   harken-<name> = {
    #     url = "git+https://github.com/k2on/apps?ref=main&rev=<rev>";
    #     inputs.nixpkgs.follows = "nixpkgs";
    #     inputs.rust-overlay.follows = "rust-overlay";
    #   };
  };

  outputs = { self, nixpkgs, rust-overlay, ... }@inputs:
    let
      lib = nixpkgs.lib;
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      # nixpkgs for a system, or for building on one for another
      # (`crossSystem`: a phone's ABI, the browser).
      importPkgs = args: import nixpkgs ({
        overlays = [ rust-overlay.overlays.default ];
        # The Android NDK and SDK are unfree, and their licence is
        # accepted here once.
        config = { allowUnfree = true; android_sdk.accept_license = true; };
      } // args);
      forAll = f: lib.genAttrs systems (system: f (importPkgs { inherit system; }));

      # A source tree that is only the named directories of this repository,
      # so that a prose edit under docs/ is not an input to a compile. Build
      # output under any `target` is left out wherever it lies.
      only = dirs: lib.cleanSourceWith {
        name = "source";
        src = self;
        filter = path: type:
          let
            rel = lib.removePrefix (toString self + "/") (toString path);
            under = d: rel == d || lib.hasPrefix (d + "/") rel;
            leadsTo = d: type == "directory" && lib.hasPrefix (rel + "/") d;
            build = lib.hasSuffix "/target" rel || lib.hasInfix "/target/" rel
              || lib.hasSuffix "/.build" rel || lib.hasInfix "/.build/" rel
              || lib.hasSuffix "/build" rel || lib.hasInfix "/build/" rel
              || lib.hasInfix "/.gradle" rel || lib.hasInfix "/.kotlin" rel;
          in
          !build && lib.any (d: under d || leadsTo d) dirs;
      };

      # The Rust workspace is `rust/`, and harken's crates join it from
      # `harken/` by path, so both trees are the source of every Rust build.
      # The kernel's tests read `spec/vectors`, so a build that runs them
      # adds it.
      rustDirs = [ "rust" "harken/domain" "harken/player" "harken/server" "harken/iced" "harken/ffi" "money/domain" "money/iced" "money/ffi" "money/server" ];
      # The typed Kotlin and Swift `arkdb gen` writes, which harken/ffi's and
      # money/ffi's tests hold to their domains (`tests/gen.rs`).
      genDirs = [
        "harken/android/app/src/main/kotlin/dev/harken/android/gen" "harken/ios/Harken/Gen"
        "money/android/app/src/main/kotlin/dev/money/android/gen" "money/ios/Money/Gen"
      ];

      # The wasm-bindgen CLI must be the exact version of the `wasm-bindgen`
      # crate the workspace locked, or the glue it writes does not match the
      # module's imports and the page fails at load with a name nobody
      # recognises. So the version is read out of the lockfile rather than
      # written down, and the two hashes beside it are the only thing to move
      # when the crate moves — a stale hash fails and prints the right one.
      wasmBindgenVersion =
        let lock = builtins.fromTOML (builtins.readFile ./rust/Cargo.lock);
        in (lib.findFirst (p: p.name == "wasm-bindgen") (throw "rust/Cargo.lock has no wasm-bindgen") lock.package).version;
      wasmBindgenHashes = {
        "0.2.129" = {
          hash = "sha256-pcecKQd7E8Opw6bkFoE569epUi7gh5qpQF1e5PJY6V8=";
          cargoHash = "sha256-/uK14uPcftMFwlBx28Z1qHkm1edIWVVXi6hRRQRmYec=";
        };
      };

      perSystem = pkgs:
        let
          rust = pkgs.rust-bin.stable.latest.default;
          # Only for the wasm-bindgen CLI below, which nixpkgs builds itself.
          rustPlatform = pkgs.makeRustPlatform { cargo = rust; rustc = rust; };

          # Every Rust build is a derivation per crate (nix/rust.nix):
          # nixpkgs' buildRustCrate over the crates rust/Cargo.nix lists,
          # which crate2nix writes from the lockfile (`nix run
          # .#update-cargo-nix`, held to it by the workflow). A crate is
          # built from its own source, features and target and its
          # dependencies' derivations, and from nothing else, so it is
          # compiled once and substituted from the cache after, whatever
          # else moves: an edit to a manifest compiles the crates whose
          # inputs it moved and what depends on them, an edit to harken the
          # crates of harken. Each target (the host, wasm32, each Android
          # ABI) is nixpkgs for that platform (`importPkgs`'s `crossSystem`)
          # and the toolchain with the target added.
          rustCrates = import ./nix/rust.nix {
            inherit lib;
            cargoNix = import ./rust/Cargo.nix { inherit pkgs; };
          };
          # The root crate `packageId` of the workspace, for the host unless
          # `pkgs` and `rust` say another target; its binaries under `bin/`,
          # its library under its `lib` output.
          crate = args: rustCrates.build ({ inherit pkgs rust; } // args);
          crossPkgs = crossSystem: importPkgs {
            localSystem = pkgs.stdenv.buildPlatform.system;
            inherit crossSystem;
          };
          # A run of crates' tests, which is cargo's `cargo test --release
          # -p <crate>…`: each crate's test targets built (the library's and
          # binaries' unit tests and every `tests/*.rs`, with its
          # dev-dependencies), then run, in order, from the crate's directory
          # in one copy of `dirs`, so that a test finds its files where cargo
          # would (`CARGO_MANIFEST_DIR`, `../../spec`), and a writer and a
          # reader see each other's. A crate is `{ packageId; member; }` (its
          # directory), with its `features` and its `doctests` (the
          # library's, which cargo runs with its tests) if it has them. `targets`
          # names the test targets to run, cargo's `--test <name>` (each
          # crate runs those it has), all of them if empty; `args` follow
          # each, as after cargo's `--`.
          crateTests =
            { name
            , crates
            , dirs ? rustDirs
            , targets ? [ ]
            , args ? ""
            , env ? { }
            , preCheck ? ""
            , postCheck ? ""
            , nativeBuildInputs ? [ ]
              # Applied to each crate's test build (`overrideAttrs`).
            , rootAttrs ? (_: { })
            , out ? "touch $out"
              # The workspace the crates are of.
            , build ? crate
            }:
            let
              src = only dirs;
              # Compiled from a tree that is the crate's directory alone, at
              # the path it has in `dirs`: so `CARGO_MANIFEST_DIR` is where
              # the run finds it, and an edit elsewhere compiles nothing.
              built = { packageId, member, features ? [ "default" ], doctests ? false }:
                let name' = lib.replaceStrings [ "-" ] [ "_" ] packageId; in
                build {
                  inherit packageId features;
                  tests = true;
                  root = { src = only [ member ]; workspace_member = member; };
                  # rustdoc compiles and runs the doctests against the
                  # library and its dependencies as the build has them.
                  rootAttrs = old: rootAttrs old // lib.optionalAttrs doctests {
                    postBuild = (old.postBuild or "") + ''
                      echo "running the doctests"
                      ${rust}/bin/rustdoc --test src/lib.rs --crate-name ${name'} --crate-type lib \
                        $(echo $LIB_RUSTC_OPTS | sed 's/--remap-path-prefix=[^ ]*//') \
                        -L dependency=target/deps --extern ${name'}=$(echo target/lib/lib${name'}-*.rlib)
                    '';
                  };
                };
              run = c: ''
                cd "$NIX_BUILD_TOP/source/${c.member}"
                for t in ${built c}/tests/*; do
                  n=$(basename "$t")
                  ${lib.optionalString (targets != [ ]) ''
                    case " ${lib.concatStringsSep " " targets} " in *" $n "*) ;; *) continue ;; esac
                  ''}
                  echo "running ${c.packageId}: $n"
                  "$t" ${args}
                  ran=$((ran + 1))
                done
              '';
            in
            pkgs.runCommand "test-${name}" ({ inherit nativeBuildInputs; } // env) ''
              # The build's directory, where the tests were compiled from.
              cp -r ${src} "$NIX_BUILD_TOP/source"
              chmod -R u+w "$NIX_BUILD_TOP/source"
              cd "$NIX_BUILD_TOP/source"
              export RUST_BACKTRACE=1
              ${preCheck}
              shopt -s nullglob
              ran=0
              ${lib.concatMapStrings run crates}
              [ "$ran" -gt 0 ] || { echo "no test target ran" >&2; exit 1; }
              cd "$NIX_BUILD_TOP/source"
              ${postCheck}
              ${out}
            '';

          # The browser peer: `harken/iced` compiled to wasm32 and bound by
          # wasm-bindgen, beside its page — one static directory. The same
          # crate as the desktop; `demo` is the seeded library with no
          # server that Pages publishes, and without it the page signs in
          # against the server it is served from. The toolchain is the
          # workspace's with the wasm target added, and the CLI is built from
          # the crate at the version the lockfile names.
          wasmRust = rust.override { targets = [ "wasm32-unknown-unknown" ]; };
          # nixpkgs for wasm32: its compiler is rustc's own linker, and
          # nixpkgs has no C toolchain for `wasm32-unknown-none`, which no
          # crate here asks for.
          wasmPkgs = importPkgs {
            localSystem = pkgs.stdenv.buildPlatform.system;
            crossSystem = { config = "wasm32-unknown-none"; rust.rustcTarget = "wasm32-unknown-unknown"; };
            config = { allowUnfree = true; allowUnsupportedSystem = true; };
          };
          wasm-bindgen-cli = pkgs.wasm-bindgen-cli.override ({
            inherit rustPlatform;
            version = wasmBindgenVersion;
          } // (wasmBindgenHashes.${wasmBindgenVersion}
            or (throw "flake.nix: no hashes for wasm-bindgen ${wasmBindgenVersion}; add them to wasmBindgenHashes")));
          # One wasm page: a binary of the workspace compiled to wasm32 and
          # bound by wasm-bindgen, beside the static files it is loaded from —
          # the browser peer, with and without its demo.
          wasmPage = { pname, crate, bin ? crate, features ? [ "default" ], files }:
            let
              wasm = rustCrates.build { pkgs = wasmPkgs; rust = wasmRust; packageId = crate; inherit features; };
            in
            pkgs.runCommand pname { nativeBuildInputs = [ wasm-bindgen-cli pkgs.binaryen ]; } ''
              wasm-bindgen --target web --no-typescript --out-dir pkg ${wasm}/bin/${bin}.wasm
              wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
                --enable-sign-ext --enable-mutable-globals --enable-reference-types \
                -o pkg/${bin}_bg.wasm pkg/${bin}_bg.wasm
              # The page names the module and the wasm with `?v=dev`; this
              # output's own hash replaces it, so a browser holding an older
              # build is sent URLs its cache has never seen (harken/iced/web).
              mkdir -p $out
              cp -r pkg $out/
              ${lib.concatMapStrings (f: "cp ${f} $out/${baseNameOf f}\n") files}
              chmod u+w $out/index.html
              substituteInPlace $out/index.html --replace-fail "v=dev" "v=$(basename $out | cut -c1-32)"
            '';
          icedWeb = { pname, crate ? "harken-iced", demo, files ? [ ./harken/iced/web/index.html ./harken/iced/web/favicon.svg ] }: wasmPage {
            inherit pname crate files;
            features = [ "default" ] ++ lib.optional demo "demo";
          };
          # What GitHub Pages publishes.
          harken-web = icedWeb { pname = "harken-web"; demo = true; };
          # What harken-server serves as its web directory.
          harken-web-server = icedWeb { pname = "harken-web-server"; demo = false; };
          # money's demo in a browser: a seeded household, its own authority,
          # nothing to sign in to (money/README.md); and what money-server
          # serves as its web directory, the same page signing in against
          # the server it is served from.
          moneyWeb = { pname, demo }: icedWeb {
            inherit pname demo;
            crate = "money-iced";
            files = [ ./money/iced/web/index.html ./money/iced/web/favicon.svg ];
          };
          money-web = moneyWeb { pname = "money-web"; demo = true; };
          money-web-server = moneyWeb { pname = "money-web-server"; demo = false; };
          # The docs (docs/site): a Cargo workspace of its own, with a
          # lockfile and a docs/site/Cargo.nix of its own, from a source
          # that is only the docs and the crates they name by path: arkui.
          # So the docs and ArkDB's kernel never rebuild each other: an edit
          # to the docs is no input to the kernel, the kernel is no input to
          # the docs, and arkui is an input to both. ArkDB's docs, when they
          # join, add its crates here.
          docsCrates = import ./nix/rust.nix {
            inherit lib;
            cargoNix = import ./docs/site/Cargo.nix { inherit pkgs; };
          };
          docsCrate = args: docsCrates.build ({ inherit pkgs rust; packageId = "docs"; } // args);
          # The site: plain HTML, CSS and a few lines of script, and each
          # desktop component's pictures drawn by iced's own renderer on the
          # CPU (tiny-skia: no window, no GPU), which the sandbox allows.
          docs = pkgs.runCommand "docs" { } "${docsCrate { }}/bin/docs $out";

          # The ffmpeg the screens workflow cuts the phones' recordings with
          # (.github/screens/encode.sh), and nothing else: H.264 out through
          # x264, the native decoders and the MP4 and MOV containers, the
          # filters (setpts, fps, scale, format, mpdecimate) and ffprobe. No
          # other codec, no network, no device, no docs — a tenth of what
          # Homebrew's ffmpeg builds and pulls in on the macOS runner.
          screens-ffmpeg = pkgs.ffmpeg-headless.override {
            withHeadlessDeps = false;
            withGPL = true;
            withX264 = true;
            withZlib = true;
            # Its own tests build against it.
            withPixelutils = true;
            withMultithread = true;
            buildFfmpeg = true;
            buildFfprobe = true;
            buildAvcodec = true;
            buildAvformat = true;
            buildAvfilter = true;
            buildAvutil = true;
            buildSwscale = true;
            buildSwresample = true;
            # Empty with no device on, but nixpkgs links its tools against it.
            buildAvdevice = true;
          };

          # XcodeGen, which writes showcase/ios's Xcode project for the
          # screens workflow: upstream's own release, a universal macOS binary
          # and its presets, pinned here, as nixpkgs 24.11 has none.
          xcodegen = pkgs.stdenvNoCC.mkDerivation rec {
            pname = "xcodegen";
            version = "2.46.0";
            src = pkgs.fetchzip {
              url = "https://github.com/yonaskolb/XcodeGen/releases/download/${version}/xcodegen.zip";
              hash = "sha256-7NjCsInBjkOL1RqWSaWKGDJxp/gn71rmhTWG2ErHDqA=";
            };
            # Signed as it is: stripping or patching it would break that.
            dontFixup = true;
            installPhase = "mkdir -p $out && cp -r bin share $out/";
            meta.platforms = [ "aarch64-darwin" "x86_64-darwin" ];
          };

          # The server (harken/server: the server and harken-peer): its
          # crate and what it reaches, under the features it asks for, and
          # none of the GUI the workspace's other members link.
          harken-server = crate { packageId = "harken-server"; };
          # `arkdb` for a server to read its system's events with
          # (docs/plan-observe.md): no GUI stack.
          arkdb = crate { packageId = "arkdb"; };
          # money's server (money/server: the hub and the bank importer), the
          # same way: no GUI stack under a server.
          money-server = crate { packageId = "money-server"; };

          # The desktop window. winit and wgpu open the display and the GPU
          # by dlopen at run time, so the libraries go on the library path.
          # money's demo window is the same.
          icedApp = packageId:
            let built = crate { inherit packageId; }; in
            if pkgs.stdenv.hostPlatform.isLinux then
              pkgs.runCommand (lib.removePrefix "rust_" built.name) { nativeBuildInputs = [ pkgs.makeWrapper ]; } ''
                makeWrapper ${built}/bin/${packageId} $out/bin/${packageId} --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath (with pkgs; [
                  wayland libxkbcommon vulkan-loader libGL xorg.libX11 xorg.libXcursor xorg.libXi xorg.libXrandr
                ])}
              ''
            else built;
          harken-iced = icedApp "harken-iced";
          money-iced = icedApp "money-iced";

          # The specification is `rust/ark` (spec/README.md), and the tools
          # over it are `rust/arkdb`, whose binary is `arkdb` (`gen`, `fuzz`,
          # and reading a system's events: `tail`, `trace`, `peer`): `ark` is
          # that build, and `fuzz` its fuzzer under the name `nix run` takes.
          ark = arkdb;
          # `nix run .#fuzz [SESSIONS] [FIRST SEED] [traces [DIR]]`: the
          # fuzzer (rust/arkdb/src/fuzz.rs, `arkdb fuzz`), ten thousand sessions
          # from seed 1 by default, each checked for convergence, replay
          # from facts and every frame's bytes; `traces` replays every
          # client's trace too and writes them under DIR. A failure prints
          # its seed and the run exits 1.
          fuzz = pkgs.writeShellApplication {
            name = "arkdb-fuzz";
            runtimeInputs = [ ark ];
            text = ''exec arkdb fuzz "$@"'';
          };
          # The vectors as the writer writes them now: the kernel's tests
          # (hash/) and arkdb's
          # (protocol/, trace/, rebase/, views/) run with ARK_WRITE_VECTORS over an
          # empty spec/vectors, which is then the output. checks.vectors
          # holds the tree's to it; `nix build .#vectors` and a copy over
          # spec/vectors is how they are written again without cargo.
          vectors = crateTests {
            name = "ark-vectors";
            crates = [
              { packageId = "ark"; member = "rust/ark"; }
              { packageId = "arkdb"; member = "rust/arkdb"; }
            ];
            # The writers only: the rest of both crates' tests are checks.rust's.
            targets = [ "hash" "vectors" "views" ];
            # arkdb's tests also hold spec/README.md and AUTHORING.md.
            dirs = rustDirs ++ [ "spec/README.md" "spec/AUTHORING.md" ];
            preCheck = ''
              rm -rf spec/vectors
              export ARK_WRITE_VECTORS=1
            '';
            out = "cp -r spec/vectors $out";
          };

          # Every client's life in every session of a fixed run of the
          # fuzzer, as traces (spec/RUNNERS.md § trace): what checks.traces
          # replays beside spec/vectors/trace/, and what a runtime in another
          # language replays to be held to this one. A function of the seed
          # and the session count, so not checked in, and moving either is a
          # deliberate edit; a run that finds anything exits 1 and there is
          # no corpus, since a trace is of correct behaviour.
          # Written, not replayed: checks.traces replays them.
          trace-corpus = pkgs.runCommand "trace-corpus" { nativeBuildInputs = [ ark ]; } ''
            arkdb fuzz 300 2026 traces $out
          '';

          # The phones (docs/plan-v5.md §1.16, N3): the Rust kernel through
          # UniFFI. `harken/ffi` is ark-ffi with harken's domain linked in, one
          # library; UniFFI writes its Kotlin and Swift from the library built
          # for the host (`uniffi-bindgen generate <library>`), and beside
          # them goes the bridge's typed half (rust/ark-ffi/{kotlin,swift}).
          # The bindgen reads each crate's uniffi.toml where `cargo metadata
          # --no-deps` finds the crate: in the workspace's manifests, which
          # need nothing fetched.
          ffiBindings = { lib, language, out ? "$TMPDIR/bindings" }: ''
            (cd ${only rustDirs}/rust && HOME=$TMPDIR PATH=${rust}/bin:$PATH \
              ${uniffiBindgen}/bin/uniffi-bindgen generate ${lib} --language ${language} --no-format \
                --metadata-no-deps --out-dir "${out}/${language}")
          '';
          # UniFFI's bindgen binary (rust/ark-ffi/src/bin/uniffi-bindgen.rs),
          # which is UniFFI's main: ark-ffi with its `bindgen` feature.
          uniffiBindgen = crate { packageId = "ark-ffi"; features = [ "default" "bindgen" ]; };
          # An app's library for the host (`crate` its ffi crate, `so` the
          # library's name), which UniFFI reads the bindings from and the
          # app's JVM tests load.
          ffiHostLib = { crate, so, ... }:
            "${(rustCrates.build { inherit pkgs rust; packageId = crate; }).lib}/lib/lib${so}${pkgs.stdenv.hostPlatform.extensions.sharedLibrary}";
          harkenFfi = { name = "harken"; crate = "harken-ffi"; so = "harken_ffi"; };
          moneyFfi = { name = "money"; crate = "money-ffi"; so = "money_ffi"; };
          # The ABIs the APK carries — a phone, an old phone, an emulator —
          # each nixpkgs for that Android against API 26 (the app's minSdk),
          # with the NDK's prebuilt clang, which links each and compiles
          # ring's C.
          androidTargets = {
            "aarch64-linux-android" = { abi = "arm64-v8a"; platform = lib.systems.examples.aarch64-android-prebuilt; };
            "armv7-linux-androideabi" = { abi = "armeabi-v7a"; platform = lib.systems.examples.armv7a-android-prebuilt; };
            "x86_64-linux-android" = {
              abi = "x86_64";
              platform = {
                config = "x86_64-unknown-linux-android";
                rust.rustcTarget = "x86_64-linux-android";
                androidNdkVersion = "26";
                useAndroidPrebuilt = true;
              };
            };
          };
          # An app's library for its phones (`harken/ffi`, `money/ffi`) as the
          # APK carries it: `name` the app, `crate` its ffi crate, `so` the
          # library's name (`harken_ffi`).
          androidPhone = { name, crate, so }:
            let
              hostLib = ffiHostLib { inherit crate so; };
              # `so` for one Android ABI, stripped, as `jniLibs/<abi>/lib<so>.so`.
              androidLib = t: a:
                let
                  built = rustCrates.build {
                    pkgs = crossPkgs (a.platform // { androidSdkVersion = "26"; });
                    rust = rust.override { targets = [ t ]; };
                    packageId = crate;
                  };
                in
                pkgs.runCommand "${name}-android-${a.abi}" { } ''
                  mkdir -p $out/jniLibs/${a.abi}
                  ${pkgs.llvmPackages.bintools-unwrapped}/bin/llvm-strip \
                    -o $out/jniLibs/${a.abi}/lib${so}.so ${built.lib}/lib/lib${so}.so
                '';
              # The Kotlin the app compiles with the library, UniFFI's
              # bindings and the typed half, under `kotlin/`; and the library
              # built for this machine as `host/lib<so>.so`, which the app's
              # JVM tests load.
              androidHost = pkgs.runCommand "${name}-android-host" { } ''
                ${ffiBindings { lib = hostLib; language = "kotlin"; }}
                mkdir -p $out/kotlin/dev/arkdb/ffi
                cp -r $TMPDIR/bindings/kotlin/. $out/kotlin/
                install -Dm755 ${hostLib} $out/host/lib${so}.so
                cp ${./rust/ark-ffi/kotlin/Typed.kt} $out/kotlin/dev/arkdb/ffi/Typed.kt
              '';
            in
            # Each ABI's library and the host's part, one directory: what the
            # app's APK builds over.
            pkgs.runCommand "${name}-android-lib-0.1.0" { } ''
              mkdir -p $out
              for part in ${lib.concatStringsSep " " (lib.mapAttrsToList androidLib androidTargets)} ${androidHost}; do
                cp -r --no-preserve=mode $part/. $out/
              done
              chmod 755 $out/host/lib${so}.so
            '';
          harken-android-lib = androidPhone harkenFfi;
          # money's (`money/ffi`): what its APK builds over.
          money-android-lib = androidPhone moneyFfi;

          # The Android SDK the APK is built with. Google ships one for
          # x86_64 Linux and for macOS; on Linux the SDK is assembled by hand
          # instead (nix/android-sdk.nix) — the platform and build-tools jars
          # from Google's zips, and aapt2, the one native tool AGP runs,
          # compiled for the host (nix/aapt2.nix) — so an ARM Linux builds the
          # phone natively. macOS takes nixpkgs' androidenv.
          aapt2 = pkgs.callPackage ./nix/aapt2.nix { stdenv = pkgs.clangStdenv; };
          androidSdk = pkgs.callPackage ./nix/android-sdk.nix { inherit aapt2; };
          sdkRoot =
            if pkgs.stdenv.hostPlatform.isLinux then "${androidSdk}/${androidSdk.root}"
            else "${(pkgs.androidenv.composeAndroidPackages {
              platformVersions = [ "35" ];
              buildToolsVersions = [ "35.0.0" ];
            }).androidsdk}/libexec/android-sdk";

          # An Android app of this tree (`<name>/android`, a gradle project
          # with one `:app`): gradle over the SDK, assembled offline from a
          # recorded Maven graph (`<name>/android/deps.json`; `nix run
          # .#<name>-apk-deps` re-records it). The app's JVM tests run first,
          # so an APK is not built over a screen or a rule that fails them.
          #
          # The release build, `<name>.apk`: not debuggable, so ART compiles
          # it as it would any app, with the baseline profiles Compose's
          # libraries ship. Not minified: R8 is not in the recorded graph.
          # Signed with the app's own `debug.keystore`, committed and public
          # on purpose (harken/android/app/build.gradle.kts says why), so each
          # build installs over the last; and a version code that rises with
          # each commit, the flake's own source time in minutes.
          #
          # `screens` is the same build recording its Paparazzi screenshots
          # (every screen and state, light and dark, rendered on the JVM
          # through layoutlib) and an index.html laying them out light beside
          # dark (nix/screens-index.py). `flags` are the app's own gradle
          # properties; `title` and `note` head that page. `shared` are the
          # directories of this tree the app compiles beside its own:
          # `kotlin/arkui`, the look every Ark app on Android shares.
          androidApp = { name, appid, flags ? [ ], shared ? [ ], title, note }:
            let
              dir = "${name}/android";
              apk = pkgs.stdenv.mkDerivation (final: {
                pname = "${name}-apk";
                version = "0.1.0";
                src = only ([ dir ] ++ shared);
                sourceRoot = "source/${dir}";
                postUnpack = "chmod -R u+w source";
                nativeBuildInputs = [ pkgs.gradle pkgs.jdk21 ];
                mitmCache = pkgs.gradle.fetchDeps {
                  pkg = final.finalPackage;
                  data = ./. + "/${dir}/deps.json";
                };
                __darwinAllowLocalNetworking = true;
                ANDROID_HOME = sdkRoot;
                ANDROID_SDK_ROOT = sdkRoot;
                gradleFlags = [
                  "-Dorg.gradle.java.home=${pkgs.jdk21.home}"
                  "-Porg.gradle.java.installations.auto-detect=false"
                  "-Porg.gradle.java.installations.auto-download=false"
                  "-Porg.gradle.java.installations.paths=${pkgs.jdk21.home}"
                  # AGP would fetch its own aapt2 from Maven, an unpatched binary
                  # that cannot run here; the SDK's is patched and can.
                  "-Pandroid.aapt2FromMavenOverride=${sdkRoot}/build-tools/35.0.0/aapt2"
                  # Whatever the SDK lacks is an error, not a download: an
                  # install reads Google's repository indexes, which change
                  # under any hash recorded of them (nix/android-sdk.nix).
                  "-Pandroid.builder.sdkDownload=false"
                ] ++ flags ++ final.versionFlags;
                # Read by app/build.gradle.kts; a local build falls back to 1.
                versionFlags = [
                  "-PversionCode=${toString (self.lastModified / 60)}"
                  "-PversionName=0.1.${self.lastModifiedDate}"
                ];
                # AGP keeps its state under ~/.android, and the builder has no home.
                preBuild = ''
                  export HOME="$TMPDIR/home" ANDROID_USER_HOME="$TMPDIR/home/.android"
                  mkdir -p "$ANDROID_USER_HOME"
                '';
                gradleBuildTask = ":app:testDebugUnitTest :app:assembleRelease";
                # The recording runs the same tasks, and the screenshots' and a
                # laptop's debug build, so that one recording serves all three.
                gradleUpdateTask = ":app:testDebugUnitTest :app:recordPaparazziDebug :app:assembleDebug :app:assembleRelease";
                doCheck = false;
                installPhase = ''
                  mkdir -p $out
                  cp app/build/outputs/apk/release/app-release.apk $out/${name}.apk
                '';
              });
              screens = apk.overrideAttrs (old: {
                pname = "${name}-android-screens";
                gradleBuildTask = ":app:recordPaparazziDebug";
                versionFlags = [ ];
                installPhase = ''
                  mkdir -p $out
                  shots=app/src/test/snapshots/images
                  for f in $shots/*.png; do
                    b=$(basename "$f" .png)
                    cp "$f" "$out/''${b#${appid}_Screens_}.png"
                  done
                  ${pkgs.python3}/bin/python3 ${./nix/screens-index.py} $out ${lib.escapeShellArg title} ${lib.escapeShellArg note} > $out/index.html
                '';
              });
            in
            { inherit apk screens; deps = apk.mitmCache.updateScript; };

          # The phone (harken/android/README.md): the app — its onboarding
          # and its look are its own code — over `harken-android-lib` (the
          # kernel per ABI, and the Kotlin over it). Its screens are rendered
          # from a fixed library (harken/android/app/src/test).
          harken = androidApp {
            name = "harken";
            appid = "dev.harken.android";
            flags = [ "-PharkenLib=${harken-android-lib}" ];
            shared = [ "kotlin/arkui" ];
            title = "Harken";
            note = "Rendered on the JVM by Paparazzi from a fixed library, at a Pixel 5's size; light beside dark. Covers are the derived squares: nothing here talks to a server.";
          };
          harken-apk = harken.apk;
          harken-android-screens = harken.screens;

          # money's phone (money/android/README.md): Compose over
          # `money-android-lib`, the look shared with harken (kotlin/arkui).
          # Its Maven graph is harken's, replayed (money/android/deps.json).
          # `appid` here is the package the screenshots are named by; the
          # installed app is us.koon.money.
          money = androidApp {
            name = "money";
            appid = "dev.money.android";
            flags = [ "-PmoneyLib=${money-android-lib}" ];
            shared = [ "kotlin/arkui" ];
            title = "money";
            note = "Rendered on the JVM by Paparazzi from the demo's household on 2026-10-26, at a Pixel 5's size, in money's own scheme (a screenshot has no wallpaper); light beside dark.";
          };
          money-apk = money.apk;
          money-android-screens = money.screens;

          # The Bible as one scroll, zoomed like a map (revelation/README.md,
          # docs/plan-bible.md): plain Kotlin, no native library and none of
          # ArkDB, so it is the SDK and gradle alone.
          revelation = androidApp {
            name = "revelation";
            appid = "dev.revelation.android";
            title = "Revelation";
            note = "Rendered on the JVM by Paparazzi through layoutlib; light beside dark.";
          };

          # Every app as one F-Droid repository (nix/fdroid.nix): each app is
          # its id, its APK, its metadata and its icon.
          fdroid = import ./nix/fdroid.nix {
            inherit lib pkgs;
            apps = [
              {
                appid = "dev.harken.android";
                apk = harken-apk;
                apkName = "harken.apk";
                metadata = ./harken/android/fdroid/metadata/dev.harken.android.yml;
                # The trumpet the browser's tab draws.
                icon = ./harken/iced/web/favicon.svg;
              }
              {
                appid = "dev.revelation.android";
                apk = revelation.apk;
                apkName = "revelation.apk";
                metadata = ./revelation/android/fdroid/metadata/dev.revelation.android.yml;
                icon = ./revelation/android/fdroid/icon.svg;
              }
              {
                appid = "us.koon.money";
                apk = money-apk;
                apkName = "money.apk";
                metadata = ./money/android/fdroid/metadata/us.koon.money.yml;
                icon = ./money/android/fdroid/icon.svg;
              }
            ];
          };

          # The same library for iOS — `aarch64-apple-ios` for a phone and
          # `aarch64-apple-ios-sim` for the simulator on an Apple-silicon Mac —
          # as `<Framework>.xcframework` (`HarkenFFI`, `MoneyFFI`), with
          # UniFFI's Swift and the typed half under `swift/`. Linking for iOS
          # needs Apple's SDK, which only a macOS builder has: on Linux these
          # derivations evaluate and are not built (meta.platforms), and they
          # have not been built anywhere yet.
          # It is cargo's build, over the lockfile's crates vendored, and not
          # a derivation per crate: nixpkgs has no iOS package set that
          # compiles with the builder's Xcode (the workflow, ios.yaml, builds
          # it with cargo too).
          iosTargets = [ "aarch64-apple-ios" "aarch64-apple-ios-sim" ];
          iosRust = rust.override { targets = iosTargets; };
          # One app's: `name`, `crate` and `so` as `androidPhone`'s, and the
          # framework's name.
          iosPhone = { name, crate, so, framework }: pkgs.stdenv.mkDerivation {
            pname = "${name}-ios-xcframework";
            version = "0.1.0";
            src = only rustDirs;
            sourceRoot = "source/rust";
            cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ./rust/Cargo.lock; };
            nativeBuildInputs = [ iosRust pkgs.rustPlatform.cargoSetupHook ];
            # xcodebuild and the iPhoneOS and simulator SDKs: the builder's
            # Xcode, outside the store.
            __noChroot = true;
            # The iOS targets compile and link with the builder's Xcode: its
            # developer directory, clang, ar and SDKs. nixpkgs' darwin stdenv
            # sets DEVELOPER_DIR and SDKROOT to its own macOS SDK, which has no
            # iPhoneOS platform, so `xcrun --sdk iphoneos` (cc-rs asks it)
            # fails under them: both are taken back for these two targets. The
            # Swift is UniFFI's from the library built for this Mac.
            buildPhase = ''
              runHook preBuild
              export PATH=/usr/bin:$PATH
              export DEVELOPER_DIR="$(/usr/bin/xcode-select -p)"
              unset SDKROOT
              ${lib.concatMapStrings (t: let u = lib.toUpper (lib.replaceStrings [ "-" ] [ "_" ] t); l = lib.replaceStrings [ "-" ] [ "_" ] t; in ''
                export CC_${l}=/usr/bin/clang CXX_${l}=/usr/bin/clang++ AR_${l}=/usr/bin/ar
                export CARGO_TARGET_${u}_LINKER=/usr/bin/clang
                cargo build --release --offline --locked -p ${crate} --lib --target ${t}
              '') iosTargets}
              ${ffiBindings { lib = ffiHostLib { inherit crate so; }; language = "swift"; }}
              mkdir -p headers
              cp $TMPDIR/bindings/swift/*.h headers/
              cat $TMPDIR/bindings/swift/*.modulemap > headers/module.modulemap
              /usr/bin/xcodebuild -create-xcframework \
                -library target/aarch64-apple-ios/release/lib${so}.a -headers headers \
                -library target/aarch64-apple-ios-sim/release/lib${so}.a -headers headers \
                -output ${framework}.xcframework
              runHook postBuild
            '';
            installPhase = ''
              mkdir -p $out/swift
              cp -r ${framework}.xcframework $out/
              cp $TMPDIR/bindings/swift/*.swift ark-ffi/swift/Typed.swift $out/swift/
            '';
            meta.platforms = [ "aarch64-darwin" "x86_64-darwin" ];
          };
          harken-ios-xcframework = iosPhone (harkenFfi // { framework = "HarkenFFI"; });
          money-ios-xcframework = iosPhone (moneyFfi // { framework = "MoneyFFI"; });

          # harken's domain (harken/domain: plain Rust, §1.16) built, and
          # its schema's hash held to the one its tests pin.
          harken-domain = crateTests {
            name = "harken-domain";
            crates = [ { packageId = "harken-domain"; member = "harken/domain"; } ];
            targets = [ "port" ];
            args = "the_schema_hash_is_pinned";
          };

          # The workspace's members (rust/Cargo.toml), each its package and
          # its directory: what the checks test and lint one by one, so that
          # each is a derivation of its own and a change to one crate runs
          # its tests and lints it, and every other's are the cache's. A
          # library's doctests run with its tests, as cargo has them.
          members = map (m:
            let
              member = if lib.hasPrefix "../" m then lib.removePrefix "../" m else "rust/${m}";
              toml = builtins.fromTOML (builtins.readFile (./. + "/${member}/Cargo.toml"));
            in
            {
              packageId = toml.package.name;
              inherit member;
              doctests = builtins.pathExists (./. + "/${member}/src/lib.rs") && !(toml.lib.proc-macro or false);
            })
            (builtins.fromTOML (builtins.readFile ./rust/Cargo.toml)).workspace.members;
          # A crate's rustc, as a shell function its build defines before it
          # compiles anything: clippy's driver in rustc's place, denying
          # warnings (buildRustCrate caps every crate's lints, so the cap is
          # dropped), and for harken-server's tests the paths of its two
          # binaries, which cargo gives a crate's tests (`CARGO_BIN_EXE_*`).
          clippyRustc = pkgs.writeShellScript "clippy-rustc" ''
            args=()
            while [ $# -gt 0 ]; do
              case "$1" in
                --cap-lints) shift 2 ;;
                *) args+=("$1"); shift ;;
              esac
            done
            exec ${rust}/bin/clippy-driver "''${args[@]}" -D warnings
          '';
          rustcHook = { packageId, clippy ? false }: old: {
            preConfigure = (old.preConfigure or "") + ''
              rustc() {
                env ${lib.optionalString (packageId == "harken-server")
                  "CARGO_BIN_EXE_harken-server=${harken-server}/bin/harken-server CARGO_BIN_EXE_harken-peer=${harken-server}/bin/harken-peer"} \
                  ${if clippy then clippyRustc else "${rust}/bin/rustc"} "$@"
              }
            '';
          };
          # One member's tests (and harken-iced's again with its demo, a
          # feature), run from a tree that is what any of them reads.
          memberTests = m: crateTests {
            name = m.packageId + lib.optionalString (m ? features) "-demo";
            crates = [ m ];
            dirs = rustDirs ++ genDirs ++ [ "spec/vectors" "spec/README.md" "spec/AUTHORING.md" ];
            # What makes a speaker's copy of a hi-res file (harken/server/tests/speaker.rs).
            nativeBuildInputs = [ pkgs.ffmpeg-headless ];
            rootAttrs = rustcHook { inherit (m) packageId; };
          };
          demos = map (app: { packageId = "${app}-iced"; member = "${app}/iced"; features = [ "default" "demo" ]; }) [ "harken" "money" ];
          # One member lint-clean, as `cargo clippy --all-targets -- -D
          # warnings`: its libraries and binaries, and again as its tests
          # build them.
          memberClippy = build: m: map (tests: build {
            inherit (m) packageId;
            features = m.features or [ "default" ];
            inherit tests;
            rootAttrs = rustcHook { inherit (m) packageId; clippy = true; };
          }) [ false true ];
          # Every `.rs` under `dir` formatted as the rustfmt.toml nearest it says.
          rustfmtCheck = name: dir: pkgs.runCommand "check-${name}-fmt" { nativeBuildInputs = [ rust ]; } ''
            cd ${dir}
            find . -name '*.rs' -print0 | xargs -0 rustfmt --check --edition 2021
            touch $out
          '';
          # Derivations as one, for a check that is many.
          allOf = name: drvs: pkgs.linkFarm name (lib.imap0 (i: d: { name = "${toString i}-${d.name}"; path = d; }) drvs);

          # The pinned previous revisions (nix/versions.nix), each its
          # `harken-server` package as that revision's own flake builds it:
          # both binaries, the server and harken-peer. In order, which is
          # the `n` of `HARKEN_OLD_<n>_*`.
          system = pkgs.stdenv.hostPlatform.system;
          versions = map (v:
            let input = inputs."harken-${v.name}"; in
            assert lib.assertMsg (input.rev == v.rev)
              "flake.nix: the input harken-${v.name} is locked at ${input.rev}, and nix/versions.nix says ${v.rev}";
            v // {
              server = input.packages.${system}.harken-server;
            })
            (import ./nix/versions.nix);
          # What the fleet reads to find them (harken/server/tests/support/fleet.rs).
          # With none pinned it says so, and the scenarios that need an older
          # binary say they are skipped; the ones that need none still run.
          oldEnv = if versions == [ ] then ''
            echo "nix/versions.nix pins no previous revision: the version scenarios that need one are skipped"
          '' else lib.concatImapStrings (n: v: ''
            export HARKEN_OLD_${toString n}_NAME=${v.name}
            export HARKEN_OLD_${toString n}_SERVER=${v.server}/bin/harken-server
            export HARKEN_OLD_${toString n}_PEER=${v.server}/bin/harken-peer
          '') versions;
          # `nix run .#update-cargo-nix`, from the repository's root: each
          # workspace's Cargo.nix (rust/, docs/site/) written again from its
          # lockfile by crate2nix, what nix/rust.nix builds every crate from.
          # A dependency on one target named by its triple
          # (`[target.<triple>.dependencies]`) is read off the target the
          # crate is built for, as crate2nix itself has it after 0.14,
          # rather than off nixpkgs' host. The workflow holds the committed
          # files to what this writes.
          update-cargo-nix = pkgs.writeShellApplication {
            name = "update-cargo-nix";
            runtimeInputs = [ pkgs.crate2nix rust pkgs.gnused ];
            text = ''
              for dir in rust docs/site; do
                (cd "$dir" && crate2nix generate -f Cargo.toml -o Cargo.nix)
                sed -i 's/(stdenv\.hostPlatform\.rust\.rustcTarget == /(target."name" == /g' "$dir/Cargo.nix"
              done
            '';
          };
          # The catalogue as its repository builds it, and the copy the demo
          # embeds made from it.
          classical = pkgs.writeText "catalogue.json" (import "${inputs.classical}/catalogue.nix");
          update-catalogue = pkgs.writeShellScriptBin "update-catalogue" ''
            cp ${classical} harken/iced/src/catalogue.json
            chmod 644 harken/iced/src/catalogue.json
            echo "harken/iced/src/catalogue.json is the catalogue at ${inputs.classical.shortRev or "?"}"
          '';
        in
        {
          packages = {
            inherit ark vectors fuzz harken-domain trace-corpus harken-android-lib harken-ios-xcframework;
            inherit harken-server arkdb;
            # `nix run .#update-catalogue`, from the repository's root: the
            # demo's copy of the catalogue made from the `classical` input.
            inherit classical update-catalogue update-cargo-nix;
            # `nix run .#harken-serve [ADDR]`: the dev server, anyone is
            # whoever they say.
            harken-serve = pkgs.callPackage ./harken/server/nix/serve.nix { inherit harken-server; };
            inherit harken-web harken-web-server harken-iced docs screens-ffmpeg xcodegen;
            inherit money-iced money-web money-web-server money-server;
            # money's library for its phones (money/ffi), as harken's.
            inherit money-android-lib money-ios-xcframework;
            default = ark;
            inherit harken-apk harken-android-screens;
            harken-apk-deps = harken.deps;
            revelation-apk = revelation.apk;
            revelation-android-screens = revelation.screens;
            revelation-apk-deps = revelation.deps;
            # money's phone, over money-android-lib (money/android/README.md).
            inherit money-apk money-android-screens;
            money-apk-deps = money.deps;
            # The workflow builds these first and keeps them as gc roots.
            inherit aapt2;
            android-sdk = androidSdk;
            # The APKs' F-Droid repository, one for both apps, and a key for
            # it (nix/fdroid.nix). `harken-fdroid` is the tool's old name,
            # kept while a workflow on main still calls it.
            inherit (fdroid) fdroid-repo apps-fdroid fdroid-keygen;
            harken-fdroid = fdroid.apps-fdroid;
          } // lib.listToAttrs (map (v: lib.nameValuePair "harken-server-${v.name}" v.server) versions)
          // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            # The fleet on three NixOS machines (docs/plan-fleet.md §3): an
            # interface down, the service restarted, the server crashed, the
            # scanner over a bound directory. A package and not a check: it
            # wants /dev/kvm, and under TCG it takes most of an hour.
            fleet-vm = import ./harken/server/nix/fleet-vm.nix {
              inherit pkgs;
              module = import ./harken/server/nix/module.nix { packages = self.packages; };
              # Both binaries: the server, and harken-peer beside it.
              inherit harken-server;
              # The previous pinned revision's, for the machines run mixed
              # (docs/plan-db.md D1); none while nix/versions.nix is empty,
              # and the mixed machines and subtests are left out.
              old = if versions == [ ] then null else (lib.last versions).server;
            };
          };

          checks = {
            # The demo's copy of the catalogue is the `classical` input's:
            # `nix flake update classical` then `nix run .#update-catalogue`.
            catalogue = pkgs.runCommand "catalogue-current" { } ''
              cmp ${classical} ${./harken/iced/src/catalogue.json} || {
                echo "harken/iced/src/catalogue.json is not the classical input's: nix run .#update-catalogue" >&2
                exit 1
              }
              touch $out
            '';
            # The fleet run mixed (docs/plan-db.md D1): every pinned
            # revision's harken-server package built from its own flake, and
            # harken/server/tests/versions.rs run with each named as
            # HARKEN_OLD_<n>_* — an old peer beside new ones, an old server
            # upgraded in place under its peers, a client upgraded in place,
            # a schema grown under running peers. In the sandbox, on
            # loopback, in release; the scenarios' own timings are printed.
            # With nix/versions.nix empty it runs the scenarios that need no
            # older binary (a schema grown, a module updated over a retained
            # log) and the others say they are skipped and why.
            versions = crateTests {
              name = "harken-versions";
              crates = [ { packageId = "harken-server"; member = "harken/server"; } ];
              targets = [ "versions" ];
              args = "--nocapture";
              rootAttrs = rustcHook { packageId = "harken-server"; };
              preCheck = oldEnv;
            };
            # The vectors in the tree are the ones the writer writes.
            vectors = pkgs.runCommand "check-vectors" { } ''
              diff -r ${vectors} ${./spec/vectors} && touch $out
            '';
            # Every trace of the corpus replays (rust/arkdb/tests/vectors.rs,
            # `trace_corpus`; spec/vectors/trace/ and the fleets' and fuzz
            # sessions' traces are replayed by checks.rust, once each). Its
            # own check rather than part of checks.rust, so that the
            # workspace's tests do not wait on a fuzz run and a trace that
            # stops replaying is named as that.
            traces = crateTests {
              name = "ark-traces";
              crates = [ { packageId = "arkdb"; member = "rust/arkdb"; } ];
              targets = [ "vectors" ];
              args = "trace_corpus --nocapture";
              dirs = rustDirs ++ [ "spec/vectors" ];
              preCheck = "export ARK_TRACE_CORPUS=${trace-corpus}";
            };
            # The fuzzer's binary still runs and checks: a fixed seed and a
            # few sessions, so that it cannot rot. Their traces are replayed
            # by checks.rust (rust/arkdb/tests/fuzz.rs, seeds 1-200).
            fuzz-smoke = pkgs.runCommand "check-fuzz-smoke" { nativeBuildInputs = [ ark ]; } ''
              arkdb fuzz 25 1
              touch $out
            '';
            # The Rust workspace's tests (which hold spec/README.md's index
            # and AUTHORING.md's examples too), a run per member; formatted
            # and lint-clean are `fmt` and `clippy`.
            rust = allOf "arkdb-rust" (map memberTests (members ++ demos));
            # The workspace lint-clean, the demo feature too, a build per
            # member.
            clippy = allOf "arkdb-clippy" (lib.concatMap (memberClippy crate) (members ++ demos));
            # The docs' crate formatted, lint-clean and its tests passing.
            docs = allOf "docs-checks" ([
              (rustfmtCheck "docs" (only [ "docs/site" ]))
              (crateTests {
                name = "docs";
                build = docsCrate;
                crates = [ { packageId = "docs"; member = "docs/site"; } ];
                dirs = [ "docs/site" "rust/arkui" "rust/Cargo.toml" ];
              })
            ] ++ memberClippy docsCrate { packageId = "docs"; });
            # The workspace formatted as each crate's rustfmt.toml says.
            fmt = rustfmtCheck "arkdb" (only rustDirs);
            # harken's domain crate builds, and its schema's hash is the one
            # pinned (harken/domain/tests/port.rs).
            inherit harken-domain;
            # The bridge (§1.16, N3): ark-ffi, harken/ffi and money/ffi build
            # natively, their tests drive the whole surface in the process (a
            # server in it, a mutate and its refusal, a view's patches, the
            # live space, alone and joining, a hall of devices, money's demo)
            # and hold the generated Kotlin and Swift to the domains; and
            # UniFFI writes the Kotlin and the Swift for both libraries.
            ffi =
              let
                bindings = app: out: ''
                  ${ffiBindings { lib = ffiHostLib app; language = "kotlin"; inherit out; }}
                  ${ffiBindings { lib = ffiHostLib app; language = "swift"; inherit out; }}
                  ls "${out}/swift/"*FFI.h "${out}/swift/"*.modulemap
                '';
              in
              crateTests {
                name = "arkdb-ffi";
                crates = [
                  { packageId = "ark-ffi"; member = "rust/ark-ffi"; }
                  { packageId = "harken-ffi"; member = "harken/ffi"; }
                  { packageId = "money-ffi"; member = "money/ffi"; }
                ];
                dirs = rustDirs ++ genDirs;
                postCheck = ''
                  ${bindings harkenFfi "$TMPDIR/bindings"}
                  grep -q 'fun `harkenOpen`' "$TMPDIR/bindings/kotlin/dev/harken/ffi/harken_ffi.kt"
                  grep -q "class Peer" "$TMPDIR/bindings/kotlin/dev/arkdb/ffi/ark_ffi.kt"
                  grep -q "func harkenOpen" "$TMPDIR/bindings/swift/harken_ffi.swift"
                  ${bindings moneyFfi "$TMPDIR/money"}
                  grep -q 'fun `moneyOpen`' "$TMPDIR/money/kotlin/dev/money/ffi/money_ffi.kt"
                  grep -q "class TxnLineView" "$TMPDIR/money/kotlin/uniffi/money_domain/money_domain.kt"
                  grep -q "func moneyDemo" "$TMPDIR/money/swift/money_ffi.swift"
                '';
                # The bindings it wrote: harken's, which the iOS type-check
                # reads, and money's under `money/`, which money's reads.
                out = "cp -r $TMPDIR/bindings $out && cp -r $TMPDIR/money $out/money";
              };
          } // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            # What of the iOS app Linux can check (harken/ios/tools/typecheck.sh):
            # Rows.swift, Bridge.swift and Model.swift with the generated types,
            # UniFFI's Swift and the bridge's typed half, type-checked by
            # nixpkgs' Swift against the library's headers and a stand-in for
            # SwiftUI. The views are SwiftUI's and are not checked here.
            harken-ios-typecheck = pkgs.swiftPackages.stdenv.mkDerivation {
              name = "harken-ios-typecheck";
              src = only [ "harken/ios" "rust/ark-ffi/swift" "swift/ArkUI" ];
              nativeBuildInputs = [ pkgs.swift ];
              buildInputs = [ pkgs.swiftPackages.Foundation pkgs.swiftPackages.Dispatch ];
              # nixpkgs' Swift 5.8 on Linux reports x86_64-pc-linux-gnu where
              # its standard library's modules are x86_64-unknown-linux-gnu. And
              # it predates `nonisolated(unsafe)` (5.10), which UniFFI's Swift
              # writes twice for Swift 6's checks: taken out of this copy only.
              buildPhase = ''
                mkdir -p bin bindings
                printf '#!/bin/sh\nexec swiftc -target ${pkgs.stdenv.hostPlatform.parsed.cpu.name}-unknown-linux-gnu "$@"\n' > bin/swiftc-here
                chmod +x bin/swiftc-here
                cp ${self.checks.${system}.ffi}/swift/* bindings/
                chmod u+w bindings/*
                sed -i 's/nonisolated(unsafe) //' bindings/*.swift
                SWIFT_EXEC=$PWD/bin/swiftc-here TYPED=$PWD/rust/ark-ffi/swift/Typed.swift \
                  bash harken/ios/tools/typecheck.sh bindings
              '';
              installPhase = "touch $out";
            };
            # The same for money's iOS app (money/ios/tools/typecheck.sh): its
            # budget arithmetic, first run, session and model with the generated
            # types, UniFFI's Swift for money's library and ArkUICore.
            money-ios-typecheck = pkgs.swiftPackages.stdenv.mkDerivation {
              name = "money-ios-typecheck";
              src = only [ "money/ios" "rust/ark-ffi/swift" "swift/ArkUI" ];
              nativeBuildInputs = [ pkgs.swift ];
              buildInputs = [ pkgs.swiftPackages.Foundation pkgs.swiftPackages.Dispatch ];
              buildPhase = ''
                mkdir -p bin bindings
                printf '#!/bin/sh\nexec swiftc -target ${pkgs.stdenv.hostPlatform.parsed.cpu.name}-unknown-linux-gnu "$@"\n' > bin/swiftc-here
                chmod +x bin/swiftc-here
                cp ${self.checks.${system}.ffi}/money/swift/* bindings/
                chmod u+w bindings/*
                sed -i 's/nonisolated(unsafe) //' bindings/*.swift
                SWIFT_EXEC=$PWD/bin/swiftc-here TYPED=$PWD/rust/ark-ffi/swift/Typed.swift \
                  bash money/ios/tools/typecheck.sh bindings
              '';
              installPhase = "touch $out";
            };
            # The NixOS module, evaluated under the configurations that
            # matter and held to its assertions, its warning and its unit.
            harken-module = pkgs.callPackage ./harken/server/nix/module-test.nix {
              module = import ./harken/server/nix/module.nix { packages = self.packages; };
            };
            # money's, as harken's: services.money evaluated and held to its
            # assertions and units.
            money-module = pkgs.callPackage ./money/server/nix/module-test.nix {
              module = import ./money/server/nix/module.nix { packages = self.packages; };
            };
          };

          # Everything the workflow publishes, as data: a page goes up at
          # k2on.github.io/apps/<name>/, an APK into the F-Droid repository
          # (and onto a tag's release). Adding one is adding it here; the
          # workflow builds each in a job of its own.
          #
          # `key` is what says whether it changed: its derivation's hash,
          # which moves exactly when something it is built from moves. An
          # APK's is taken without its version flags, since its version code
          # is the commit time and would move on every commit; the same
          # build without them is the APK's inputs and nothing else. The
          # workflow skips what it has already built under that key, and
          # with Cachix whatever does build is substituted, not compiled.
          publish =
            let
              key = drv: builtins.substring 0 32 (baseNameOf (builtins.unsafeDiscardStringContext drv.drvPath));
              page = name: package: title: about: { inherit name package title about; key = key self.packages.${pkgs.system}.${package}; };
              apk = name: app: { first ? [ ] }: {
                inherit name first;
                package = "${name}-apk";
                file = "${name}.apk";
                key = key (app.apk.overrideAttrs (_: { versionFlags = [ ]; }));
              };
            in
            {
              pages = [
                (page "harken" "harken-web" "harken" "the browser peer: the same engine compiled to wasm. Open it alone to run it entirely in the tab, or point it at a harken server.")
                (page "docs" "docs" "docs" "the components every app here shares (arkui), a page each, pictured by arkui itself, and catalogue.json beside it.")
              ];
              apks = [
                # The kernel for every ABI first, so its failure reads on its own.
                (apk "harken" harken { first = [ "harken-android-lib" ]; })
                (apk "revelation" revelation { })
                # money's kernel for every ABI first, as harken's.
                (apk "money" money { first = [ "money-android-lib" ]; })
              ];
            };

          devShells = {
            rust = pkgs.mkShell {
              packages = [ (rust.override { extensions = [ "rust-src" "rust-analyzer" ]; }) pkgs.pkg-config ];
            };
            # What `nix build .#harken-web` builds with, for doing it by hand
            # (harken/iced/README.md); pages.yml also keeps it as a gc root so
            # the cached store holds the wasm-bindgen CLI it compiled.
            harken-web = pkgs.mkShell {
              packages = [ wasmRust wasm-bindgen-cli pkgs.binaryen ];
            };
            default = pkgs.mkShell {
              packages = [ rust pkgs.pkg-config ];
            };
          };
        };

      outputs = forAll perSystem;
    in
    {
      packages = lib.mapAttrs (_: o: o.packages) outputs;
      # What .github/workflows/build.yaml builds and publishes, read by its
      # `plan` job (`nix eval --json .#publish.x86_64-linux`).
      publish = lib.mapAttrs (_: o: o.publish) outputs;
      checks = lib.mapAttrs (_: o: o.checks) outputs;
      devShells = lib.mapAttrs (_: o: o.devShells) outputs;
      # `services.harken`: the sync server, sign-in, /media and the
      # browser client on one port.
      nixosModules.default = import ./harken/server/nix/module.nix { packages = self.packages; };
      # The same, by name; and `services.money`: the household's budget,
      # its sync server, browser client and bank importer on one port. Its
      # own module rather than part of `default`, so a machine that imports
      # `default` for harken is not given money's options too.
      nixosModules.harken = self.nixosModules.default;
      nixosModules.money = import ./money/server/nix/module.nix { packages = self.packages; };
    };
}
