{
  description = "ArkDB: the specification and runtime in Rust, and harken on it";

  # FlakeHub rather than github: for every input, because a machine that can
  # reach FlakeHub and cache.nixos.org but not api.github.com (this one, for
  # instance) must still be able to build. The lock pins the tarballs.
  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.2411.*.tar.gz";
    rust-overlay.url = "https://flakehub.com/f/oxalica/rust-overlay/0.1.*.tar.gz";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

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
      forAll = f: lib.genAttrs systems (system:
        f (import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
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

      # The Rust workspace is `rust/`, and harken's three crates join it from
      # `harken/` by path, so both trees are the source of every Rust build.
      # The kernel's tests read `spec/vectors`, so a build that runs them
      # adds it.
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
          # One wasm page: a binary of the workspace compiled to wasm32 and
          # bound by wasm-bindgen, beside the static files it is loaded from —
          # the browser peer, and the admin page (docs/plan-guards.md D4),
          # which is the same mechanism with another binary and another page.
          wasmPage = { pname, crate, bin ? crate, features ? "", files }: wasmPlatform.buildRustPackage {
            inherit pname;
            version = "0.1.0";
            src = only rustDirs;
            sourceRoot = "source/rust";
            cargoLock.lockFile = ./rust/Cargo.lock;
            nativeBuildInputs = [ wasm-bindgen-cli pkgs.binaryen ];
            # Not cargoBuildHook: it targets the host, and this is the wasm.
            buildPhase = ''
              runHook preBuild
              cargo build --release --offline --frozen -p ${crate} \
                ${features} --target wasm32-unknown-unknown
              wasm-bindgen --target web --no-typescript --out-dir pkg \
                target/wasm32-unknown-unknown/release/${bin}.wasm
              wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
                --enable-sign-ext --enable-mutable-globals --enable-reference-types \
                -o pkg/${bin}_bg.wasm pkg/${bin}_bg.wasm
              runHook postBuild
            '';
            # The page names the module and the wasm with `?v=dev`; this
            # output's own hash replaces it, so a browser holding an older
            # build is sent URLs its cache has never seen (harken/iced/web).
            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp -r pkg $out/
              cp ${files} $out/
              substituteInPlace $out/index.html --replace-fail "v=dev" "v=$(basename $out | cut -c1-32)"
              runHook postInstall
            '';
            doCheck = false;
          };
          icedWeb = { pname, demo }: wasmPage {
            inherit pname;
            crate = "harken-iced";
            features = lib.optionalString demo "--features demo";
            files = "../harken/iced/web/index.html ../harken/iced/web/favicon.svg";
          };
          # The admin page: `ark-explorer`'s `ark-admin` over a server's
          # authority, which ark-server serves on its admin listener
          # (`services.harken.admin.page`).
          ark-admin = wasmPage {
            pname = "ark-admin";
            crate = "ark-explorer";
            bin = "ark-admin";
            files = "ark-explorer/web/index.html";
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

          # The specification is `rust/ark` (spec/README.md), and the tools
          # over it are `rust/ark-dev`: one build of both, whose binaries are
          # `arkc`, which verifies module files and says whether one may
          # follow another, and `ark-fuzz`, the fuzzer. `arkc` and `fuzz` are
          # that build under the names `nix run` takes.
          ark = crate { pname = "ark"; flags = [ "-p" "ark" "-p" "ark-dev" ]; };
          arkc = pkgs.writeShellApplication {
            name = "arkc";
            runtimeInputs = [ ark ];
            text = ''exec arkc "$@"'';
          };
          # `nix run .#fuzz [SESSIONS] [FIRST SEED] [traces [DIR]]`: the
          # fuzzer (rust/ark-dev/src/bin/ark-fuzz.rs), ten thousand sessions
          # from seed 1 by default, each checked for convergence, replay
          # from facts and every frame's bytes; `traces` replays every
          # client's trace too and writes them under DIR. A failure prints
          # its seed and the run exits 1.
          fuzz = pkgs.writeShellApplication {
            name = "ark-fuzz";
            runtimeInputs = [ ark ];
            text = ''exec ark-fuzz "$@"'';
          };
          # The vectors as the writer writes them now: the kernel's tests
          # (hash/, eval/, verify/, module/, views/) and ark-dev's
          # (protocol/, trace/, rebase/) run with ARK_WRITE_VECTORS over an
          # empty spec/vectors, which is then the output. checks.vectors
          # holds the tree's to it; `nix build .#vectors` and a copy over
          # spec/vectors is how they are written again without cargo.
          vectors = crate {
            pname = "ark-vectors";
            flags = [ "-p" "ark" "-p" "ark-dev" ];
            doCheck = true;
            preCheck = ''
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
          trace-corpus = pkgs.runCommand "trace-corpus" { nativeBuildInputs = [ ark ]; } ''
            ark-fuzz 300 2026 traces $out
          '';

          # The binary that writes harken's module file from the domain crate,
          # and the file it writes (harken.ark: the binding map and every
          # body, what the server's HARKEN_MODULE and HARKEN_OLD_MODULES
          # read), verified by arkc.
          harken-domain-bin = crate { pname = "harken-domain"; };
          harken-domain = pkgs.runCommand "harken-domain" { nativeBuildInputs = [ harken-domain-bin ark ]; } ''
            mkdir -p $out
            harken-domain $out/harken.ark
            arkc verify $out/harken.ark
          '';

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
              # The module that revision's peers author at, as its tree has
              # it committed: what a fresh server of this build is told it
              # ran before (`HARKEN_OLD_MODULES`, docs/plan-guards.md D1).
              module = "${input}/harken/domain/harken.ark";
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
            export HARKEN_OLD_${toString n}_MODULE=${v.module}
          '') versions;
        in
        {
          packages = {
            inherit ark arkc vectors fuzz harken-domain trace-corpus;
            harken-server = crate { pname = "harken-server"; };
            # `nix run .#harken-serve [ADDR]`: the dev server, anyone is
            # whoever they say.
            harken-serve = pkgs.callPackage ./harken/server/nix/serve.nix { harken-server = crate { pname = "harken-server"; }; };
            inherit harken-web harken-web-server harken-iced ark-admin;
            default = ark;
            # The F-Droid tooling (nix/fdroid.nix): `harken-fdroid repo --keep`
            # keeps offering what the live repository has. The phone is frozen
            # at spec v4 with the Kotlin runtime (kotlin/FROZEN.md), so this
            # flake builds no APK and the repository for one is not here.
            inherit (import ./nix/fdroid.nix {
              inherit lib pkgs;
              harken-apk = throw "flake.nix: the phone is frozen at spec v4 (kotlin/FROZEN.md); no APK is built";
            }) harken-fdroid fdroid-keygen;
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
              harken-server = crate { pname = "harken-server"; };
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
            versions = crate {
              pname = "harken-versions";
              flags = [ "-p" "harken-server" "--test" "versions" ];
              doCheck = true;
              # The test target needs the two binaries built beside it.
              cargoBuildFlags = [ "-p" "harken-server" ];
              checkFlags = [ "--nocapture" ];
              preCheck = oldEnv;
              installPhase = "touch $out";
            };
            # The vectors in the tree are the ones the writer writes.
            vectors = pkgs.runCommand "check-vectors" { } ''
              diff -r ${vectors} ${./spec/vectors} && touch $out
            '';
            # Every trace of the corpus and of spec/vectors/trace/ replays
            # (rust/ark-dev/tests/vectors.rs, `traces` and `trace_corpus`).
            # Its own check rather than part of checks.rust, so that the
            # workspace's tests do not wait on a fuzz run and a trace that
            # stops replaying is named as that.
            traces = crate {
              pname = "ark-traces";
              flags = [ "-p" "ark-dev" "--test" "vectors" ];
              cargoBuildFlags = [ "-p" "ark-dev" ];
              src = only (rustDirs ++ [ "spec/vectors" ]);
              doCheck = true;
              preCheck = "export ARK_TRACE_CORPUS=${trace-corpus}";
              checkFlags = [ "--nocapture" ];
              installPhase = "touch $out";
            };
            # The fuzzer still generates, verifies, runs and checks: a fixed
            # seed and a few cases, so that the generator cannot rot.
            fuzz-smoke = pkgs.runCommand "check-fuzz-smoke" { nativeBuildInputs = [ ark ]; } ''
              ark-fuzz 25 1 traces
              touch $out
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
            # harken/domain/harken.ark is what the domain crate writes, and
            # arkc verifies it: the file a pinned revision's module is read
            # from (nix/versions.nix), so it must not drift from the source.
            harken-domain = pkgs.runCommand "check-harken-domain" { nativeBuildInputs = [ ark ]; } ''
              diff ${harken-domain}/harken.ark ${./harken/domain/harken.ark} \
                || { echo "harken/domain/harken.ark is not what harken-domain writes: write it again"; exit 1; }
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
