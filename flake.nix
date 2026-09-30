{
  description = "ArkDB: the specification and runtime in Rust, the frozen Swift and Kotlin, and harken on them";

  # FlakeHub rather than github: for every input, because a machine that can
  # reach FlakeHub and cache.nixos.org but not api.github.com (this one, for
  # instance) must still be able to build. The lock pins the tarballs.
  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.2411.*.tar.gz";
    rust-overlay.url = "https://flakehub.com/f/oxalica/rust-overlay/0.1.*.tar.gz";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      lib = nixpkgs.lib;
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAll = f: lib.genAttrs systems (system:
        f (import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
          # The Android SDK is unfree, and its licence is accepted here once.
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

      # The Rust workspace is `rust/`, and harken's four crates join it from
      # `harken/` by path, so both trees are the source of every Rust build.
      rustDirs = [ "rust" "harken/domain" "harken/server" "harken/iced" ];

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
          rustPlatform = pkgs.makeRustPlatform { cargo = rust; rustc = rust; };

          # One crate of the workspace, built from the whole of it.
          crate = { pname, flags ? [ "-p" pname ], ... }@args: rustPlatform.buildRustPackage ({
            inherit pname;
            version = "0.1.0";
            src = only rustDirs;
            sourceRoot = "source/rust";
            cargoLock.lockFile = ./rust/Cargo.lock;
            cargoBuildFlags = flags;
            cargoTestFlags = flags;
            doCheck = false;
          } // removeAttrs args [ "pname" "flags" ]);

          # The browser peer: `harken/iced` compiled to wasm32 and bound by
          # wasm-bindgen, beside its page — one static directory. The same
          # crate as the desktop; `demo` is the seeded library with no
          # server that Pages publishes, and without it the page signs in
          # against the server it is served from. The toolchain is the
          # workspace's with the wasm target added, and the CLI is built from
          # the crate at the version the lockfile names.
          wasmRust = rust.override { targets = [ "wasm32-unknown-unknown" ]; };
          wasmPlatform = pkgs.makeRustPlatform { cargo = wasmRust; rustc = wasmRust; };
          wasm-bindgen-cli = pkgs.wasm-bindgen-cli.override ({
            inherit rustPlatform;
            version = wasmBindgenVersion;
          } // (wasmBindgenHashes.${wasmBindgenVersion}
            or (throw "flake.nix: no hashes for wasm-bindgen ${wasmBindgenVersion}; add them to wasmBindgenHashes")));
          icedWeb = { pname, demo }: wasmPlatform.buildRustPackage {
            inherit pname;
            version = "0.1.0";
            src = only rustDirs;
            sourceRoot = "source/rust";
            cargoLock.lockFile = ./rust/Cargo.lock;
            nativeBuildInputs = [ wasm-bindgen-cli pkgs.binaryen ];
            # Not cargoBuildHook: it targets the host, and this is the wasm.
            buildPhase = ''
              runHook preBuild
              cargo build --release --offline --frozen -p harken-iced \
                ${lib.optionalString demo "--features demo"} --target wasm32-unknown-unknown
              wasm-bindgen --target web --no-typescript --out-dir pkg \
                target/wasm32-unknown-unknown/release/harken-iced.wasm
              wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
                --enable-sign-ext --enable-mutable-globals --enable-reference-types \
                -o pkg/harken-iced_bg.wasm pkg/harken-iced_bg.wasm
              runHook postBuild
            '';
            # The page names the module and the wasm with `?v=dev`; this
            # output's own hash replaces it, so a browser holding an older
            # build is sent URLs its cache has never seen (harken/iced/web).
            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp -r pkg $out/
              cp ../harken/iced/web/index.html ../harken/iced/web/favicon.svg $out/
              substituteInPlace $out/index.html --replace-fail "v=dev" "v=$(basename $out | cut -c1-32)"
              runHook postInstall
            '';
            doCheck = false;
          };
          # What GitHub Pages publishes.
          harken-web = icedWeb { pname = "harken-web"; demo = true; };
          # What harken-server serves as its web directory.
          harken-web-server = icedWeb { pname = "harken-web-server"; demo = false; };

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

          # The specification is `rust/ark` (spec/README.md), and its three
          # binaries are one crate build: `ark-vectors` writes the vectors,
          # `arkc` verifies, hashes and compares modules, `gen-unicode`
          # writes the Unicode tables. `arkc` and `vectors` are that build
          # under the names `nix run` takes.
          ark = crate { pname = "ark"; };
          arkc = pkgs.writeShellApplication {
            name = "arkc";
            runtimeInputs = [ ark ];
            text = ''exec arkc "$@"'';
          };
          vectors = pkgs.writeShellApplication {
            name = "ark-vectors";
            runtimeInputs = [ ark ];
            text = ''exec ark-vectors "$@"'';
          };

          # The binary that writes harken.ark from the domain crate, and the
          # module it writes, verified by the reference.
          harken-domain-bin = crate { pname = "harken-domain"; };
          harken-domain = pkgs.runCommand "harken-domain" { nativeBuildInputs = [ harken-domain-bin ark ]; } ''
            mkdir -p $out
            harken-domain $out/harken.ark
            arkc verify $out/harken.ark
          '';

          # Swift and Kotlin are frozen at spec v3 (swift/FROZEN.md,
          # kotlin/FROZEN.md) and have no check or package here. The one
          # thing built from them is harken-apk below: the frozen Android app
          # over the frozen Kotlin runtime and the frozen print of harken's
          # v3 domain, which reads no vectors.

          # The Android SDK the APK is built with. Google ships one for
          # x86_64 Linux and for macOS; on Linux the SDK is assembled by hand
          # instead (nix/android-sdk.nix) — the platform and build-tools jars
          # from Google's zips, and aapt2, the one native tool AGP runs, from
          # Debian's package for the host (nix/aapt2.nix) — so an ARM Linux
          # builds the phone natively. macOS takes nixpkgs' androidenv.
          aapt2 = pkgs.callPackage ./nix/aapt2.nix { stdenv = pkgs.clangStdenv; };
          androidSdk = pkgs.callPackage ./nix/android-sdk.nix { inherit aapt2; };
          sdkRoot =
            if pkgs.stdenv.hostPlatform.isLinux then "${androidSdk}/${androidSdk.root}"
            else "${(pkgs.androidenv.composeAndroidPackages {
              platformVersions = [ "35" ];
              buildToolsVersions = [ "35.0.0" ];
            }).androidsdk}/libexec/android-sdk";

          # The phone, frozen at spec v3: gradle over the SDK, the Kotlin
          # runtime and client as a composite build, and the printed domain
          # — assembled offline from a recorded Maven graph
          # (harken/android/deps.json; `nix run .#harken-apk-deps`
          # re-records it). Debug-signed, like any assembleDebug; it
          # installs anywhere and belongs nowhere public.
          harken-apk = pkgs.stdenv.mkDerivation (final: {
            pname = "harken-apk";
            version = "0.1.0";
            src = only [ "harken/android" "harken/domain/gen/kotlin" "kotlin" ];
            sourceRoot = "source/harken/android";
            # stdenv makes only the source root writable, and gradle writes
            # `.gradle/` and `build/` inside the included build at ../../kotlin
            # too — a read-only one ends the build with no task run and no
            # message. The whole unpacked tree is writable instead.
            postUnpack = "chmod -R u+w source";
            nativeBuildInputs = [ pkgs.gradle pkgs.jdk21 ];
            mitmCache = pkgs.gradle.fetchDeps {
              pkg = final.finalPackage;
              data = ./harken/android/deps.json;
            };
            __darwinAllowLocalNetworking = true;
            ANDROID_HOME = sdkRoot;
            ANDROID_SDK_ROOT = sdkRoot;
            gradleFlags = [
              "-Dorg.gradle.java.home=${pkgs.jdk21.home}"
              # The Kotlin build asks for a JDK 21 toolchain; this is the one,
              # and gradle is not to go looking for or downloading another.
              "-Porg.gradle.java.installations.auto-detect=false"
              "-Porg.gradle.java.installations.auto-download=false"
              "-Porg.gradle.java.installations.paths=${pkgs.jdk21.home}"
              # AGP would fetch its own aapt2 from Maven, an unpatched binary
              # that cannot run here; the SDK's is patched and can.
              "-Pandroid.aapt2FromMavenOverride=${sdkRoot}/build-tools/35.0.0/aapt2"
            ];
            # AGP keeps its own state (analytics settings, the debug keystore it
            # signs with) under ~/.android, and the builder has no home.
            preBuild = ''
              export HOME="$TMPDIR/home" ANDROID_USER_HOME="$TMPDIR/home/.android"
              mkdir -p "$ANDROID_USER_HOME"
            '';
            gradleBuildTask = ":app:assembleDebug";
            # The recording runs the same assemble rather than resolving every
            # configuration: AGP's androidTest classpaths cannot be resolved
            # in isolation, and what the build fetches is the whole graph.
            gradleUpdateTask = ":app:assembleDebug";
            doCheck = false;
            installPhase = ''
              mkdir -p $out
              cp app/build/outputs/apk/debug/app-debug.apk $out/harken-debug.apk
            '';
          });
        in
        {
          packages = {
            inherit ark arkc vectors harken-domain;
            harken-server = crate { pname = "harken-server"; };
            # `nix run .#harken-serve [ADDR]`: the dev server, anyone is
            # whoever they say.
            harken-serve = pkgs.callPackage ./harken/server/nix/serve.nix { harken-server = crate { pname = "harken-server"; }; };
            inherit harken-web harken-web-server harken-iced;
            default = ark;
            inherit harken-apk;
            harken-apk-deps = harken-apk.mitmCache.updateScript;
          } // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            # The two pieces an ARM Linux needed, on their own.
            inherit aapt2;
            android-sdk = androidSdk;
            # The fleet on three NixOS machines (docs/plan-fleet.md §3): an
            # interface down, the service restarted, the server crashed, the
            # scanner over a bound directory. A package and not a check: it
            # wants /dev/kvm, and under TCG it takes most of an hour.
            fleet-vm = import ./harken/server/nix/fleet-vm.nix {
              inherit pkgs;
              module = import ./harken/server/nix/module.nix { packages = self.packages; };
              # Both binaries: the server, and harken-peer beside it.
              harken-server = crate { pname = "harken-server"; };
            };
          };

          checks = {
            # The vectors in the tree are the ones the specification writes.
            vectors = pkgs.runCommand "check-vectors" { nativeBuildInputs = [ ark ]; } ''
              ark-vectors vectors
              diff -r vectors ${./spec/vectors} && touch $out
            '';
            # The Rust workspace: formatted, lint-clean, and its tests.
            rust = crate {
              pname = "arkdb-rust";
              flags = [ "--workspace" ];
              src = only (rustDirs ++ [ "spec/vectors" ]);
              doCheck = true;
              preCheck = ''
                cargo fmt --all --check
                cargo clippy --workspace --all-targets --offline -- -D warnings
                cargo clippy -p harken-iced --features demo --all-targets --offline -- -D warnings
              '';
              # The client's demo is a feature, so its tests are a second run.
              postCheck = ''
                cargo test -p harken-iced --features demo --offline
              '';
              installPhase = "touch $out";
            };
            # harken.ark is what the domain crate emits, and the reference
            # verifies it.
            harken-domain = pkgs.runCommand "check-harken-domain" { nativeBuildInputs = [ ark ]; } ''
              diff ${harken-domain}/harken.ark ${./harken/domain/harken.ark}
              arkc verify ${./harken/domain/harken.ark}
              touch $out
            '';
          } // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
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
