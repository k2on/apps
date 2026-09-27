{
  description = "ArkDB: the specification, the toolchain, the runtimes, and harken on them";

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
        f (import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; }));

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

      # The Rust workspace is `rust/`, and harken's three crates join it from
      # `harken/` by path, so both trees are the source of every Rust build.
      rustDirs = [ "rust" "harken/domain" "harken/server" "harken/desktop" ];

      # What a client of harken is generated with: the functions it calls,
      # and nothing the scanner alone authors (harken/README.md).
      clientFunctions = "create_playlist,add_to_playlist,remove_from_playlist,library,playlists,playlist_items";

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

          # The specification and arkc, one Haskell package: `ark-spec` holds
          # the library and both executables.
          ark-spec = pkgs.haskellPackages.callCabal2nix "ark-spec" ./spec { };
          arkc = pkgs.writeShellApplication {
            name = "arkc";
            runtimeInputs = [ ark-spec ];
            text = ''exec arkc "$@"'';
          };
          vectors = pkgs.writeShellApplication {
            name = "ark-vectors";
            runtimeInputs = [ ark-spec ];
            text = ''exec ark-vectors "$@"'';
          };

          # The binary that writes harken.ark, and the module and generated
          # code it and arkc produce: what `harken/domain` must hold.
          harken-domain-bin = crate { pname = "harken-domain"; };
          harken-domain = pkgs.runCommand "harken-domain" { nativeBuildInputs = [ harken-domain-bin ark-spec ]; } ''
            mkdir -p $out/gen/rust $out/gen/swift $out/gen/kotlin
            harken-domain $out/harken.ark
            arkc verify $out/harken.ark
            for t in rust swift kotlin; do
              arkc gen $t $out/harken.ark $out/gen/$t --name Harken --only ${clientFunctions}
            done
          '';

          # The Swift runtime and client, with the conformance runner over the
          # vectors it is held to. The two flags the toolchain needs on this
          # platform are the ones swift/tools/nix-swiftc passes.
          swift = pkgs.swiftPackages.stdenv.mkDerivation {
            pname = "arkdb-swift";
            version = "0.1.0";
            src = only [ "swift" "spec/vectors" ];
            sourceRoot = "source/swift";
            nativeBuildInputs = [ pkgs.swift pkgs.swiftpm ];
            # Foundation as a build input, so its rpath reaches every binary
            # SwiftPM links — the manifest it compiles and runs included.
            buildInputs = [ pkgs.swiftPackages.Foundation pkgs.swiftPackages.Dispatch ];
            swiftpmBuildConfig = "debug";
            # What swift/tools/swift.sh sets in the devshell: the manifest
            # SwiftPM compiles and runs has no rpath for libdispatch.
            preBuild = ''
              export SWIFT_EXEC="$PWD/tools/nix-swiftc"
              export ARKDB_SWIFTC_INCLUDE=
              chmod +x tools/nix-swiftc
              export LD_LIBRARY_PATH="${lib.makeLibraryPath [ pkgs.swiftPackages.Dispatch pkgs.swiftPackages.Foundation ]}:${pkgs.swiftPackages.Foundation}/lib/swift/linux:${pkgs.swiftPackages.swift-unwrapped}/lib/swift/linux''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
            '';
            doCheck = true;
            checkPhase = ''
              runHook preCheck
              "$(swiftpmBinPath)/ArkDBTests"
              runHook postCheck
            '';
            installPhase = ''
              mkdir -p $out/lib
              cp -r "$(swiftpmBinPath)"/*.swiftmodule "$(swiftpmBinPath)"/*.swiftdoc $out/lib/ 2>/dev/null || true
              cp "$(swiftpmBinPath)"/ArkDBTests $out/lib/
            '';
          };

          # The Kotlin runtime and client: gradle over a recorded Maven graph
          # (kotlin/deps.json), replayed offline the way nixpkgs builds every
          # gradle project. `nix run .#kotlin-deps` re-records it.
          kotlin = pkgs.stdenv.mkDerivation (final: {
            pname = "arkdb-kotlin";
            version = "0.1.0";
            src = only [ "kotlin" "spec/vectors" ];
            sourceRoot = "source/kotlin";
            nativeBuildInputs = [ pkgs.gradle pkgs.jdk21 ];
            mitmCache = pkgs.gradle.fetchDeps {
              pkg = final.finalPackage;
              data = ./kotlin/deps.json;
            };
            __darwinAllowLocalNetworking = true;
            gradleFlags = [ "-Dorg.gradle.java.home=${pkgs.jdk21}" ];
            gradleBuildTask = "build";
            doCheck = false;
            installPhase = ''
              mkdir -p $out/lib
              cp ark-runtime/build/libs/*.jar ark-client/build/libs/*.jar $out/lib/
            '';
          });
        in
        {
          packages = {
            inherit ark-spec arkc vectors harken-domain;
            arkdb-kotlin = kotlin;
            kotlin-deps = kotlin.mitmCache.updateScript;
            harken-server = crate { pname = "harken-server"; };
            harken-desktop = crate { pname = "harken-desktop"; };
            arkdb-swift = swift;
            default = ark-spec;
          };

          checks = {
            # The vectors in the tree are the ones the specification writes.
            vectors = pkgs.runCommand "check-vectors" { nativeBuildInputs = [ ark-spec ]; } ''
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
              '';
              installPhase = "touch $out";
            };
            # What arkc wrote from harken.ark compiles against ark::gen and
            # agrees with the interpreter on one mutator (harken/domain/gen-check).
            harken-gen-check = rustPlatform.buildRustPackage {
              pname = "harken-gen-check";
              version = "0.1.0";
              src = only rustDirs;
              sourceRoot = "source/harken/domain/gen-check";
              cargoLock.lockFile = ./harken/domain/gen-check/Cargo.lock;
              doCheck = true;
              installPhase = "touch $out";
            };
            # harken.ark and the three generated files are what the domain
            # program and arkc write today.
            harken-domain = pkgs.runCommand "check-harken-domain" { } ''
              diff ${harken-domain}/harken.ark ${./harken/domain/harken.ark}
              diff -r ${harken-domain}/gen ${./harken/domain/gen}
              touch $out
            '';
            swift = swift;
            kotlin = kotlin;
          };

          devShells = {
            # The spec: GHC with the boot libraries the spec confines itself to.
            spec = pkgs.mkShell {
              packages = [
                (pkgs.haskellPackages.ghcWithPackages (p: [ p.array p.bytestring p.containers p.text p.mtl p.directory ]))
                pkgs.cabal-install
                pkgs.haskell-language-server
                pkgs.ormolu
              ];
            };
            rust = pkgs.mkShell {
              packages = [ (rust.override { extensions = [ "rust-src" "rust-analyzer" ]; }) pkgs.pkg-config ];
            };
            swift = pkgs.mkShell {
              packages = [ pkgs.swift pkgs.swiftpm pkgs.swiftPackages.Foundation ];
            };
            kotlin = pkgs.mkShell {
              packages = [ pkgs.kotlin pkgs.gradle pkgs.jdk21 ];
            };
            default = pkgs.mkShell {
              packages = [ pkgs.cabal-install rust pkgs.kotlin pkgs.jdk21 ];
            };
          };
        };

      outputs = forAll perSystem;
    in
    {
      packages = lib.mapAttrs (_: o: o.packages) outputs;
      checks = lib.mapAttrs (_: o: o.checks) outputs;
      devShells = lib.mapAttrs (_: o: o.devShells) outputs;
    };
}
