{
  description = "ArkDB: the specification and runtime in Rust, and harken on it";

  # FlakeHub rather than github: for every input, because a machine that can
  # reach FlakeHub and cache.nixos.org but not api.github.com (this one, for
  # instance) must still be able to build. The lock pins the tarballs.
  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.2411.*.tar.gz";
    rust-overlay.url = "https://flakehub.com/f/oxalica/rust-overlay/0.1.*.tar.gz";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    # The Rust builds' library: each target's dependencies built once, and
    # the workspace once over them, which every check then reuses. 0.20 is
    # the last series that supports nixpkgs 24.11 (0.21 dropped it); it
    # moves with nixpkgs.
    crane.url = "https://flakehub.com/f/ipetkov/crane/0.20.*.tar.gz";

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

  outputs = { self, nixpkgs, rust-overlay, crane, ... }@inputs:
    let
      lib = nixpkgs.lib;
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAll = f: lib.genAttrs systems (system:
        f (import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
          # The Android NDK and SDK are unfree, and their licence is
          # accepted here once.
          config = { allowUnfree = true; android_sdk.accept_license = true; };
        }));

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
      rustDirs = [ "rust" "harken/domain" "harken/player" "harken/server" "harken/iced" "harken/ffi" ];
      # The typed Kotlin and Swift `arkdb gen` writes, which harken/ffi's
      # tests hold to the domain (harken/ffi/tests/gen.rs).
      genDirs = [ "harken/android/app/src/main/kotlin/dev/harken/android/gen" "harken/ios/Harken/Gen" ];

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

          # Every Rust build is crane's, in layers, so that compiled code is
          # compiled once per target rather than once per derivation: a
          # target's dependencies (`buildDepsOnly`, over a dummy of the
          # workspace that is only its manifests, so an edit to the Rust
          # does not rebuild them), then the kernel over them (`rust/` real
          # and harken's crates still dummies, so an edit to harken does
          # not rebuild it), then harken's crates over that; on the host the
          # workspace's test targets over that again, which every check runs
          # from. A crane library per toolchain, since compiled code is per
          # target.
          craneFor = toolchain: (crane.mkLib pkgs).overrideToolchain toolchain;
          craneLib = craneFor rust;
          # What every layer shares: the source, the workspace at rust/, the
          # dependencies vendored once from the lockfile.
          common = {
            version = "0.1.0";
            src = only rustDirs;
            sourceRoot = "source/rust";
            inherit cargoVendorDir;
          };
          # crane vendors each crate as a symlink into its own store path,
          # and cargo then sees a crate at its real path while rustc's
          # `file!()` is the vendored one. askama's derive joins the two
          # (`<file's dir>/../../<real manifest dir>/askama.toml`), and in
          # uniffi_bindgen, which UniFFI's bindgen binary compiles, that
          # path does not exist. So the crates that do so are copied whole
          # into the vendor directory, and every other stays a link.
          cargoVendorDir =
            let
              linked = craneLib.vendorCargoDeps { cargoLock = ./rust/Cargo.lock; };
              copied = [ "uniffi_bindgen" ];
            in
            pkgs.runCommand "vendor-cargo-deps" { } ''
              for registry in ${linked}/*/; do
                dest=$out/$(basename "$registry")
                mkdir -p "$dest"
                for crate in "$registry"*; do
                  case $(basename "$crate") in
                    ${lib.concatMapStringsSep "|" (c: "${c}-[0-9]*") copied}) cp -rL "$crate" "$dest/" ;;
                    *) ln -s "$(readlink -f "$crate")" "$dest/$(basename "$crate")" ;;
                  esac
                done
              done
              sed "s|${linked}|$out|g" ${linked}/config.toml > $out/config.toml
            '';
          # The dummy source keeps a Cargo.lock only at its root, and the
          # workspace's is rust/'s; no build script or `include_*!` in the
          # workspace reads a file at compile time, so that is all it lacks.
          depsArgs = common // {
            extraDummyScript = "cp ${./rust/Cargo.lock} $out/rust/Cargo.lock";
          };
          # The kernel: the workspace's members under rust/ (harken's join
          # from ../harken), built from a source that is rust/ as it is and
          # harken's crates as the dependency layer's dummies, so that the
          # layer is keyed on rust/ and harken's manifests alone. Every
          # target's kernel layer runs the cargo commands of the layer over
          # it, which compile the same kernel units (the same crates, the same
          # features), so that over it cargo finds the kernel fresh — its
          # files are the same, at the store's one mtime — and compiles only
          # harken's crates, whose manifests are not the dummies' (crane
          # gives each dummy a build script).
          kernelCrates = lib.filter (m: !lib.hasPrefix "../" m)
            (builtins.fromTOML (builtins.readFile ./rust/Cargo.toml)).workspace.members;
          kernelFlags = lib.concatMapStringsSep " " (c: "-p ${c}") kernelCrates;
          kernelSrc = pkgs.runCommand "source" { } ''
            cp -r ${craneLib.mkDummySrc depsArgs} $out
            chmod -R u+w $out
            rm -rf $out/rust
            cp -r ${only [ "rust" ]}/rust $out/rust
          '';
          # On the host every cargo command resolves features once for the
          # whole workspace (cargo's `feature-unification = "workspace"`),
          # so that `-p ark` and `--workspace` compile the same units and a
          # check of a few crates reuses the layers rather than compiling
          # them again with other features. The setting is unstable in
          # cargo, and the channel override is read by cargo alone: rustc
          # and build scripts see stable, as before.
          hostEnv = {
            __CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS = "nightly";
            CARGO_UNSTABLE_FEATURE_UNIFICATION = "true";
            CARGO_RESOLVER_FEATURE_UNIFICATION = "workspace";
          };
          host = common // { env = hostEnv; };
          # The host's dependencies: built, checked (for clippy), and their
          # test-only ones.
          hostDeps = craneLib.buildDepsOnly (depsArgs // host // {
            pname = "arkdb";
            cargoExtraArgs = "--locked --workspace";
          });
          # The host's kernel over them: its libraries, binaries and test
          # targets, under the workspace's features.
          hostKernel = craneLib.mkCargoDerivation (host // {
            pname = "arkdb-kernel";
            src = kernelSrc;
            cargoArtifacts = hostDeps;
            buildPhaseCargoCommand = ''
              cargoWithProfile build --locked ${kernelFlags}
              cargoWithProfile test --locked ${kernelFlags} --no-run
            '';
          });
          # The workspace over the kernel, once: every library and binary
          # under `bin/` and `lib/` (harken's crates the only ones compiled),
          # and the target directory the packages and the tests are built
          # from.
          hostWorkspace = craneLib.buildPackage (host // {
            pname = "arkdb-workspace";
            cargoArtifacts = hostKernel;
            cargoExtraArgs = "--locked --workspace";
            doCheck = false;
            doInstallCargoArtifacts = true;
          });
          # Every test target of the workspace, compiled and not run, and
          # harken-iced's with its demo feature: what the checks run.
          hostTests = craneLib.mkCargoDerivation (host // {
            pname = "arkdb-tests";
            cargoArtifacts = hostWorkspace;
            buildPhaseCargoCommand = ''
              cargoWithProfile test --locked --workspace --no-run
              cargoWithProfile test --locked -p harken-iced --features demo --no-run
            '';
          });

          # One crate of the workspace (or a few: `flags`), its binaries and
          # libraries, from the workspace layer.
          crate = { pname, flags ? "-p ${pname}", ... }@args: craneLib.buildPackage (host // {
            cargoArtifacts = hostWorkspace;
            cargoExtraArgs = "--locked ${flags}";
            doCheck = false;
          } // removeAttrs args [ "flags" ]);
          # A run of some of the workspace's tests, from the test layer:
          # `cargo test --release <flags> <testArgs>`, the output empty.
          tests = { pname, flags, testArgs ? "", ... }@args: craneLib.cargoTest (host // {
            cargoArtifacts = hostTests;
            cargoExtraArgs = "--locked ${flags}";
            cargoTestExtraArgs = testArgs;
            doInstallCargoArtifacts = false;
            installPhase = "touch $out";
          } // removeAttrs args [ "flags" "testArgs" ]);

          # The browser peer: `harken/iced` compiled to wasm32 and bound by
          # wasm-bindgen, beside its page — one static directory. The same
          # crate as the desktop; `demo` is the seeded library with no
          # server that Pages publishes, and without it the page signs in
          # against the server it is served from. The toolchain is the
          # workspace's with the wasm target added, and the CLI is built from
          # the crate at the version the lockfile names.
          wasmRust = rust.override { targets = [ "wasm32-unknown-unknown" ]; };
          wasmCrane = craneFor wasmRust;
          wasm-bindgen-cli = pkgs.wasm-bindgen-cli.override ({
            inherit rustPlatform;
            version = wasmBindgenVersion;
          } // (wasmBindgenHashes.${wasmBindgenVersion}
            or (throw "flake.nix: no hashes for wasm-bindgen ${wasmBindgenVersion}; add them to wasmBindgenHashes")));
          # One wasm page: a binary of the workspace compiled to wasm32 and
          # bound by wasm-bindgen, beside the static files it is loaded from —
          # the browser peer, with and without its demo.
          # The pages' dependencies for wasm32, once for both.
          wasmBuild = flags: "cargoWithProfile build --locked --target wasm32-unknown-unknown ${flags}";
          wasmBuilds = lib.concatMapStringsSep "\n" wasmBuild [
            "-p harken-iced" "-p harken-iced --features demo"
          ];
          wasmDeps = wasmCrane.buildDepsOnly (depsArgs // {
            pname = "arkdb-wasm";
            doCheck = false;
            buildPhaseCargoCommand = wasmBuilds;
          });
          # The kernel for wasm32 over them, under the same two builds.
          wasmKernel = wasmCrane.mkCargoDerivation (common // {
            pname = "arkdb-wasm-kernel";
            src = kernelSrc;
            cargoArtifacts = wasmDeps;
            buildPhaseCargoCommand = wasmBuilds;
          });
          wasmPage = { pname, crate, bin ? crate, features ? "", files }: wasmCrane.mkCargoDerivation (common // {
            inherit pname;
            cargoArtifacts = wasmKernel;
            doInstallCargoArtifacts = false;
            nativeBuildInputs = [ wasm-bindgen-cli pkgs.binaryen ];
            buildPhaseCargoCommand = ''
              ${wasmBuild "-p ${crate} ${features}"}
              wasm-bindgen --target web --no-typescript --out-dir pkg \
                target/wasm32-unknown-unknown/release/${bin}.wasm
              wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
                --enable-sign-ext --enable-mutable-globals --enable-reference-types \
                -o pkg/${bin}_bg.wasm pkg/${bin}_bg.wasm
            '';
            # The page names the module and the wasm with `?v=dev`; this
            # output's own hash replaces it, so a browser holding an older
            # build is sent URLs its cache has never seen (harken/iced/web).
            installPhaseCommand = ''
              mkdir -p $out
              cp -r pkg $out/
              cp ${files} $out/
              substituteInPlace $out/index.html --replace-fail "v=dev" "v=$(basename $out | cut -c1-32)"
            '';
          });
          icedWeb = { pname, demo }: wasmPage {
            inherit pname;
            crate = "harken-iced";
            features = lib.optionalString demo "--features demo";
            files = "../harken/iced/web/index.html ../harken/iced/web/favicon.svg";
          };
          # What GitHub Pages publishes.
          harken-web = icedWeb { pname = "harken-web"; demo = true; };
          # What harken-server serves as its web directory.
          harken-web-server = icedWeb { pname = "harken-web-server"; demo = false; };

          # The server (harken/server: the server and harken-peer), on a
          # chain of its own as the APK's host part is below — dependencies,
          # kernel, the crate — and not over the workspace's layers: their
          # dependency layer is every member's, iced's stack (wgpu, winit, …)
          # for harken-iced among them, and the server would wait on it for
          # a GUI it never links. The same command at each layer,
          # `-p harken-server` alone, so that the kernel crates are compiled
          # under the server's features and found fresh above; the kernel
          # layer is keyed on `rust/**` and the manifests, the crate on
          # harken's sources too.
          serverBuild = "cargoWithProfile build --locked -p harken-server";
          serverDeps = craneLib.buildDepsOnly (depsArgs // {
            pname = "harken-server";
            doCheck = false;
            buildPhaseCargoCommand = serverBuild;
          });
          serverKernel = craneLib.mkCargoDerivation (common // {
            pname = "harken-server-kernel";
            src = kernelSrc;
            cargoArtifacts = serverDeps;
            buildPhaseCargoCommand = serverBuild;
          });
          harken-server = craneLib.buildPackage (common // {
            pname = "harken-server";
            cargoArtifacts = serverKernel;
            cargoExtraArgs = "--locked -p harken-server";
            doCheck = false;
          });
          # `arkdb` for a server to read its system's events with
          # (docs/plan-observe.md), on the server's chain: no GUI stack.
          arkdb = craneLib.buildPackage (common // {
            pname = "arkdb";
            cargoArtifacts = serverKernel;
            cargoExtraArgs = "--locked -p arkdb";
            doCheck = false;
          });

          # The desktop window. winit and wgpu open the display and the GPU
          # by dlopen at run time, so the libraries go on the library path.
          harken-iced = crate {
            pname = "harken-iced";
            nativeBuildInputs = [ pkgs.makeWrapper ];
            postFixup = lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              wrapProgram $out/bin/harken-iced --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath (with pkgs; [
                wayland libxkbcommon vulkan-loader libGL xorg.libX11 xorg.libXcursor xorg.libXi xorg.libXrandr
              ])}
            '';
          };

          # The specification is `rust/ark` (spec/README.md), and the tools
          # over it are `rust/arkdb`: one build of both, whose binary is
          # `arkdb` (`gen`, `fuzz`, and reading a system's events: `tail`,
          # `trace`, `peer`); `fuzz` is its fuzzer under the name `nix run`
          # takes.
          ark = crate { pname = "ark"; flags = "-p ark -p arkdb"; };
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
          vectors = tests {
            pname = "ark-vectors";
            # The writers only: the rest of both crates' tests are checks.rust's.
            flags = "-p ark -p arkdb --test hash --test vectors --test views";
            # arkdb's tests also hold spec/README.md and AUTHORING.md.
            src = only (rustDirs ++ [ "spec/README.md" "spec/AUTHORING.md" ]);
            # stdenv makes only the source root (rust/) writable, and the
            # writer writes beside it.
            postUnpack = "chmod u+w source";
            preCheck = ''
              chmod -R u+w ../spec
              rm -rf ../spec/vectors
              export ARK_WRITE_VECTORS=1
            '';
            installPhase = "cp -r ../spec/vectors $out";
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
          # for the host (`uniffi-bindgen generate --library`), and beside
          # them goes the bridge's typed half (rust/ark-ffi/{kotlin,swift}).
          ffiBindings = { lib, language }: ''
            ${uniffiBindgen}/bin/uniffi-bindgen \
              generate --library ${lib} --language ${language} --no-format --out-dir bindings/${language}
          '';
          # The Android NDK nixpkgs packages from Google's zip (x86_64 Linux
          # and macOS hosts), whose clang links each Android target and
          # compiles ring's C.
          ndk = (pkgs.androidenv.composeAndroidPackages {
            includeNDK = true;
            ndkVersions = [ "27.0.12077973" ];
            platformVersions = [ ];
            buildToolsVersions = [ ];
            includeEmulator = false;
            includeSources = false;
          }).ndk-bundle;
          # The ABIs the APK carries: a phone, an old phone, an emulator.
          androidTargets = {
            "aarch64-linux-android" = { abi = "arm64-v8a"; clang = "aarch64-linux-android"; };
            "armv7-linux-androideabi" = { abi = "armeabi-v7a"; clang = "armv7a-linux-androideabi"; };
            "x86_64-linux-android" = { abi = "x86_64"; clang = "x86_64-linux-android"; };
          };
          androidRust = rust.override { targets = lib.attrNames androidTargets; };
          androidCrane = craneFor androidRust;
          # The NDK's clang and ar for one target, as cargo and cc-rs read them.
          androidEnv = t: a:
            let up = lib.toUpper (lib.replaceStrings [ "-" ] [ "_" ] t); low = lib.replaceStrings [ "-" ] [ "_" ] t; in ''
              ndkRoot=$(echo ${ndk}/libexec/android-sdk/ndk/*)
              bin=$ndkRoot/toolchains/llvm/prebuilt/${if pkgs.stdenv.hostPlatform.isDarwin then "darwin" else "linux"}-x86_64/bin
              export AR=$bin/llvm-ar
              export CARGO_TARGET_${up}_LINKER=$bin/${a.clang}26-clang
              export CC_${low}=$bin/${a.clang}26-clang AR_${low}=$bin/llvm-ar
            '';
          # `harken_ffi` for one Android ABI against API 26 (the app's
          # minSdk), stripped, as `jniLibs/<abi>/libharken_ffi.so`: its
          # dependencies for that target, the kernel over them, and the
          # library over that.
          androidLib = t: a:
            let
              build = "cargoWithProfile build --locked -p harken-ffi --lib --target ${t}";
              deps = androidCrane.buildDepsOnly (depsArgs // {
                pname = "harken-android-${a.abi}";
                doCheck = false;
                preBuild = androidEnv t a;
                buildPhaseCargoCommand = build;
              });
              kernel = androidCrane.mkCargoDerivation (common // {
                pname = "harken-android-${a.abi}-kernel";
                src = kernelSrc;
                cargoArtifacts = deps;
                preBuild = androidEnv t a;
                buildPhaseCargoCommand = build;
              });
            in
            androidCrane.mkCargoDerivation (common // {
              pname = "harken-android-${a.abi}";
              cargoArtifacts = kernel;
              doInstallCargoArtifacts = false;
              preBuild = androidEnv t a;
              buildPhaseCargoCommand = build;
              installPhaseCommand = ''
                mkdir -p $out/jniLibs/${a.abi}
                $bin/llvm-strip -o $out/jniLibs/${a.abi}/libharken_ffi.so target/${t}/release/libharken_ffi.so
              '';
            });
          # The Kotlin the app compiles with the library, UniFFI's bindings
          # and the typed half, under `kotlin/`; and the library built for
          # this machine as `host/libharken_ffi.so`, which the app's JVM
          # tests load. Built as an Android ABI's is, on a chain of its own
          # (dependencies, kernel, library) and not over the workspace's
          # layers, whose dependency layer is every member's, iced's stack
          # (wgpu, winit, …) among them: `-p harken-ffi` would wait on it.
          # The dependencies have UniFFI's bindgen binary's too.
          ffiHostBuild = "cargoWithProfile build --locked -p harken-ffi --lib";
          ffiHostDeps = craneLib.buildDepsOnly (depsArgs // {
            pname = "harken-android-host";
            doCheck = false;
            buildPhaseCargoCommand = ''
              ${ffiHostBuild}
              cargoWithProfile build --locked -p ark-ffi --features bindgen --bin uniffi-bindgen
            '';
          });
          ffiHostKernel = craneLib.mkCargoDerivation (common // {
            pname = "harken-android-host-kernel";
            src = kernelSrc;
            cargoArtifacts = ffiHostDeps;
            buildPhaseCargoCommand = ffiHostBuild;
          });
          # UniFFI's bindgen binary (rust/ark-ffi/src/bin/uniffi-bindgen.rs),
          # which is UniFFI's main and nothing of the workspace: built over
          # the dummy source with that one file real, so the workspace is
          # not compiled again under the features its CLI adds.
          uniffiBindgen = craneLib.buildPackage (common // {
            pname = "uniffi-bindgen";
            src = craneLib.mkDummySrc (depsArgs // {
              extraDummyScript = depsArgs.extraDummyScript + ''

                install -m644 ${./rust/ark-ffi/src/bin/uniffi-bindgen.rs} $out/rust/ark-ffi/src/bin/uniffi-bindgen.rs
              '';
            });
            cargoArtifacts = ffiHostDeps;
            cargoExtraArgs = "--locked -p ark-ffi --features bindgen --bin uniffi-bindgen";
            # The dummy layer built a stub at the same path, and every file
            # in the store is as old as every other, so cargo would take the
            # stub for this file's build.
            preBuild = "touch ark-ffi/src/bin/uniffi-bindgen.rs";
            doCheck = false;
          });
          androidHost = craneLib.mkCargoDerivation (common // {
            pname = "harken-android-host";
            cargoArtifacts = ffiHostKernel;
            doInstallCargoArtifacts = false;
            buildPhaseCargoCommand = ''
              ${ffiHostBuild}
              ${ffiBindings { lib = "target/release/libharken_ffi.so"; language = "kotlin"; }}
            '';
            installPhaseCommand = ''
              mkdir -p $out/kotlin/dev/arkdb/ffi
              cp -r bindings/kotlin/. $out/kotlin/
              install -Dm755 target/release/libharken_ffi.so $out/host/libharken_ffi.so
              cp ark-ffi/kotlin/Typed.kt $out/kotlin/dev/arkdb/ffi/
            '';
          });
          # Each ABI's library and the host's part, one directory: what
          # `harken-apk` builds the app over.
          harken-android-lib = pkgs.runCommand "harken-android-lib-0.1.0" { } ''
            mkdir -p $out
            for part in ${lib.concatStringsSep " " (lib.mapAttrsToList androidLib androidTargets)} ${androidHost}; do
              cp -r --no-preserve=mode $part/. $out/
            done
            chmod 755 $out/host/libharken_ffi.so
          '';

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
          # properties; `title` and `note` head that page.
          androidApp = { name, appid, flags ? [ ], title, note }:
            let
              dir = "${name}/android";
              apk = pkgs.stdenv.mkDerivation (final: {
                pname = "${name}-apk";
                version = "0.1.0";
                src = only [ dir ];
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
            title = "Harken";
            note = "Rendered on the JVM by Paparazzi from a fixed library, at a Pixel 5's size; light beside dark. Covers are the derived squares: nothing here talks to a server.";
          };
          harken-apk = harken.apk;
          harken-android-screens = harken.screens;

          # The Bible as one scroll, zoomed like a map (revelation/README.md,
          # docs/plan-bible.md): plain Kotlin, no native library and none of
          # ArkDB, so it is the SDK and gradle alone.
          revelation = androidApp {
            name = "revelation";
            appid = "dev.revelation.android";
            title = "Revelation";
            note = "Rendered on the JVM by Paparazzi through layoutlib; light beside dark.";
          };

          # Both apps as one F-Droid repository (nix/fdroid.nix): each app is
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
            ];
          };

          # The same library for iOS — `aarch64-apple-ios` for a phone and
          # `aarch64-apple-ios-sim` for the simulator on an Apple-silicon Mac —
          # as `HarkenFFI.xcframework`, with UniFFI's Swift and the typed half
          # under `swift/`. Linking for iOS needs Apple's SDK, which only a
          # macOS builder has: on Linux this derivation evaluates and is not
          # built (meta.platforms), and it has not been built anywhere yet.
          iosRust = rust.override { targets = [ "aarch64-apple-ios" "aarch64-apple-ios-sim" ]; };
          iosCrane = craneFor iosRust;
          iosTargets = [ "aarch64-apple-ios" "aarch64-apple-ios-sim" ];
          # The library for each iOS target and for the host, which UniFFI
          # reads the Swift from.
          # The iOS targets compile and link with the builder's Xcode: its
          # developer directory, clang, ar and SDKs. nixpkgs' darwin stdenv
          # sets DEVELOPER_DIR and SDKROOT to its own macOS SDK, which has no
          # iPhoneOS platform, so `xcrun --sdk iphoneos` (cc-rs asks it)
          # fails under them: both are taken back for these two targets, and
          # the host build keeps nix's toolchain.
          iosBuild = ''
            export PATH=/usr/bin:$PATH
            export DEVELOPER_DIR="$(/usr/bin/xcode-select -p)"
            unset SDKROOT
            ${lib.concatMapStrings (t: let u = lib.toUpper (lib.replaceStrings [ "-" ] [ "_" ] t); l = lib.replaceStrings [ "-" ] [ "_" ] t; in ''
              export CC_${l}=/usr/bin/clang CXX_${l}=/usr/bin/clang++ AR_${l}=/usr/bin/ar
              export CARGO_TARGET_${u}_LINKER=/usr/bin/clang
            '') iosTargets}
            ${lib.concatMapStrings (t: "cargoWithProfile build --locked -p harken-ffi --lib --target ${t}\n") iosTargets}
            cargoWithProfile build --locked -p harken-ffi --lib
          '';
          # Their dependencies.
          iosDeps = iosCrane.buildDepsOnly (depsArgs // {
            pname = "harken-ios";
            doCheck = false;
            # xcodebuild and the iPhoneOS and simulator SDKs: the builder's
            # Xcode, outside the store.
            __noChroot = true;
            buildPhaseCargoCommand = iosBuild;
            meta.platforms = [ "aarch64-darwin" "x86_64-darwin" ];
          });
          # The kernel over them.
          iosKernel = iosCrane.mkCargoDerivation (common // {
            pname = "harken-ios-kernel";
            src = kernelSrc;
            cargoArtifacts = iosDeps;
            __noChroot = true;
            buildPhaseCargoCommand = iosBuild;
            meta.platforms = [ "aarch64-darwin" "x86_64-darwin" ];
          });
          harken-ios-xcframework = iosCrane.mkCargoDerivation (common // {
            pname = "harken-ios-xcframework";
            cargoArtifacts = iosKernel;
            doInstallCargoArtifacts = false;
            __noChroot = true;
            buildPhaseCargoCommand = iosBuild + ''
              ${ffiBindings { lib = "target/release/libharken_ffi.dylib"; language = "swift"; }}
              mkdir -p headers
              cp bindings/swift/*.h headers/
              cat bindings/swift/*.modulemap > headers/module.modulemap
              /usr/bin/xcodebuild -create-xcframework \
                -library target/aarch64-apple-ios/release/libharken_ffi.a -headers headers \
                -library target/aarch64-apple-ios-sim/release/libharken_ffi.a -headers headers \
                -output HarkenFFI.xcframework
            '';
            installPhaseCommand = ''
              mkdir -p $out/swift
              cp -r HarkenFFI.xcframework $out/
              cp bindings/swift/*.swift ark-ffi/swift/Typed.swift $out/swift/
            '';
            meta.platforms = [ "aarch64-darwin" "x86_64-darwin" ];
          });

          # harken's domain (harken/domain: plain Rust, §1.16) built, and
          # its schema's hash held to the one its tests pin.
          harken-domain = tests {
            pname = "harken-domain";
            flags = "-p harken-domain --test port";
            testArgs = "-- the_schema_hash_is_pinned";
          };

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
        in
        {
          packages = {
            inherit ark vectors fuzz harken-domain trace-corpus harken-android-lib harken-ios-xcframework;
            inherit harken-server arkdb;
            # `nix run .#harken-serve [ADDR]`: the dev server, anyone is
            # whoever they say.
            harken-serve = pkgs.callPackage ./harken/server/nix/serve.nix { inherit harken-server; };
            inherit harken-web harken-web-server harken-iced;
            default = ark;
            inherit harken-apk harken-android-screens;
            harken-apk-deps = harken.deps;
            revelation-apk = revelation.apk;
            revelation-android-screens = revelation.screens;
            revelation-apk-deps = revelation.deps;
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
            # The test target's two binaries, built beside it, are in the
            # workspace layer.
            versions = tests {
              pname = "harken-versions";
              flags = "-p harken-server --test versions";
              testArgs = "-- --nocapture";
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
            traces = tests {
              pname = "ark-traces";
              flags = "-p arkdb --test vectors";
              testArgs = "trace_corpus -- --nocapture";
              src = only (rustDirs ++ [ "spec/vectors" ]);
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
            # and AUTHORING.md's examples too); formatted and lint-clean are
            # `fmt` and `clippy`.
            rust = tests {
              pname = "arkdb-rust";
              flags = "--workspace";
              src = only (rustDirs ++ genDirs ++ [ "spec/vectors" "spec/README.md" "spec/AUTHORING.md" ]);
              # The client's demo is a feature, so its tests are a second run.
              postCheck = ''
                cargoWithProfile test --locked -p harken-iced --features demo
              '';
            };
            # The workspace lint-clean, the demo feature too, over the
            # host's dependencies.
            clippy = craneLib.cargoClippy (host // {
              pname = "arkdb";
              cargoArtifacts = hostDeps;
              cargoExtraArgs = "--locked --workspace";
              cargoClippyExtraArgs = "--all-targets -- -D warnings";
              postBuild = "cargoWithProfile clippy --locked -p harken-iced --features demo --all-targets -- -D warnings";
              doInstallCargoArtifacts = false;
              installPhase = "touch $out";
            });
            # The workspace formatted as rust/rustfmt.toml says.
            fmt = craneLib.cargoFmt (common // {
              pname = "arkdb";
              cargoExtraArgs = "--all";
              installPhase = "touch $out";
            });
            # harken's domain crate builds, and its schema's hash is the one
            # pinned (harken/domain/tests/port.rs).
            inherit harken-domain;
            # The bridge (§1.16, N3): ark-ffi and harken/ffi build natively,
            # their tests drive the whole surface in the process (a server in
            # it, a mutate and its refusal, a view's patches, the live space,
            # alone and joining, a hall of devices) and hold the generated
            # Kotlin and Swift to the domain; and UniFFI writes the Kotlin and
            # the Swift for the library.
            ffi = tests {
              pname = "arkdb-ffi";
              flags = "-p ark-ffi -p harken-ffi";
              src = only (rustDirs ++ genDirs);
              postCheck = ''
                lib=target/release/libharken_ffi.so
                for language in kotlin swift; do
                  ${uniffiBindgen}/bin/uniffi-bindgen \
                    generate --library "$lib" --language $language --no-format --out-dir "$TMPDIR/bindings/$language"
                done
                grep -q 'fun `harkenOpen`' "$TMPDIR/bindings/kotlin/dev/harken/ffi/harken_ffi.kt"
                grep -q "class Peer" "$TMPDIR/bindings/kotlin/dev/arkdb/ffi/ark_ffi.kt"
                grep -q "func harkenOpen" "$TMPDIR/bindings/swift/harken_ffi.swift"
                ls "$TMPDIR/bindings/swift/"*FFI.h "$TMPDIR/bindings/swift/"*.modulemap
              '';
              # The bindings it wrote, which the iOS type-check reads.
              installPhase = "cp -r $TMPDIR/bindings $out";
            };
          } // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            # What of the iOS app Linux can check (harken/ios/tools/typecheck.sh):
            # Rows.swift, Bridge.swift and Model.swift with the generated types,
            # UniFFI's Swift and the bridge's typed half, type-checked by
            # nixpkgs' Swift against the library's headers and a stand-in for
            # SwiftUI. The views are SwiftUI's and are not checked here.
            harken-ios-typecheck = pkgs.swiftPackages.stdenv.mkDerivation {
              name = "harken-ios-typecheck";
              src = only [ "harken/ios" "rust/ark-ffi/swift" ];
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
            # The NixOS module, evaluated under the configurations that
            # matter and held to its assertions, its warning and its unit.
            harken-module = pkgs.callPackage ./harken/server/nix/module-test.nix {
              module = import ./harken/server/nix/module.nix { packages = self.packages; };
            };
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
      checks = lib.mapAttrs (_: o: o.checks) outputs;
      devShells = lib.mapAttrs (_: o: o.devShells) outputs;
      # `services.harken`: the sync server, sign-in, /media and the
      # browser client on one port.
      nixosModules.default = import ./harken/server/nix/module.nix { packages = self.packages; };
    };
}
