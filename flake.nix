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
    # library is made of (harken/seed/catalogue.json, the copy
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

      # The Rust workspace is `rust/`, and the apps' crates join it from
      # `harken/` and `money/` by path (each app's `seed/` among them), so those trees are the
      # source of every Rust build.
      # The kernel's tests read `spec/vectors`, so a build that runs them
      # adds it.
      # The Ark apps that are generated (nix/ark-app.nix, ui/docs/app.md:
      # "Generated, Not Committed"): each its directory with an app.toml.
      arkApp = import ./nix/ark-app.nix { inherit lib self; };
      arkApps = [ "money" "revelation" ];

      rustDirs = [ "rust" "ui/rust" "modules" "harken/domain" "harken/app" "harken/player" "harken/server" "harken/iced" "harken/ffi" "money/core" "money/seed" "money/app" "money/bridge" "money/views" "money/iced" "money/server" "harken/seed" "money/web" ];
      # The typed Kotlin and Swift `arkdb gen` writes, which harken/ffi's
      # tests hold to its domain (`tests/gen.rs`).
      genDirs = [
        "harken/android/app/src/main/kotlin/dev/harken/android/gen" "harken/ios/Harken/Gen"
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
          # For the tools nixpkgs builds with cargo itself (UniFFI's bindgen).
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
          # crates of harken. Each target (the host, each Android
          # ABI) is nixpkgs for that platform (`importPkgs`'s `crossSystem`)
          # and the toolchain with the target added.
          rustCrates = import ./nix/rust.nix {
            inherit lib;
            cargoNix = import ./rust/Cargo.nix { inherit pkgs; };
            # Each Ark app's generated crates (its generator's main).
            extraCrates = lib.foldl' (acc: dir: acc // arkApp.crates pkgs dir) { } arkApps;
          };
          # The root crate `packageId` of the workspace, for the host unless
          # `pkgs` and `rust` say another target; its binaries under `bin/`,
          # its library under its `lib` output.
          crate = args: rustCrates.build ({ inherit pkgs rust; overrides = desktopOverrides; } // args);
          # COSMIC's iced (rust/Cargo.toml) links libxkbcommon rather than
          # opening it at run time: smithay-client-toolkit's build script
          # asks pkg-config for it, and every crate that links a window
          # (the xkbcommon bindings, the apps, their tests, the docs) is
          # given it to link against. The window still takes the host's
          # first on NixOS (nix/gpu-env.sh).
          desktopOverrides = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux (
            let xkb = _: { nativeBuildInputs = [ pkgs.pkg-config ]; buildInputs = [ pkgs.libxkbcommon ]; }; in
            lib.genAttrs [ "smithay-client-toolkit" "xkbcommon" "arkui" "arkui-shots" "money-iced" "harken-iced" "money-app" "docs" ] (_: xkb)
          );
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
              # Its crates' features as cargo's `--workspace` resolves them
              # (`checksUnified`), or each test build's own (null).
            , unified ? checksUnified
            }:
            let
              src = only dirs;
              # Compiled from a tree that is the crate's directory alone, at
              # the path it has in `dirs`: so `CARGO_MANIFEST_DIR` is where
              # the run finds it, and an edit elsewhere compiles nothing.
              built = { packageId, member, features ? [ "default" ], doctests ? false }:
                let name' = lib.replaceStrings [ "-" ] [ "_" ] packageId; in
                build {
                  inherit packageId features unified;
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
            pkgs.runCommand "test-${name}" ({
              inherit nativeBuildInputs;
              # The crates' test builds, whose dependencies `checks-crates`
              # builds ahead (`rootsOf`).
              passthru.roots = map built crates;
            } // env) ''
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
          docsCrate = args: docsCrates.build ({ inherit pkgs rust; packageId = "docs"; overrides = desktopOverrides; } // args);
          # The site, an ArkUI app written as static HTML (ui/docs/app.md,
          # "The Browser's Modes"): every page drawn by arkui-web natively,
          # each desktop component's pictures drawn by iced's own renderer
          # on the CPU (tiny-skia: no window, no GPU), which the sandbox
          # allows, and beside them the module that takes a page over
          # (`docs-web`, the same crate for wasm32 under `hydrate`).
          docs-web =
            let
              wasm = docsCrates.build { pkgs = wasmPkgs; rust = wasmRust; packageId = "docs"; features = [ "hydrate" ]; overrides = wasmOverrides; };
            in
            pkgs.runCommand "docs-web" { nativeBuildInputs = [ wasm-bindgen-cli pkgs.binaryen ]; passthru = { inherit wasm; }; } ''
              wasm-bindgen --target web --no-typescript --out-dir $out/pkg ${wasm}/bin/docs-web.wasm
              wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
                --enable-sign-ext --enable-mutable-globals --enable-reference-types \
                -o $out/pkg/docs-web_bg.wasm $out/pkg/docs-web_bg.wasm
            '';
          # Served at https://k2on.github.io/apps/docs/; the module's URLs
          # named by its build's hash, so a browser holding an older one is
          # sent URLs its cache has never seen.
          docs = pkgs.runCommand "docs" { } ''
            ${docsCrate { }}/bin/docs $out --base /apps/docs/ --site https://k2on.github.io --version $(basename ${docs-web} | cut -c1-32)
            cp -r ${docs-web}/pkg $out/
          '';

          # Python with Pillow, which draws the README's heroes from the
          # published screenshots (.github/screens/hero.py).
          screens-python = pkgs.python3.withPackages (ps: [ ps.pillow ]);

          # The screens workflows' ffmpeg and XcodeGen, in a file of their
          # own: screens.yaml's cache of them is keyed on it.
          inherit (import ./nix/screens-tools.nix { inherit pkgs; }) screens-ffmpeg xcodegen;

          # The server (harken/server: the server and harken-peer): its
          # crate and what it reaches, under the features it asks for, and
          # none of the GUI the workspace's other members link.
          harken-server = crate { packageId = "harken-server"; };
          # `arkdb` for a server to read its system's events with
          # (docs/plan-observe.md): no GUI stack.
          arkdb = crate { packageId = "arkdb"; };
          # money's server (money/server: the hub and the bank importer), the
          # same way: no GUI stack under a server.
          # money's server is one process for everything the household
          # reaches: the sync socket, sign-in, the importer and the browser
          # client, whose build it carries (MONEY_WEB, unless the
          # environment names another).
          money-server-bin = crate { packageId = "money-server"; };
          # Its browser client is static files, the same from any machine:
          # on a Mac, Linux's (built ahead there and fetched), rather than
          # the Mac compiling the client's wasm itself.
          money-server = pkgs.runCommand "money-server-${money-server-bin.version or "0"}" { nativeBuildInputs = [ pkgs.makeWrapper ]; } ''
            mkdir -p $out/bin
            makeWrapper ${money-server-bin}/bin/money-server $out/bin/money-server --set-default MONEY_WEB ${if pkgs.stdenv.hostPlatform.isDarwin then outputs.x86_64-linux.packages.money-web else money-web}
          '';
          # Each app's demo data (docs/seed.md): `<app>-seed DATA` writes what
          # that app's server boots seeded from; money's is money/seed.
          money-seed = crate { packageId = "money-seed"; features = [ "cli" ]; };
          # Each Ark app's generated pieces (nix/ark-app.nix): its
          # generator, the SwiftUI and Compose of its #[view]s (`<app>-ui`),
          # its native projects (`<app>-native`) and its prebuild.
          arkOf = lib.genAttrs arkApps (arkApp.packages { inherit pkgs only bundlePrefix; build = crate; });
          harken-seed = crate { packageId = "harken-seed"; };
          # The catalogue's covers, kept in the repository (harken/seed/covers,
          # each 600 pixels at most, named as harken-seed's `media::key` names
          # them: the first sixteen hex digits of the address's SHA-256), so
          # seeding fetches nothing (`harken-seed --covers`) and every run has
          # the same bytes: a host's thumbnails are not the same bytes from
          # one fetch to the next, which a hash pinned against them failed on.
          harken-covers = builtins.path { path = ./harken/seed/covers; name = "harken-covers"; };
          # `nix run .#update-covers`: harken/seed/covers from the catalogue,
          # each fetched and made small, after the catalogue moves.
          update-covers = pkgs.writeShellApplication {
            name = "update-covers";
            runtimeInputs = [ pkgs.jq pkgs.coreutils pkgs.curl pkgs.imagemagick ];
            text = builtins.readFile ./nix/update-covers.sh;
          };
          # `nix run .#<app>-demo [ADDR]`: that server on a fresh seeded
          # directory, with dev auth, for a person to try.
          money-demo = pkgs.callPackage ./nix/demo.nix {
            name = "money";
            seed = money-seed;
            server = money-server;
            listen = "127.0.0.1:8788";
            env = ''MONEY_DEV_AUTH=1 MONEY_DATA="$dir"'';
          };
          harken-demo = pkgs.callPackage ./nix/demo.nix {
            name = "harken";
            seed = harken-seed;
            server = harken-server;
            listen = "127.0.0.1:8787";
            env = ''HARKEN_DEV_AUTH=1 HARKEN_DATA="$dir" HARKEN_MEDIA="$dir/media"'';
            seedArgs = "--covers ${harken-covers}";
          };

          # The desktop window. winit and wgpu open the display and the GPU
          # by dlopen at run time, so the libraries go on the library path
          # — at launch, by nix/gpu-env.sh, because which come first is the
          # host's to answer: on NixOS the GPU driver in /run/opengl-driver
          # was linked against the host's libwayland, and the flake's ahead
          # of it on LD_LIBRARY_PATH kept every Mesa driver from loading, so
          # iced drew on the CPU without a word (the script says why, in
          # full). The window then says which renderer it got, on stderr
          # (ui/rust/arkui/src/renderer.rs). money's demo window is the same.
          icedLibs = lib.makeLibraryPath (with pkgs; [
            wayland libxkbcommon vulkan-loader libGL xorg.libX11 xorg.libXcursor xorg.libXi xorg.libXrandr
          ]);
          # What a shell (or a launcher) sources to be given that path.
          gpuEnv = pkgs.writeText "ark-gpu-env.sh" ''
            ARK_FLAKE_LIBS=${icedLibs}
            ARK_LDD=${lib.getBin pkgs.glibc}/bin/ldd
            ${builtins.readFile ./nix/gpu-env.sh}
          '';
          icedApp = packageId:
            let built = crate { inherit packageId; }; in
            if pkgs.stdenv.hostPlatform.isLinux then
              pkgs.runCommand (lib.removePrefix "rust_" built.name) { } ''
                mkdir -p $out/bin
                cat > $out/bin/${packageId} <<'EOF'
                #!${pkgs.runtimeShell}
                source ${gpuEnv}
                exec -a "$0" ${built}/bin/${packageId} "$@"
                EOF
                chmod +x $out/bin/${packageId}
              ''
            else built;
          harken-iced = icedApp "harken-iced";
          money-iced = icedApp "money-iced";

          # The browser (ui/docs/app.md, "The Layers"): an app's Leptos
          # client (`<app>/web`, on arkui-web) compiled to wasm32 and bound by
          # wasm-bindgen, beside its page — one static directory, no trunk.
          # The toolchain is the workspace's with the wasm target added, and
          # the CLI is built from the crate at the version the lockfile names.
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
          # Leptos's code splitting (wasm_split_helpers) names its markers
          # by the crate's manifest's path, which cargo says in
          # CARGO_MANIFEST_PATH and buildRustCrate does not: any path will
          # do, as long as it is the same every build.
          wasmOverrides = {
            wasm_split_helpers = _: { CARGO_MANIFEST_PATH = "wasm_split_helpers/Cargo.toml"; };
          };
          # One wasm page: a binary of the workspace compiled to wasm32 and
          # bound by wasm-bindgen, beside the static files it is loaded from.
          wasmPage = { pname, crate, bin ? crate, features ? [ "default" ], files }:
            let
              wasm = rustCrates.build { pkgs = wasmPkgs; rust = wasmRust; packageId = crate; inherit features; overrides = wasmOverrides; };
            in
            # `passthru.wasm`: the module before it is bound, to build alone.
            pkgs.runCommand pname { nativeBuildInputs = [ wasm-bindgen-cli pkgs.binaryen ]; passthru = { inherit wasm; }; } ''
              wasm-bindgen --target web --no-typescript --out-dir pkg ${wasm}/bin/${bin}.wasm
              wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
                --enable-sign-ext --enable-mutable-globals --enable-reference-types \
                -o pkg/${bin}_bg.wasm pkg/${bin}_bg.wasm
              # The page names the module and the wasm with `?v=dev`; this
              # output's own hash replaces it, so a browser holding an older
              # build is sent URLs its cache has never seen.
              mkdir -p $out
              cp -r pkg $out/
              ${lib.concatMapStrings (f: "cp ${f} $out/${baseNameOf f}\n") files}
              if [ -f $out/index.html ]; then
                chmod u+w $out/index.html
                substituteInPlace $out/index.html --replace-fail "v=dev" "v=$(basename $out | cut -c1-32)"
              fi
            '';
          # money's web client: the shell and its screens over the demo
          # household, in the page alone (money/web).
          money-web = wasmPage {
            pname = "money-web";
            crate = "money-web";
            files = [ ./money/web/index.html ./money/web/favicon.svg ];
          };
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
          # The bindgen reads each crate's uniffi.toml from the directory
          # `[crate-roots]` names for it: those four files and nothing else of
          # the workspace, so the bindings (and an APK over them) move only
          # when the library or one of them does. A crate with none takes
          # UniFFI's defaults.
          ffiBindings = { lib, language, out ? "$TMPDIR/bindings" }:
            let
              roots = only [ "rust/ark-ffi/uniffi.toml" "rust/ark-auth/uniffi.toml" "harken/ffi/uniffi.toml" ];
              config = pkgs.writeText "uniffi-roots.toml" ''
                [crate-roots]
                ark_ffi = "${roots}/rust/ark-ffi"
                ark_auth = "${roots}/rust/ark-auth"
                harken_ffi = "${roots}/harken/ffi"
              '';
            in
            ''
              ${uniffiBindgen}/bin/uniffi-bindgen generate ${lib} --language ${language} --no-format \
                --config ${config} --out-dir "${out}/${language}"
            '';
          # UniFFI's bindgen binary (nix/uniffi-bindgen), which is UniFFI's
          # main at the bridge's version: its own crate and lockfile, built in
          # one cargo build. Crate by crate, `uniffi` and `uniffi_bindgen`
          # came from different runs whose `uniffi_bindgen` hashed
          # differently, and would not link (x86_64 CI, 2026-11-04).
          uniffiBindgen = rustPlatform.buildRustPackage {
            pname = "uniffi-bindgen";
            version = "0.32.2";
            src = ./nix/uniffi-bindgen;
            cargoLock.lockFile = ./nix/uniffi-bindgen/Cargo.lock;
            doCheck = false;
          };
          # An app's library for the host (`crate` its ffi crate, `so` the
          # library's name), which UniFFI reads the bindings from and the
          # app's JVM tests load; with the crate's `features`, which are the
          # phone's (none for an iPhone: money's `page` is Android's).
          ffiHostLib = { crate, so, features ? [ "default" ], ... }:
            "${(rustCrates.build { inherit pkgs rust features; packageId = crate; }).lib}/lib/lib${so}${pkgs.stdenv.hostPlatform.extensions.sharedLibrary}";
          harkenFfi = { name = "harken"; crate = "harken-ffi"; so = "harken_ffi"; };
          # An Ark app's library is the shim nix/ark-app.nix generates over
          # its crate (`ark`: its app.toml), no crate of the app's own.
          moneyFfi = { name = "money"; crate = "money-ffi"; so = "money_ffi"; ark = arkApp.read "money"; };
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
          # An app's library for its phones (`harken/ffi`; money's the
          # generated shim over money/app) as the
          # APK carries it: `name` the app, `crate` its ffi crate, `so` the
          # library's name (`harken_ffi`).
          androidPhone = { name, crate, so, ark ? null }:
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
          # money's (the generated `money-ffi` over money/app): what its APK
          # builds over.
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
          # `ui/kotlin`, the look every Ark app on Android shares.
          #
          # An Ark app (`ark`, its nix/ark-app.nix packages) builds its
          # generated project (`<name>/.native/android`) over its own Kotlin
          # (`<name>/native/android`), its Maven graph that directory's
          # deps.json.
          androidApp = { name, appid, flags ? [ ], shared ? [ ], title, note, ark ? null }:
            let
              dir = if ark == null then "${name}/android" else "${name}/.native/android";
              deps = if ark == null then "${name}/android/deps.json" else "${name}/native/android/deps.json";
              src =
                if ark == null then only ([ dir ] ++ shared)
                # Named `source`, as `only` names its trees: unpacked under
                # that name, which sourceRoot says.
                else pkgs.runCommand "source" { } ''
                  cp -r ${only ([ "${name}/native/android" "${name}/app.toml" ] ++ shared)} $out
                  chmod -R u+w $out
                  cp -r ${ark.native}/. $out/
                '';
              apk = pkgs.stdenv.mkDerivation (final: {
                pname = "${name}-apk";
                version = "0.1.0";
                inherit src;
                sourceRoot = "source/${dir}";
                postUnpack = "chmod -R u+w source";
                nativeBuildInputs = [ pkgs.gradle pkgs.jdk21 ];
                mitmCache = pkgs.gradle.fetchDeps {
                  pkg = final.finalPackage;
                  data = ./. + "/${deps}";
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
                  # Its id: every app's is `bundlePrefix` and its name.
                  "-PapplicationId=${bundlePrefix}.${name}"
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
                # The APK alone: its tests are a build of their own (`tests`,
                # below), so the APK is there for its screenshots while they
                # run, and the F-Droid deploy waits for both.
                gradleBuildTask = ":app:assembleRelease";
                # The recording runs the same tasks, and the screenshots' and a
                # laptop's debug build, so that one recording serves all three.
                gradleUpdateTask = ":app:testDebugUnitTest :app:recordPaparazziDebug :app:assembleDebug :app:assembleRelease";
                doCheck = false;
                installPhase = ''
                  mkdir -p $out
                  cp app/build/outputs/apk/release/app-release.apk $out/${name}.apk
                '';
              });
              # The app's unit tests, as a build: what the APK used to run
              # before it was made. Their results are its output.
              tests = apk.overrideAttrs (old: {
                pname = "${name}-android-tests";
                gradleBuildTask = ":app:testDebugUnitTest";
                versionFlags = [ ];
                installPhase = ''
                  mkdir -p $out
                  if [ -d app/build/test-results ]; then cp -r app/build/test-results $out/; fi
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
            { inherit apk tests screens; deps = apk.mitmCache.updateScript; };

          # The phone (harken/android/README.md): the app — its onboarding
          # and its look are its own code — over `harken-android-lib` (the
          # kernel per ABI, and the Kotlin over it). Its screens are rendered
          # from a fixed library (harken/android/app/src/test).
          harken = androidApp {
            name = "harken";
            appid = "dev.harken.android";
            flags = [ "-PharkenLib=${harken-android-lib}" ];
            shared = [ "ui/kotlin" ];
            title = "Harken";
            note = "Rendered on the JVM by Paparazzi from a fixed library, at a Pixel 5's size; light beside dark. Covers are the derived squares: nothing here talks to a server.";
          };
          harken-apk = harken.apk;
          harken-android-tests = harken.tests;
          harken-android-screens = harken.screens;

          # money's phone (money/native/android/README.md): Compose over
          # `money-android-lib`, the look shared with harken (ui/kotlin), in
          # the project ui/prebuild.py generates. Its Maven graph is
          # harken's, replayed (money/native/android/deps.json).
          # `appid` here is the package the screenshots are named by; the
          # installed app is us.koon.money.
          money = androidApp {
            name = "money";
            appid = arkOf.money.a.android.package;
            ark = arkOf.money;
            flags = [ "-ParkLib=${money-android-lib}" ];
            # arkui, and the device modules money lists (money/app.toml),
            # whose kotlin/ its generated build.gradle.kts compiles in.
            shared = [ "ui/kotlin" "modules" ];
            title = "money";
            note = "Rendered on the JVM by Paparazzi from the demo's household on 2026-10-26, at a Pixel 5's size, in money's own scheme (a screenshot has no wallpaper); light beside dark.";
          };
          money-apk = money.apk;
          money-android-tests = money.tests;
          money-android-screens = money.screens;

          # The Bible as one scroll, zoomed like a map (revelation/README.md,
          # docs/plan-bible.md): plain Kotlin, no native library and none of
          # ArkDB, so it is the SDK and gradle alone.
          revelation = androidApp {
            name = "revelation";
            appid = arkOf.revelation.a.android.package;
            ark = arkOf.revelation;
            # ArkUI's screenshot mode and ready signal alone (ui/kotlin/shots).
            shared = [ "ui/kotlin/shots" ];
            title = "Revelation";
            note = "Rendered on the JVM by Paparazzi through layoutlib; light beside dark.";
          };

          # An Ark app's F-Droid metadata, written from its app.toml's
          # [store] by ui/prebuild.py (nothing of F-Droid's is committed).
          storeYml = dir: pkgs.runCommand "${dir}-fdroid.yml" { nativeBuildInputs = [ pkgs.python3 ]; } ''
            cd ${only [ "ui/prebuild.py" "ui/theme.py" "ui/theme.toml" "modules" "${dir}/app.toml" ]}
            python3 ui/prebuild.py fdroid ${dir} --root . --out $out
          '';

          # Every app as one F-Droid repository (nix/fdroid.nix): each app is
          # its id, its APK, its metadata and its icon.
          fdroid = import ./nix/fdroid.nix {
            inherit lib pkgs;
            apps = [
              {
                appid = "${bundlePrefix}.harken";
                apk = harken-apk;
                apkName = "harken.apk";
                metadata = ./harken/android/fdroid/metadata/us.koon.harken.yml;
                # The trumpet the desktop window and the docs draw.
                icon = ./harken/iced/icon.svg;
              }
              {
                appid = "${bundlePrefix}.revelation";
                apk = revelation.apk;
                apkName = "revelation.apk";
                metadata = storeYml "revelation";
                icon = ./revelation/icon.svg;
              }
              {
                appid = "${bundlePrefix}.money";
                apk = money-apk;
                apkName = "money.apk";
                metadata = storeYml "money";
                icon = ./money/icon.svg;
              }
            ];
          };

          # The same library for iOS — `aarch64-apple-ios` for a phone and
          # `aarch64-apple-ios-sim` for the simulator on an Apple-silicon Mac —
          # as `<Framework>.xcframework` (`HarkenFFI`, `MoneyFFI`), with
          # UniFFI's Swift and the typed half under `swift/`. This one is
          # `harken-ios-xcframework` and `money-ios-xcframework`, a Mac's own
          # build with whatever Xcode it has selected, for working in Xcode;
          # what the pinned apps link (`iosKernel`) is built on Linux
          # (`appleKernel`, below), and on Linux these evaluate and are not
          # built (meta.platforms).
          # It is cargo's build, over the lockfile's crates vendored, and not
          # a derivation per crate: nixpkgs has no iOS package set that
          # compiles with the builder's Xcode (the workflow, ios.yaml, builds
          # it with cargo too).
          #
          # **Each slice only where it is wanted.** A kernel is built for the
          # `targets` asked for, each a cargo build of its own: the simulator's
          # alone for the screenshots' app and the tests, the phone's alone for
          # the archive, so no build compiles a slice nothing of it links (each
          # was half of a kernel's time).
          #
          # **The crates' dependencies apart** (`iosDeps`), as crane's
          # `buildDepsOnly` has them: the lockfile's crates compiled once for a
          # target, over the workspace's manifests and an empty stand-in for
          # each of its crates (`stubs`), so that derivation moves only with
          # Cargo.lock, the manifests, the toolchain and Xcode. A kernel starts
          # from its `target/` and compiles only the workspace's crates.
          #
          # **Only the phone's crates.** A kernel's source is the crates its
          # library is built from (`phoneCrates`, its path dependencies read
          # from their manifests) and, of the rest of the workspace, only the
          # manifests, each crate an empty stand-in: cargo reads the whole
          # workspace (`--locked --offline`), and a desktop window's or a
          # server's change is no phone's.
          iosTargets = [ "aarch64-apple-ios" "aarch64-apple-ios-sim" ];
          # The Mac's slice, for the same app's macOS destination (one
          # multiplatform target: ui/prebuild.py's project.yml).
          iosRust = rust.override { targets = iosTargets ++ [ "aarch64-apple-darwin" ]; };
          # Each target's slice of the xcframework: its directory and its
          # platform variant.
          iosSlice = {
            aarch64-apple-ios = { id = "ios-arm64"; variant = ""; platform = "ios"; };
            aarch64-apple-ios-sim = { id = "ios-arm64-simulator"; variant = "simulator"; platform = "ios"; };
            aarch64-apple-darwin = { id = "macos-arm64"; variant = ""; platform = "macos"; };
          };
          # The workspace's crates, by directory from the repository's root,
          # and by package name.
          rustMembers = map (m: if lib.hasPrefix "../" m then lib.removePrefix "../" m else "rust/${m}")
            (builtins.fromTOML (builtins.readFile ./rust/Cargo.toml)).workspace.members;
          rustManifest = dir: builtins.fromTOML (builtins.readFile (./. + "/${dir}/Cargo.toml"));
          rustMember = lib.listToAttrs (map (d: { name = (rustManifest d).package.name; value = d; }) rustMembers);
          # `a/b/../c` as `a/c`.
          normalise = p: lib.concatStringsSep "/" (lib.foldl' (acc: s:
            if s == ".." then lib.init acc else if s == "." || s == "" then acc else acc ++ [ s ]) [ ] (lib.splitString "/" p));
          # A crate's path dependencies (what it links: [dependencies] and
          # [build-dependencies], a target's own included; not its tests').
          rustPathDeps = dir:
            let
              m = rustManifest dir;
              tables = [ m ] ++ lib.attrValues (m.target or { });
              deps = lib.concatMap (t: lib.attrValues ((t.dependencies or { }) // (t.build-dependencies or { }))) tables;
            in
            lib.concatMap (d: lib.optional (builtins.isAttrs d && d ? path) (normalise "${dir}/${d.path}")) deps;
          phoneCrates = crate: map (x: x.key) (builtins.genericClosure {
            startSet = [{ key = rustMember.${crate}; }];
            operator = x: map (d: { key = d; }) (rustPathDeps x.key);
          });
          # The shell that writes an empty stand-in for each target of each
          # crate in `dirs` (its lib.rs, main.rs, src/bin/*, build.rs, as it
          # has them), from the repository's root.
          stubs = dirs: lib.concatMapStrings (dir:
            let
              has = f: builtins.pathExists (./. + "/${dir}/${f}");
              bin = ./. + "/${dir}/src/bin";
              bins = lib.optionals (builtins.pathExists bin)
                (map (n: "src/bin/${n}") (lib.attrNames (lib.filterAttrs (_: t: t == "regular") (builtins.readDir bin))));
            in
            lib.concatMapStrings (f: ''
              mkdir -p "$(dirname ${dir}/${f})"
              echo ${if f == "src/lib.rs" then "" else "'fn main() {}'"} > ${dir}/${f}
            '') (lib.filter has [ "src/lib.rs" "src/main.rs" "build.rs" ] ++ bins)) dirs;
          # A source of the workspace's manifests and lockfile, the crates in
          # `real` as they are, every other one a stand-in; and an Ark app's
          # phones' library (`ark`, its app.toml), the shim nix/ark-app.nix
          # writes, made a member at `<app>/.ffi`.
          rustSource = { real, ark ? null }: {
            src = only ([ "rust/Cargo.toml" "rust/Cargo.lock" ] ++ real ++ map (d: "${d}/Cargo.toml") rustMembers);
            sourceRoot = "source/rust";
            # Unpacked read-only but for the source root: the stand-ins are
            # written beside it.
            postUnpack = ''
              chmod -R u+w source
              (cd source && ${stubs (lib.subtractLists real rustMembers)})
            '' + lib.optionalString (ark != null) ''
              mkdir -p source/${ark.dir}/.ffi
              cp ${pkgs.writeText "Cargo.toml" (arkApp.ffiCargo ark)} source/${ark.dir}/.ffi/Cargo.toml
              cp ${pkgs.writeText "lib.rs" (arkApp.ffiSource ark)} source/${ark.dir}/.ffi/lib.rs
              sed -i 's|^members = \[|members = ["../${ark.dir}/.ffi", |' source/rust/Cargo.toml
            '';
          };
          # The crates a phone's library is built from: an Ark app's crate's,
          # or the library crate's own.
          phoneSource = { crate, ark ? null }:
            rustSource { real = phoneCrates (if ark == null then crate else ark.ffi.crate); inherit ark; };
          # The environment each iOS cargo build is in, and its cargo build of
          # `crate` for each of `targets`.
          # An Ark app's shim is no member of the lockfile, which cargo then
          # adds it to (`locked` false): offline, over the vendored crates.
          iosCargo = { crate, targets, xcode, locked ? true }:
            let tc = if xcode == null then "/usr/bin" else "$DEVELOPER_DIR/Toolchains/XcodeDefault.xctoolchain/usr/bin";
            in {
              inherit tc;
              # COSMIC's iced and its dependencies from GitHub are in the
              # lockfile (rust/Cargo.toml); the phones link none of them, but
              # the lockfile is vendored whole, each one fetched at the
              # revision it pins. By the workspace's own toolchain's cargo
              # (`rustPlatform`): their manifests are edition 2024, which
              # nixpkgs' cargo (1.82) cannot read.
              cargoDeps = rustPlatform.importCargoLock { lockFile = ./rust/Cargo.lock; allowBuiltinFetchGit = true; };
              nativeBuildInputs = [ iosRust rustPlatform.cargoSetupHook ];
              # xcodebuild and the iPhoneOS and simulator SDKs: the builder's
              # Xcode, outside the store. Pinned, only its directory is read,
              # from inside the sandbox (`__impureHostDeps`, which the builder's
              # nix.conf must allow: `allowed-impure-host-deps`).
              __noChroot = xcode == null;
              __impureHostDeps = lib.optional (xcode != null) xcode.app;
              # The iOS targets compile and link with the builder's Xcode: its
              # developer directory, clang, ar and SDKs. nixpkgs' darwin stdenv
              # sets DEVELOPER_DIR and SDKROOT to its own macOS SDK, which has no
              # iPhoneOS platform, so `xcrun --sdk iphoneos` (cc-rs asks it)
              # fails under them: both are taken back for these targets.
              build = ''
                # The apps' own deployment target: what ios.yaml's cargo had;
                # the Mac's, the app's macOS destination's.
                export IPHONEOS_DEPLOYMENT_TARGET=17.0 MACOSX_DEPLOYMENT_TARGET=15.0
                ${if xcode == null then ''
                  export PATH=/usr/bin:$PATH
                  export DEVELOPER_DIR="$(/usr/bin/xcode-select -p)"
                '' else ''
                  export DEVELOPER_DIR=${xcode.developer}
                  ${xcode.check}
                  export PATH=$DEVELOPER_DIR/usr/bin:${tc}:$PATH
                  export HOME=$TMPDIR/home
                  mkdir -p "$HOME"
                ''}
                unset SDKROOT
                ${lib.concatMapStrings (t: let u = lib.toUpper (lib.replaceStrings [ "-" ] [ "_" ] t); l = lib.replaceStrings [ "-" ] [ "_" ] t; in ''
                  export CC_${l}=${tc}/clang CXX_${l}=${tc}/clang++ AR_${l}=${tc}/ar
                  export CARGO_TARGET_${u}_LINKER=${tc}/clang
                  cargo build --release --offline ${lib.optionalString locked "--locked"} -p ${crate} --lib --no-default-features --target ${t}
                '') targets}
              '';
            };
          # The lockfile's crates, compiled for `targets` as `crate` links
          # them: its `target/`, which a kernel starts from.
          iosDeps = { name, crate, targets, xcode ? null, ark ? null }:
            let c = iosCargo { inherit crate targets xcode; locked = ark == null; };
            in pkgs.stdenv.mkDerivation ({
              pname = "${name}-ios-deps";
              version = "0.1.0";
              inherit (c) cargoDeps nativeBuildInputs __noChroot __impureHostDeps;
              buildPhase = ''
                runHook preBuild
                ${c.build}
                runHook postBuild
              '';
              installPhase = "mkdir -p $out && cp -r target $out/";
              dontFixup = true;
              meta.platforms = [ "aarch64-darwin" "x86_64-darwin" ];
            } // rustSource { real = [ ]; inherit ark; });
          # One app's: `name`, `crate` and `so` as `androidPhone`'s, the
          # framework's name, and its `targets`. With `xcode`, the pinned
          # Xcode (`xcode` below), it is built from the runner's installed
          # copy of it, under the strict sandbox with that one directory let
          # in; without, from whatever Xcode the builder has selected,
          # unsandboxed. The Swift is UniFFI's from the library built for
          # this Mac.
          iosPhone = { name, crate, so, framework, xcode ? null, targets ? iosTargets, ark ? null }:
            let
              c = iosCargo { inherit crate targets xcode; locked = ark == null; };
              deps = iosDeps { inherit name crate targets xcode ark; };
            in
            pkgs.stdenv.mkDerivation ({
              pname = "${name}-ios-xcframework${lib.optionalString (targets != iosTargets) "-${lib.concatMapStringsSep "-" (t: iosSlice.${t}.id) targets}"}";
              version = "0.1.0";
              inherit (c) cargoDeps nativeBuildInputs __noChroot __impureHostDeps;
              passthru = { inherit deps; };
              buildPhase = ''
                runHook preBuild
                # The dependencies as `deps` compiled them, every file of
                # them one time, newer than the vendored crates unpacked
                # here (so cargo takes them as built from these); and the
                # workspace's crates newer still than what it compiled of
                # their stand-ins, so that cargo compiles them again, and
                # only them.
                cp -r ${deps}/target target
                chmod -R u+w target
                now=$(date +%s)
                find target -exec touch -h -d "@$now" {} +
                find .. -path '*/target' -prune -o -name '*.rs' -exec touch -d "@$((now + 2))" {} +
                ${c.build}
                ${ffiBindings { lib = ffiHostLib { inherit crate so; features = [ ]; }; language = "swift"; }}
                mkdir -p headers
                cp $TMPDIR/bindings/swift/*.h headers/
                cat $TMPDIR/bindings/swift/*.modulemap > headers/module.modulemap
                ${if xcode == null then ''
                  /usr/bin/xcodebuild -create-xcframework \
                    ${lib.concatMapStrings (t: "-library target/${t}/release/lib${so}.a -headers headers ") targets} \
                    -output ${framework}.xcframework
                '' else ''
                  # By hand, as xcodebuild lays it out: under the strict sandbox
                  # `xcodebuild -create-xcframework` crashes (xcode-strict.yaml),
                  # and an xcframework of static libraries is only this.
                  for slice in ${lib.concatMapStringsSep " " (t: "${iosSlice.${t}.id}:${t}") targets}; do
                    id=''${slice%%:*} target=''${slice##*:}
                    mkdir -p ${framework}.xcframework/$id
                    cp target/$target/release/lib${so}.a ${framework}.xcframework/$id/
                    cp -r headers ${framework}.xcframework/$id/Headers
                  done
                  slice() {
                    echo "<dict><key>BinaryPath</key><string>lib${so}.a</string><key>HeadersPath</key><string>Headers</string>"
                    echo "<key>LibraryIdentifier</key><string>$1</string><key>LibraryPath</key><string>lib${so}.a</string>"
                    echo "<key>SupportedArchitectures</key><array><string>arm64</string></array><key>SupportedPlatform</key><string>$3</string>"
                    [ -z "$2" ] || echo "<key>SupportedPlatformVariant</key><string>$2</string>"
                    echo "</dict>"
                  }
                  {
                    echo '<?xml version="1.0" encoding="UTF-8"?>'
                    echo '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
                    echo '<plist version="1.0"><dict><key>AvailableLibraries</key><array>'
                    ${lib.concatMapStrings (t: ''
                      slice ${iosSlice.${t}.id} "${iosSlice.${t}.variant}" ${iosSlice.${t}.platform}
                    '') targets}
                    echo '</array><key>CFBundlePackageType</key><string>XFWK</string>'
                    echo '<key>XCFrameworkFormatVersion</key><string>1.0</string></dict></plist>'
                  } > ${framework}.xcframework/Info.plist
                ''}
                runHook postBuild
              '';
              installPhase = ''
                mkdir -p $out/swift
                cp -r ${framework}.xcframework $out/
                cp $TMPDIR/bindings/swift/*.swift ark-ffi/swift/Typed.swift $out/swift/
              '';
              meta.platforms = [ "aarch64-darwin" "x86_64-darwin" ];
            } // phoneSource { inherit crate ark; });
          harken-ios-xcframework = iosPhone (harkenFfi // { framework = "HarkenFFI"; });
          money-ios-xcframework = iosPhone (moneyFfi // { framework = "MoneyFFI"; });

          # **The Apple kernels, built on Linux.** Each slice of a kernel (the
          # phone's, the simulator's, the Mac's) is a static library, which
          # needs no linker: rustc writes objects and an archive. So it is
          # cross-compiled on Linux, a derivation per crate (nix/rust.nix) as
          # Android's is, and a Mac only fetches it from the cache and links
          # it into the app with Xcode. The one C in a kernel is ring's (its
          # crypto, for rustls): compiled by LLVM's clang for the Apple
          # target against the headers of Apple's macOS SDK, which nixpkgs
          # fetches from Apple (`appleSdk`, substituted from cache.nixos.org)
          # and whose TargetConditionals.h says iOS or the simulator by the
          # compiler's target, so the same headers serve every slice. No
          # bindgen, and nothing that links. A change to one app's crates
          # rebuilds those crates and the library, and every crate the two
          # kernels share (ark, ark-ffi, rustls, …) is the same derivation
          # in both and comes from the cache.
          appleSdk =
            let
              dir = "${nixpkgs}/pkgs/by-name/ap/apple-sdk";
              v = (builtins.fromJSON (builtins.readFile "${dir}/metadata/versions.json"))."15";
            in
            (pkgs.callPackage "${dir}/common/fetch-sdk.nix" { }) v;
          # Each target: nixpkgs' platform for it, and clang's triple, with
          # the deployment version the apps have (ios.yaml's, the Mac
          # destination's).
          appleTargets = {
            aarch64-apple-ios = { config = "aarch64-apple-ios"; clang = "arm64-apple-ios17.0"; env = "IPHONEOS_DEPLOYMENT_TARGET"; version = "17.0"; };
            aarch64-apple-ios-sim = { config = "aarch64-apple-ios"; clang = "arm64-apple-ios17.0-simulator"; env = "IPHONEOS_DEPLOYMENT_TARGET"; version = "17.0"; };
            aarch64-apple-darwin = { config = "aarch64-apple-darwin"; clang = "arm64-apple-macos15.0"; env = "MACOSX_DEPLOYMENT_TARGET"; version = "15.0"; };
          };
          # nixpkgs for the target, with no C toolchain of its own (none
          # builds for Apple on Linux, and none is needed): buildRustCrate
          # on its stdenvNoCC, which passes rustc no linker.
          applePkgs = t:
            let
              p = importPkgs {
                localSystem = pkgs.stdenv.buildPlatform.system;
                # The Mac's version as nixpkgs' own (its darwin setup sets
                # MACOSX_DEPLOYMENT_TARGET from it, 11.0 unless told).
                crossSystem = { inherit (appleTargets.${t}) config; rust.rustcTarget = t; }
                  // lib.optionalAttrs (appleTargets.${t}.env == "MACOSX_DEPLOYMENT_TARGET") { darwinMinVersion = appleTargets.${t}.version; };
                config = { allowUnfree = true; allowUnsupportedSystem = true; };
              };
            in
            p // { buildRustCrate = p.buildRustCrate.override { stdenv = p.stdenvNoCC; libiconv = null; }; };
          # clang for the target, as cc-rs runs it (ring's build script): the
          # SDK's headers named, as nixpkgs' clang searches no sysroot itself.
          appleCc = t: pkgs.writeShellScript "cc-${t}" ''
            exec ${pkgs.llvmPackages.clang-unwrapped}/bin/clang -target ${appleTargets.${t}.clang} \
              -isysroot ${appleSdk} -resource-dir ${pkgs.llvmPackages.clang}/resource-root \
              -idirafter ${appleSdk}/usr/include "$@"
          '';
          # One crate's static library for one Apple target: the crate's
          # library as `staticlib` alone (cargo's `--lib --no-default-features`).
          appleLib = { crate, target }:
            let tt = appleTargets.${target}; l = lib.replaceStrings [ "-" ] [ "_" ] target; in
            rustCrates.build {
              pkgs = applePkgs target;
              rust = rust.override { targets = [ target ]; };
              packageId = crate;
              features = [ ];
              root = { type = [ "staticlib" ]; };
              overrides.ring = _: {
                "CC_${l}" = appleCc target;
                "AR_${l}" = "${pkgs.llvmPackages.bintools-unwrapped}/bin/llvm-ar";
                SDKROOT = appleSdk;
              };
              targetAttrs = _: { ${tt.env} = tt.version; };
            };
          # One app's kernel as the Mac links it, from Linux: its
          # `<Framework>.xcframework` of the `targets`' static libraries
          # (laid out by hand, as xcodebuild lays one out, which is only
          # files) and UniFFI's Swift, read from the library built for this
          # machine.
          appleKernel = { name, crate, so, framework, targets, ... }:
            let llvm = pkgs.llvmPackages.bintools-unwrapped; in
            pkgs.runCommand "${name}-ios-kernel-${lib.concatMapStringsSep "-" (t: iosSlice.${t}.id) targets}" { } ''
              ${ffiBindings { lib = ffiHostLib { inherit crate so; features = [ ]; }; language = "swift"; }}
              mkdir -p headers
              cp $TMPDIR/bindings/swift/*.h headers/
              cat $TMPDIR/bindings/swift/*.modulemap > headers/module.modulemap
              x=$out/${framework}.xcframework
              # The archive as Apple's linker reads it. rustc on Linux writes
              # the static library in GNU's format (a `//` name table), and
              # copies ring's own symbol table in as a member (`__.SYMDEF`):
              # ld64 refuses both ("archive member '//' not a mach-o file",
              # app-screens 42897443933). So its members are taken out and
              # put in again in Darwin's format, with a symbol table of its
              # own; two members of one name would be lost, and stop it.
              darwin() {
                local from=$1 to=$2 dir
                dir=$(mktemp -d)
                names=$(${llvm}/bin/llvm-ar t "$from" | grep -v '^__\.SYMDEF')
                if [ -n "$(sort <<< "$names" | uniq -d)" ]; then echo "$from: members of one name" >&2; exit 1; fi
                (cd "$dir" && ${llvm}/bin/llvm-ar x "$from")
                (cd "$dir" && ${llvm}/bin/llvm-ar --format=darwin rcs "$to" $names)
                rm -rf "$dir"
              }
              ${lib.concatMapStrings (t: ''
                mkdir -p $x/${iosSlice.${t}.id}
                darwin ${(appleLib { inherit crate; target = t; }).lib}/lib/lib${so}*.a $x/${iosSlice.${t}.id}/lib${so}.a
                cp -r headers $x/${iosSlice.${t}.id}/Headers
              '') targets}
              slice() {
                echo "<dict><key>BinaryPath</key><string>lib${so}.a</string><key>HeadersPath</key><string>Headers</string>"
                echo "<key>LibraryIdentifier</key><string>$1</string><key>LibraryPath</key><string>lib${so}.a</string>"
                echo "<key>SupportedArchitectures</key><array><string>arm64</string></array><key>SupportedPlatform</key><string>$3</string>"
                [ -z "$2" ] || echo "<key>SupportedPlatformVariant</key><string>$2</string>"
                echo "</dict>"
              }
              {
                echo '<?xml version="1.0" encoding="UTF-8"?>'
                echo '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
                echo '<plist version="1.0"><dict><key>AvailableLibraries</key><array>'
                ${lib.concatMapStrings (t: ''
                  slice ${iosSlice.${t}.id} "${iosSlice.${t}.variant}" ${iosSlice.${t}.platform}
                '') targets}
                echo '</array><key>CFBundlePackageType</key><string>XFWK</string>'
                echo '<key>XCFrameworkFormatVersion</key><string>1.0</string></dict></plist>'
              } > $x/Info.plist
              mkdir -p $out/swift
              cp $TMPDIR/bindings/swift/*.swift $out/swift/
              cp ${./rust/ark-ffi/swift/Typed.swift} $out/swift/Typed.swift
            '';

          # **The iPhone apps, built by nix** (what app-screens.yaml takes
          # their iOS screenshots from: .github/screens/apps.md).
          #
          # Xcode is never in the store: Apple's licence keeps it off any
          # cache, and a store path would be pushed to (and fetched from)
          # Cachix like any other. The builds use the runner's own, installed
          # at `xcode.app`, and are pinned to it by `xcode` below: a small
          # derivation of its version and build number, an input of each
          # build, so a different Xcode is a different build, and the same
          # one the same hash on every runner. Each build checks the Xcode it
          # finds is that one (its version.plist) and stops if it is not; an
          # unchanged app with the same Xcode is the cache's, and Xcode need
          # not even be on the machine.
          #
          # Most of it is built under nix's strict sandbox: the kernel, its
          # C and Rust compiled by the pinned Xcode's toolchain for the
          # simulator and the phone. The app's own `xcodebuild` cannot be:
          # Xcode's file watcher (DVTFilePathEventWatcher) asks
          # DiskArbitration and other system services for what it watches,
          # which nix's macOS sandbox lets no build reach — its list of
          # services is nix's own, sandbox paths only add paths, and a
          # derivation's `__sandboxProfile` is not applied to a sandboxed
          # build — and it falls (SIGSEGV) when refused. So that one step is
          # `__noChroot`, which needs `sandbox = relaxed` on the builder,
          # where every other build is still sandboxed. All of this was found
          # on GitHub's macos-15 runners, step by step, on the branch
          # claude/sleepy-faraday-lcbow5-xcode (its xcode-strict.yaml).
          # Every app's id, Android's (its applicationId) and the iPhone's (its
          # bundle id), is this and its name.
          bundlePrefix = "us.koon";
          # The pin: Xcode 26.3 (17C529), where GitHub's macos-15 image
          # installs it. Only these strings are in the store (`pin`, a few
          # bytes of text, named so that no push filter takes it for Xcode);
          # `check` is the shell each build runs first, with DEVELOPER_DIR
          # set, and ../.github/xcode-pinned.sh checks the runner the same way
          # before anything is built. To move it, change `version` and
          # `build` (`xcodebuild -version` prints both).
          xcode = let
            version = "26.3";
            build = "17C529";
            app = "/Applications/Xcode_${version}.app";
            pin = pkgs.writeText "xcode-pin-${version}-${build}" ''
              app=${app}
              version=${version}
              build=${build}
            '';
          in {
            inherit version build app pin;
            developer = "${app}/Contents/Developer";
            # The build number of the Xcode at DEVELOPER_DIR, from its
            # version.plist, held to the pin's.
            check = ''
              . ${pin}
              have=$(grep -A1 '<key>ProductBuildVersion</key>' "$DEVELOPER_DIR/../version.plist" 2>/dev/null | sed -n 's|.*<string>\(.*\)</string>.*|\1|p' || true)
              if [ "$have" != "$build" ]; then
                echo "error: $DEVELOPER_DIR is Xcode build '$have', not $version ($build), which flake.nix pins (xcode)" >&2
                exit 1
              fi
            '';
          };
          # One app's kernel: of `targets`, both slices unless told. Built on
          # x86_64 Linux (`appleKernel`) on every system, the Mac's too, so
          # a Mac fetches it from the cache (app-screens.yaml and ios.yaml
          # build it on Linux first) and only links it, with Xcode, into
          # the app. It needs no Xcode, and a new Xcode builds no kernel.
          iosKernel = { ffi, framework, targets ? iosTargets }:
            outputs.x86_64-linux.appleKernel (ffi // { inherit framework targets; });
          # **ArkUI for SwiftUI, compiled once** per Apple platform (`kind`, as
          # `iosApp`'s): ui/swift's two modules, ArkUICore and then ArkUI
          # over it, each a static library and its binary .swiftmodule, by
          # the pinned Xcode's swiftc. Every app of that platform links it
          # (`iosApp`) rather than compiling the package from source in its
          # own xcodebuild, so it is compiled once a run, not once an app,
          # and only ui/swift moves it: an edit to an app rebuilds that app
          # alone. Whole-module and -O, as Release builds it; Swift 5's
          # mode, as the package's tools version (5.9) gives it. A binary
          # module is read only by the compiler that wrote it, which the pin
          # (`xcode`) holds every build to.
          #
          # Xcode's compiler built it, so it is named for Xcode: the Mac's
          # push filter (`[Xx]code([^Gg]|$)`) keeps it off the public cache,
          # and each app copies it into its own build directory before
          # linking, so no app's output refers to it. `__noChroot` as the
          # app's own build, for xcrun and the SDKs.
          arkuiSwift = kind:
            let
              p = {
                simulator = { sdk = "iphonesimulator"; triple = "arm64-apple-ios26.0-simulator"; };
                archive = { sdk = "iphoneos"; triple = "arm64-apple-ios26.0"; };
                macos = { sdk = "macosx"; triple = "arm64-apple-macos26.0"; };
              }.${kind};
              dev = xcode.developer;
            in
            pkgs.stdenv.mkDerivation {
              name = "arkui-swift-${kind}-xcode${xcode.version}";
              src = only [ "ui/swift" ];
              __noChroot = true;
              dontConfigure = true;
              buildPhase = ''
                runHook preBuild
                export DEVELOPER_DIR=${dev}
                ${xcode.check}
                export HOME=$TMPDIR/home CFFIXED_USER_HOME=$TMPDIR/home
                mkdir -p "$HOME"
                unset SDKROOT MACOSX_DEPLOYMENT_TARGET CC CXX LD AR AS RANLIB STRIP NM
                for v in $(env | sed -n 's/^\(NIX_[A-Za-z0-9_]*\)=.*/\1/p'); do
                  case $v in NIX_BUILD_TOP|NIX_BUILD_CORES|NIX_STORE) ;; *) unset "$v" ;; esac
                done
                sdk=$(/usr/bin/xcrun --sdk ${p.sdk} --show-sdk-path)
                mkdir -p out/lib out/modules
                for m in ArkUICore ArkUI; do
                  start=$(date +%s)
                  /usr/bin/xcrun --sdk ${p.sdk} swiftc \
                    -target ${p.triple} -sdk "$sdk" -swift-version 5 \
                    -O -whole-module-optimization -num-threads "''${NIX_BUILD_CORES:-1}" \
                    -parse-as-library -Xfrontend -no-serialize-debugging-options \
                    -module-cache-path "$TMPDIR/module-cache" \
                    -module-name $m -I out/modules \
                    -emit-module -emit-module-path out/modules/$m.swiftmodule \
                    -emit-library -static -o out/lib/lib$m.a \
                    ui/swift/Sources/$m/*.swift
                  echo "$m (${kind}): compiled in $(( $(date +%s) - start ))s"
                done
                runHook postBuild
              '';
              installPhase = "cp -R out $out";
              dontStrip = true;
              dontFixup = true;
              meta.platforms = [ "aarch64-darwin" ];
            };
          arkui-swift-simulator = arkuiSwift "simulator";
          arkui-swift-archive = arkuiSwift "archive";
          arkui-swift-macos = arkuiSwift "macos";
          # One app, as Release: `name` its directory (<name>/ios, its
          # XcodeGen project), `App` its scheme and product, `ffi` and
          # `framework` its kernel's; `kind` "simulator" (the .app, for its
          # screenshots), "archive" (the unsigned .xcarchive ios.yaml signs
          # and sends to TestFlight, giving it its build number there) or
          # "macos" (the same target's Mac destination, the .app, for the
          # Mac's screenshots: `<name>-macos`). Its
          # bundle id is `bundlePrefix`.<name>, every app's; its version
          # project.yml's.
          # An Ark app (`ark`) builds its generated project
          # (`<name>/.native/ios`) over its own Swift (`<name>/native/ios`).
          iosApp = { name, App, ffi, framework, kind, ark ? null }:
            let
              bundle = "${bundlePrefix}.${name}";
              # Only the slice it links: the simulator's for the screenshots'
              # app, the phone's for the archive.
              kernel = iosKernel {
                inherit ffi framework;
                targets = { simulator = [ "aarch64-apple-ios-sim" ]; archive = [ "aarch64-apple-ios" ]; macos = [ "aarch64-apple-darwin" ]; }.${kind};
              };
              dev = xcode.developer;
              # ArkUI, as this platform's apps all link it (`arkuiSwift`).
              arkui = arkuiSwift kind;
              # The device modules the app lists (its app.toml's `modules`),
              # each its swift/ alone, and its own (<name>/modules); none
              # for an app with no app.toml. They are compiled into the app:
              # each implements a protocol UniFFI writes into the app's own
              # bindings, so none can be compiled ahead of it.
              modules = lib.optionals (ark != null)
                (map (m: "modules/${m}/swift") (ark.a.modules or [ ]) ++ [ "${name}/modules" ]);
              common = ''
                -project ${App}.xcodeproj -scheme ${App} -configuration Release \
                -derivedDataPath $TMPDIR/derived \
                -IDEPackageSupportDisableManifestSandbox=1 \
                -IDEPackageSupportDisablePluginExecutionSandbox=1 \
                PRODUCT_BUNDLE_IDENTIFIER=${bundle} \
                CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY= \
                ENABLE_USER_SCRIPT_SANDBOXING=NO \
                COMPILER_INDEX_STORE_ENABLE=NO'';
              build = {
                simulator = ''
                  xcodebuild build ${common} \
                    -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' \
                    ARCHS=arm64 ONLY_ACTIVE_ARCH=YES
                '';
                archive = ''
                  xcodebuild archive ${common} \
                    -destination 'generic/platform=iOS' \
                    -archivePath $TMPDIR/${App}.xcarchive
                '';
                macos = ''
                  xcodebuild build ${common} \
                    -destination 'generic/platform=macOS' \
                    ARCHS=arm64 ONLY_ACTIVE_ARCH=YES
                '';
              }.${kind};
              install = {
                simulator = "cp -R $TMPDIR/derived/Build/Products/Release-iphonesimulator/${App}.app $out/";
                archive = "cp -R $TMPDIR/${App}.xcarchive $out/";
                macos = "cp -R $TMPDIR/derived/Build/Products/Release/${App}.app $out/";
              }.${kind};
            in
            pkgs.stdenv.mkDerivation {
              pname = if kind == "macos" then "${name}-macos" else "${name}-ios-${kind}";
              version = "0.1.0";
              # The app, and the device modules it compiles in (`modules`):
              # ArkUI is linked prebuilt (`arkui`), so ui/swift is not here.
              src = only (modules ++ (if ark == null then [ "${name}/ios" ] else [ "${name}/native/ios" "${name}/app.toml" ]));
              nativeBuildInputs = [ xcodegen pkgs.yq-go ];
              # xcodebuild, and nothing else here: see above.
              __noChroot = true;
              buildPhase = ''
                runHook preBuild
                # The pinned Xcode, a home of its own, a name for the build
                # user (XcodeGen stops without one), and none of nixpkgs'
                # compiler settings, which Xcode's tools would take for theirs.
                export DEVELOPER_DIR=${dev}
                ${xcode.check}
                export PATH=${dev}/usr/bin:$PATH
                export HOME=$TMPDIR/home CFFIXED_USER_HOME=$TMPDIR/home USER=nixbld LOGNAME=nixbld
                mkdir -p "$HOME/Library/Developer/CoreSimulator/Devices"
                unset SDKROOT MACOSX_DEPLOYMENT_TARGET CC CXX LD AR AS RANLIB STRIP NM
                for v in $(env | sed -n 's/^\(NIX_[A-Za-z0-9_]*\)=.*/\1/p'); do
                  case $v in NIX_BUILD_TOP|NIX_BUILD_CORES|NIX_STORE) ;; *) unset "$v" ;; esac
                done
                ${if ark == null then ''
                  mkdir -p ${name}/ffi/ios
                  cp -r ${kernel}/${framework}.xcframework ${kernel}/swift ${name}/ffi/ios/
                  chmod -R u+w ${name}/ffi/ios
                  cd ${name}/ios
                '' else ''
                  # The generated project, and the kernel where it looks (../ffi/ios).
                  mkdir -p ${name}/.native/ffi/ios
                  cp -r ${ark.native}/${name}/.native/. ${name}/.native/
                  cp -r ${kernel}/${framework}.xcframework ${kernel}/swift ${name}/.native/ffi/ios/
                  chmod -R u+w ${name}/.native
                  cd ${name}/.native/ios
                ''}
                # ArkUI linked as built for this platform (`arkui`), not its
                # package compiled again: the package and its product taken out
                # of the project, and the library and its modules given to each
                # target that had it. Copied here first, so the app refers to
                # nothing of it in the store.
                cp -R ${arkui} $TMPDIR/arkui
                chmod -R u+w $TMPDIR/arkui
                ARKUI=$TMPDIR/arkui yq -i '
                  del(.packages.ArkUI)
                  | (.targets[] | select((.dependencies // []) | any_c(.package == "ArkUI"))) |= (
                      .dependencies |= map(select(.package != "ArkUI"))
                      | .settings.base.SWIFT_INCLUDE_PATHS = "$(inherited) " + strenv(ARKUI) + "/modules"
                      | .settings.base.LIBRARY_SEARCH_PATHS = "$(inherited) " + strenv(ARKUI) + "/lib"
                      | .settings.base.OTHER_LDFLAGS = "$(inherited) -lArkUI -lArkUICore")' project.yml
                xcodegen generate
                # SwiftPM sandboxes the manifests and plugins it builds, which
                # cannot be done inside nix's: told not to.
                ${build}
                runHook postBuild
              '';
              installPhase = ''
                mkdir -p $out
                ${install}
              '';
              dontStrip = true;
              dontFixup = true;
              passthru = { inherit bundle arkui; };
              meta.platforms = [ "aarch64-darwin" ];
            };
          # money's generated project is Linux's (`arkOf` there): what the
          # generator writes is text, the same on any machine, and a Mac
          # would otherwise compile the generator's Rust (money-gen and
          # its crates) itself, which nothing builds ahead on Linux.
          moneyIos = { name = "money"; App = "Money"; ffi = moneyFfi; framework = "MoneyFFI"; ark = outputs.x86_64-linux.arkOf.money; };
          harkenIos = { name = "harken"; App = "Harken"; ffi = harkenFfi; framework = "HarkenFFI"; };
          # Both slices; and the simulator's alone, which the tests link
          # (ios.yaml) and the screenshots' app is built over: the same
          # derivation, so built once.
          money-ios-kernel = iosKernel { inherit (moneyIos) ffi framework; };
          # `nix run .#money-prebuild`: money's projects into money/.native,
          # with its Android library linked beside them (and on a Mac its
          # kernel for the simulator and the Mac), for Xcode and Android Studio.
          money-prebuild = arkOf.money.prebuild ({ android = money-android-lib; }
            // lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin { ios = iosKernel { inherit (moneyIos) ffi framework; targets = [ "aarch64-apple-ios-sim" "aarch64-apple-darwin" ]; }; });
          harken-ios-kernel = iosKernel { inherit (harkenIos) ffi framework; };
          money-ios-kernel-simulator = iosKernel { inherit (moneyIos) ffi framework; targets = [ "aarch64-apple-ios-sim" ]; };
          harken-ios-kernel-simulator = iosKernel { inherit (harkenIos) ffi framework; targets = [ "aarch64-apple-ios-sim" ]; };
          money-ios-simulator = iosApp (moneyIos // { kind = "simulator"; });
          harken-ios-simulator = iosApp (harkenIos // { kind = "simulator"; });
          money-ios-archive = iosApp (moneyIos // { kind = "archive"; });
          # The same target's Mac destination, built by nix for the Mac's
          # screenshots (the shell's sidebar, toolbar, inspector, Settings).
          money-macos = iosApp (moneyIos // { kind = "macos"; });
          harken-ios-archive = iosApp (harkenIos // { kind = "archive"; });

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
            unified = if m ? features then demoUnified else checksUnified;
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
            unified = if m ? features then demoUnified else checksUnified;
          }) [ false true ];
          # The checks' crates resolved as one `cargo test --workspace`
          # resolves them: every member's tests and libraries at once, each
          # crate built with the union of what they ask of it, and so built
          # once for all of them. Without it each test and lint build
          # resolved its own graph, and a crate reached with different
          # features was a build of its own, with everything over it: ark-peer
          # was thirteen builds, and a change to it thirteen recompiles of it
          # and of every crate above each. The demos are resolved with the
          # demo feature beside them, which moves only the crates it reaches.
          workspaceRoots = ms: lib.concatMap (m: map (tests: { inherit (m) packageId; features = m.features or [ "default" ]; inherit tests; }) [ false true ]) ms;
          checksUnified = rustCrates.unify { inherit pkgs; roots = workspaceRoots members; };
          demoUnified = rustCrates.unify { inherit pkgs; roots = workspaceRoots (members ++ demos); };
          # Every `.rs` under `dir` formatted as the rustfmt.toml nearest it says.
          rustfmtCheck = name: dir: pkgs.runCommand "check-${name}-fmt" { nativeBuildInputs = [ rust ]; } ''
            cd ${dir}
            find . -name '*.rs' -print0 | xargs -0 rustfmt --check --edition 2021
            touch $out
          '';
          # Derivations as one, for a check that is many.
          allOf = name: drvs: (pkgs.linkFarm name (lib.imap0 (i: d: { name = "${toString i}-${d.name}"; path = d; }) drvs))
            // { roots = lib.concatMap rootsOf drvs; };
          # The crate builds a check is made of at its top (each test build,
          # each lint-clean build), whose dependencies are what checks share.
          rootsOf = d: d.roots or (if d ? completeDeps then [ d ] else [ ]);
          # Every crate the named checks' top builds depend on, once: what
          # build.yaml's `crates` job builds and pushes before the checks
          # fan out, so that two checks never compile the same crate at once.
          checksCrates = names:
            let
              roots = lib.concatMap (n: rootsOf self.checks.${system}.${n}) names;
              deps = lib.concatMap (r: r.completeDeps ++ r.completeBuildDeps) roots;
              key = d: builtins.unsafeDiscardStringContext (baseNameOf d.drvPath);
            in
            allOf "checks-crates" (lib.attrValues (lib.listToAttrs (map (d: lib.nameValuePair (key d) d) deps)));

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
          # `nix run .#update-theme`, from the repository's root: the colours
          # every app is drawn in, written from ui/theme.toml into the
          # Kotlin, Swift and Rust each platform's arkui reads (ui/README.md).
          update-theme = pkgs.writeShellScriptBin "update-theme" ''
            ${pkgs.python3}/bin/python3 ui/theme.py write
          '';
          # The catalogue as its repository builds it, and the copy the demo
          # embeds made from it.
          classical = pkgs.writeText "catalogue.json" (import "${inputs.classical}/catalogue.nix");
          update-catalogue = pkgs.writeShellScriptBin "update-catalogue" ''
            cp ${classical} harken/seed/catalogue.json
            chmod 644 harken/seed/catalogue.json
            echo "harken/seed/catalogue.json is the catalogue at ${inputs.classical.shortRev or "?"}"
          '';
        in
        {
          # For the Mac's apps (`iosKernel`): every Apple kernel is Linux's;
          # and so is every Ark app's generated project (`moneyIos`).
          inherit appleKernel arkOf;
          packages = {
            inherit ark vectors fuzz harken-domain trace-corpus harken-android-lib harken-ios-xcframework;
            # The iPhone apps from the pinned Xcode: for the simulator (the
            # screenshots), as the archive TestFlight is sent, and the kernel.
            inherit money-ios-simulator harken-ios-simulator money-ios-archive harken-ios-archive money-macos;
            inherit money-ios-kernel harken-ios-kernel money-ios-kernel-simulator harken-ios-kernel-simulator;
            # ArkUI for SwiftUI, compiled once per platform for every app.
            inherit arkui-swift-simulator arkui-swift-archive arkui-swift-macos;
            inherit harken-server arkdb;
            # `nix run .#update-catalogue`, from the repository's root: the
            # demo's copy of the catalogue made from the `classical` input.
            inherit classical update-catalogue update-cargo-nix update-covers;
            # `nix run .#update-theme`: the theme's generated files, from
            # ui/theme.toml.
            inherit update-theme;
            # An Ark app's generated pieces (nix/ark-app.nix): its
            # #[view]s as SwiftUI and Compose (`<app>-ui`), its native
            # projects (`<app>-native`), and `nix run .#<app>-prebuild`.
            money-ui = arkOf.money.ui;
            money-native = arkOf.money.native;
            inherit money-prebuild;
            # revelation's: Kotlin alone, its Android project generated.
            revelation-native = arkOf.revelation.native;
            revelation-prebuild = arkOf.revelation.prebuild { };
            # `nix run .#harken-serve [ADDR]`: the dev server, anyone is
            # whoever they say.
            harken-serve = pkgs.callPackage ./harken/server/nix/serve.nix { inherit harken-server; };
            inherit harken-iced docs docs-web screens-ffmpeg screens-python xcodegen;
            inherit money-iced money-web money-server;
            inherit money-seed harken-seed harken-covers money-demo harken-demo;
          } // {
            # money's library for its phones (the generated shim over
            # money/app), as harken's.
            inherit money-android-lib money-ios-xcframework;
            default = ark;
            inherit harken-apk harken-android-tests harken-android-screens;
            harken-apk-deps = harken.deps;
            revelation-apk = revelation.apk;
            revelation-android-tests = revelation.tests;
            revelation-android-screens = revelation.screens;
            revelation-apk-deps = revelation.deps;
            # money's phone, over money-android-lib (money/native/android/README.md).
            inherit money-apk money-android-tests money-android-screens;
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
            # The crates build.yaml's checks share, built once before they
            # fan out (`checksCrates`): of every check its `checks` jobs run.
            checks-crates = checksCrates [
              "fmt" "clippy" "rust" "ffi" "traces" "vectors" "fuzz-smoke" "versions"
              "harken-domain" "harken-module" "docs" "theme" "theme-contrast"
            ];
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
            # money's web client builds for wasm32 and is bound
            # (`.github/screens/app-web.sh` photographs it).
            inherit money-web;
            # And is a real client: signed in through money-server, synced
            # with another device over its socket, offline and back
            # (money/web/nix/sync.nix, in Playwright's Chromium).
            money-web-sync = pkgs.callPackage ./money/web/nix/sync.nix {
              inherit money-web money-seed;
              money-server = money-server-bin;
            };
            # The demo's copy of the catalogue is the `classical` input's:
            # `nix flake update classical` then `nix run .#update-catalogue`.
            catalogue = pkgs.runCommand "catalogue-current" { } ''
              cmp ${classical} ${./harken/seed/catalogue.json} || {
                echo "harken/seed/catalogue.json is not the classical input's: nix run .#update-catalogue" >&2
                exit 1
              }
              touch $out
            '';
            # What is generated from ui/theme.toml is what is committed:
            # edit the tokens, then `nix run .#update-theme`.
            theme = pkgs.runCommand "theme-current" { nativeBuildInputs = [ pkgs.python3 ]; } ''
              python3 ${./ui}/theme.py write generated > /dev/null
              status=0
              for f in $(python3 ${./ui}/theme.py files); do
                cmp "generated/$f" "${self}/$f" || {
                  echo "$f is not what ui/theme.toml makes: nix run .#update-theme" >&2
                  status=1
                }
              done
              if [ $status != 0 ]; then exit 1; fi
              touch $out
            '';
            # Each Ark app's native projects are generated: its generator
            # builds and runs over its views, and ui/prebuild.py writes its
            # projects (nix/ark-app.nix). The emitters' own snapshots are
            # ark-ui-gen's tests (checks.rust).
            money-native = arkOf.money.native;
            revelation-native = arkOf.revelation.native;
            # Every text colour the tokens define is legible on every ground
            # it is drawn on, at WCAG's 4.5:1 (3:1 for large text, icons and
            # a control's edge), the player's wash among them.
            # What an app opts into is what it builds with (ui/docs/app.md,
            # The Device): its app.toml says what it must, each module it
            # lists exists, and its crate depends on each listed module's.
            prebuild = pkgs.runCommand "prebuild-check" { nativeBuildInputs = [ pkgs.python3 ]; } ''
              # The modules, their ports (ark-module's src), and each app's
              # crates that name them (its bridge's, when it has one).
              cd ${only ([ "ui/prebuild.py" "ui/theme.py" "ui/theme.toml" "modules" "ui/rust/module/src" ]
                ++ lib.filter (p: builtins.pathExists (./. + "/${p}"))
                  (lib.concatMap (d: [ "${d}/app.toml" "${d}/app/Cargo.toml" "${d}/bridge/Cargo.toml" "${d}/icon.svg" ]) arkApps))}
              python3 ui/prebuild.py check
              touch $out
            '';
            # The layers held (layers.toml, ui/docs/app.md "Layers"): every
            # workspace member in one layer, and no dependency a layer may
            # not use but those layers.toml's [allow] lists until fixed —
            # read from `cargo metadata`, so it holds whatever builds them.
            layers = pkgs.runCommand "layers" { nativeBuildInputs = [ rust pkgs.python3 ]; } ''
              cd ${only (rustDirs ++ [ "layers.toml" "tools/layers.py" ])}/rust
              export CARGO_HOME=$TMPDIR/cargo
              cargo metadata --no-deps --offline --format-version 1 | python3 ../tools/layers.py ../layers.toml
              touch $out
            '';
            theme-contrast = pkgs.runCommand "theme-contrast" { nativeBuildInputs = [ pkgs.python3 ]; } ''
              python3 ${./ui}/theme.py contrast
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
                unified = null;
                crates = [ { packageId = "docs"; member = "docs/site"; } ];
                dirs = [ "docs/site" "ui/rust/arkui" "ui/rust/glyphs" "ui/rust/web" "rust/Cargo.toml" ];
              })
            ] ++ memberClippy (args: docsCrate (removeAttrs args [ "unified" ])) { packageId = "docs"; });
            # The workspace formatted as each crate's rustfmt.toml says.
            fmt = rustfmtCheck "arkdb" (only (rustDirs ++ [ "ui/rustfmt.toml" ]));
            # harken's domain crate builds, and its schema's hash is the one
            # pinned (harken/domain/tests/port.rs).
            inherit harken-domain;
            # The bridge (§1.16, N3): ark-ffi, harken/ffi and money/bridge build
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
                  { packageId = "money-bridge"; member = "money/bridge"; features = [ "default" "page" ]; }
                ];
                dirs = rustDirs ++ genDirs;
                postCheck = ''
                  ${bindings harkenFfi "$TMPDIR/bindings"}
                  grep -q 'fun `harkenOpen`' "$TMPDIR/bindings/kotlin/dev/harken/ffi/harken_ffi.kt"
                  grep -q "class Peer" "$TMPDIR/bindings/kotlin/dev/arkdb/ffi/ark_ffi.kt"
                  grep -q "func harkenOpen" "$TMPDIR/bindings/swift/harken_ffi.swift"
                  ${bindings moneyFfi "$TMPDIR/money"}
                  grep -q 'fun `moneyOpen`' "$TMPDIR/money/kotlin/uniffi/money_bridge/money_bridge.kt"
                  grep -q "class MoneyChange" "$TMPDIR/money/kotlin/uniffi/money_bridge/money_bridge.kt"
                  grep -q "func moneyDemo" "$TMPDIR/money/swift/money_bridge.swift"
                  # Every client's formatting, linked into each library.
                  grep -q "func formatAmount" "$TMPDIR/money/swift/ark_format.swift"
                  grep -q "func formatLength" "$TMPDIR/bindings/swift/ark_format.swift"
                  grep -q 'fun `formatLength`' "$TMPDIR/bindings/kotlin/uniffi/ark_format/ark_format.kt"
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
              src = only [ "harken/ios" "rust/ark-ffi/swift" "ui/swift" ];
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
            # The same for money's iOS app (money/native/ios/typecheck.sh): its
            # budget arithmetic, first run, session and model with the generated
            # types, UniFFI's Swift for money's library and ArkUICore.
            money-ios-typecheck = pkgs.swiftPackages.stdenv.mkDerivation {
              name = "money-ios-typecheck";
              src = only [ "money/native/ios" "money/app.toml" "modules" "rust/ark-ffi/swift" "ui/swift" ];
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
                  bash money/native/ios/typecheck.sh bindings
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

          # Everything the workflows publish, as data, and the one list of
          # the apps: a page goes up at k2on.github.io/apps/<name>/, an APK
          # into the F-Droid repository (and onto a tag's release), an
          # iPhone app to TestFlight (ios.yaml); and each app with
          # <name>/screens/shots.json is photographed on the phones and the
          # desktop it is listed for (app-screens.yaml). Adding one is adding it here; the
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
                tests = "${name}-android-tests";
                file = "${name}.apk";
                key = key (app.apk.overrideAttrs (_: { versionFlags = [ ]; }));
              };
              # An iPhone app (ios.yaml builds and sends each; app-screens.yaml
              # photographs each): `App` its scheme and product; whether it goes
              # to TestFlight (its App Store Connect record must exist), and
              # whether its tests run. Its key is its archive's: the archive
              # has no build number in it (ios.yaml gives it one), so it moves
              # only with what the app is built from.
              iphone = name: App: { testflight ? true, test ? false }:
                let archive = self.packages.aarch64-darwin."${name}-ios-archive"; in {
                  inherit name App testflight test;
                  inherit (archive) bundle;
                  package = "${name}-ios-archive";
                  key = key archive;
                };
            in
            {
              pages = [
                (page "docs" "docs" "docs" "the components every app here shares (arkui), a page each in an ArkUI app written as static HTML, with live arkui-web parts, pictures drawn by arkui itself, and catalogue.json beside it.")
              ];
              apks = [
                # The kernel for every ABI first, so its failure reads on its own.
                (apk "harken" harken { first = [ "harken-android-lib" ]; })
                (apk "revelation" revelation { })
                # money's kernel for every ABI first, as harken's.
                (apk "money" money { first = [ "money-android-lib" ]; })
              ];
              ios = [
                (iphone "harken" "Harken" { })
                (iphone "money" "Money" { test = true; })
              ];
              # The desktop windows (`<name>-iced`), published nowhere yet but
              # photographed beside the phones (app-screens.yaml), each built
              # by nix at the commit.
              desktop = map (name: { inherit name; package = "${name}-iced"; key = key self.packages.${pkgs.system}."${name}-iced"; }) [ "harken" "money" ];
            };

          # A laptop's cargo builds four crates at a time, not one a core:
          # iced, wgpu and tract each take a gigabyte or more to compile and
          # link, and a dozen at once ran a 16 GB laptop out of memory (the
          # tests themselves stay under 100 MB). `-j` on the command line
          # still says otherwise.
          #
          # On Linux a shell also has the desktop window's library path
          # (nix/gpu-env.sh, as the launcher does), so `cargo run -p
          # money-iced` reaches the GPU as `nix run .#money-iced` does.
          devShells = let
            cargoJobs = { CARGO_BUILD_JOBS = "4"; }
              // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux { shellHook = "source ${gpuEnv}"; };
          in {
            rust = pkgs.mkShell ({
              packages = [ (rust.override { extensions = [ "rust-src" "rust-analyzer" ]; }) pkgs.pkg-config ];
            } // cargoJobs);
            default = pkgs.mkShell ({
              packages = [ rust pkgs.pkg-config ];
            } // cargoJobs);
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
