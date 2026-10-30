# Every Rust build of the workspace, a derivation per crate: nixpkgs'
# buildRustCrate over the crates rust/Cargo.nix lists (crate2nix writes it
# from the lockfile: `nix run .#update-cargo-nix`). A crate's derivation is
# its source, its features, its target and its dependencies' derivations, and
# nothing of the rest of the graph, so a lockfile that moves one crate builds
# that crate and what depends on it, and every other comes from the cache.
#
# rust/Cargo.nix is used for its crates (each one's source, features and
# dependencies) and crate2nix's pure helpers; the resolution and the wiring
# are here, because crate2nix resolves features once, for the target, and
# uses that for the build machine's crates too: a crate that a build script
# or a proc macro needs as well as the target (rustix, say) is then compiled
# for the build machine with the target's dependencies and features, which
# a cross build does not survive. Here they are two graphs, as cargo has
# them: what the target links, and what runs on the build machine (build
# scripts' dependencies and proc macros), each with its own features. On a
# native build the two are one, as crate2nix has them, so a crate needed by
# both is compiled once with the union of their features.
{ lib, cargoNix }:
let
  inherit (cargoNix.internal)
    crates expandFeatures enableFeatures dependencyFeatures filterEnabledDependencies sortedUnique;

  # crate2nix's makeDefaultTarget, which a dependency's `cfg(...)` is
  # evaluated against, with the os and env rustc has for Android, wasm and
  # Apple (crate2nix says `linux` and `gnu` for them), and the target's name
  # for a dependency on one target (`[target.<triple>.dependencies]`).
  targetOf = platform: {
    name = platform.rust.rustcTarget;
    unix = platform.isUnix;
    windows = platform.isWindows;
    fuchsia = true;
    test = false;
    inherit (platform.rust.platform) arch vendor;
    os = if platform.isAndroid then "android" else platform.rust.platform.os;
    family = platform.rust.platform.target-family;
    env =
      if platform.isAndroid || platform.isWasm || platform.isDarwin then ""
      else if platform.isMusl then "musl"
      else "gnu";
    endian = if platform.parsed.cpu.significantByte.name == "littleEndian" then "little" else "big";
    pointer_width = toString platform.parsed.cpu.bits;
    debug_assertions = false;
  };

  # buildRustCrate for one package set, with this toolchain, and the parts of
  # cargo's environment it lacks: `CARGO_CRATE_NAME` (UniFFI's macros read
  # it), `CARGO_ENCODED_RUSTFLAGS`, and every `CARGO_CFG_*` as rustc has them
  # for the target (buildRustCrate says `unix` and `gnu` whatever the target,
  # and winit's and wgpu's build scripts choose their backends by them).
  builderFor = { rust, overrides }: pkgs:
    let
      base = pkgs.buildRustCrate.override {
        rustc = rust;
        cargo = rust;
        defaultCrateOverrides = pkgs.defaultCrateOverrides // overrides;
      };
      cfg = ''
        export CARGO_ENCODED_RUSTFLAGS=""
        unset $(env | sed -n 's/^\(CARGO_CFG_[A-Z0-9_]*\)=.*/\1/p')
        eval "$(${rust}/bin/rustc --print cfg --target "$TARGET" | awk -F= '
          $1 == "debug_assertions" { next }
          { k = "CARGO_CFG_" toupper($1); v = $2; gsub(/"/, "", v)
            if (k in m) m[k] = m[k] "," v; else { m[k] = v; o[++n] = k } }
          END { for (i = 1; i <= n; i++) printf "export %s=\"%s\"\n", o[i], m[o[i]] }')"
      '';
    in
    args: (base args).overrideAttrs (old: {
      CARGO_CRATE_NAME = lib.replaceStrings [ "-" ] [ "_" ] (args.libName or args.crateName);
      configurePhase = lib.replaceStrings [ "export HOST=" ] [ "${cfg}\nexport HOST=" ] old.configurePhase;
      # rustc names a wasm binary `<name>.wasm`, and buildRustCrate renames
      # a binary whose name has a dash by its name without one.
      preBuild = (old.preBuild or "") + lib.optionalString pkgs.stdenv.hostPlatform.isWasm ''
        mv() { if [ ! -e "$1" ] && [ -e "$1.wasm" ]; then command mv "$1.wasm" "$2.wasm"; else command mv "$@"; fi; }
      '';
    });

  # The features of every crate the root reaches, by graph ("target" or
  # "build"): cargo's resolution, a crate's features the union of what each
  # of its dependents in that graph asks of it.
  resolve = { packageId, features, tests, split, targets }:
    let
      visit = cache: graph: id: wanted:
        let
          config = crates.${id};
          key = "${graph}/${id}";
          have = cache.${key} or { };
          # What it had and what is asked now, expanded together: a weak
          # feature (`dep?/f`) asked now acts on a dependency enabled before.
          enabled = enableFeatures (config.dependencies or [ ])
            (expandFeatures (config.features or { }) (builtins.attrNames have ++ wanted));
          now = lib.genAttrs enabled (_: true);
          root = id == packageId;
          deps = filterEnabledDependencies {
            dependencies = (config.dependencies or [ ]) ++ lib.optionals (tests && root) (config.devDependencies or [ ]);
            features = enabled;
            target = targets.${graph} // { test = tests && root; };
          };
          buildDeps = filterEnabledDependencies {
            dependencies = config.buildDependencies or [ ];
            features = enabled;
            target = targets.${graph};
          };
          graphOf = dep: if split && (crates.${dep.packageId}.procMacro or false) then "build" else graph;
          step = g: c: dep: visit c g dep.packageId (dependencyFeatures enabled dep);
        in
        if cache ? ${key} && have == now then cache
        else
          lib.foldl (step (if split then "build" else graph))
            (lib.foldl (c: dep: step (graphOf dep) c dep) (cache // { ${key} = now; }) deps)
            buildDeps;
    in
    visit { } "target" packageId features;

  # The root's derivation, and its dependencies' under it.
  build =
    { pkgs
    , rust
    , packageId
    , features ? [ "default" ]
      # The root built with its dev-dependencies and its tests
      # (buildRustCrate's `buildTests`): every test target, not run.
    , tests ? false
      # By crate name, as buildRustCrate's crateOverrides.
    , overrides ? { }
      # Applied to the root's buildRustCrate arguments, and then to its
      # derivation (`overrideAttrs`).
    , root ? { }
    , rootAttrs ? (_: { })
    }:
    let
      split = pkgs.stdenv.buildPlatform != pkgs.stdenv.hostPlatform;
      targets = {
        target = targetOf pkgs.stdenv.hostPlatform;
        build = targetOf pkgs.stdenv.buildPlatform;
      };
      resolved = resolve { inherit packageId features tests split targets; };
      builders = {
        target = builderFor { inherit rust overrides; } pkgs;
        build = builderFor { inherit rust overrides; } pkgs.buildPackages;
      };
      fetch = p: config: p.fetchurl {
        name = "${config.crateName}-${config.version}.tar.gz";
        url = "https://static.crates.io/crates/${config.crateName}/${config.crateName}-${config.version}.crate";
        inherit (config) sha256;
      };
      pkgsOf = { target = pkgs; build = pkgs.buildPackages; };
      # Every crate of each graph, lazily: only what the root reaches is
      # built. The root is apart from them: built with its tests, it is not
      # the library a dev-dependency of its own links (ark's tests through
      # a crate over ark), as with cargo.
      built = lib.genAttrs [ "target" "build" ] (graph:
        lib.mapAttrs (id: _: crate graph id false) crates);
      crate = graph: id: isRoot:
        let
          config = removeAttrs crates.${id} [ "resolvedDefaultFeatures" "devDependencies" ];
          features = builtins.attrNames (resolved."${graph}/${id}" or { });
          buildGraph = if split then "build" else graph;
          deps = filterEnabledDependencies {
            dependencies = (config.dependencies or [ ]) ++ lib.optionals (tests && isRoot) (crates.${id}.devDependencies or [ ]);
            inherit features;
            target = targets.${graph} // { test = tests && isRoot; };
          };
          buildDeps = filterEnabledDependencies {
            dependencies = config.buildDependencies or [ ];
            inherit features;
            target = targets.${graph};
          };
          drvOf = dep: built.${if crates.${dep.packageId}.procMacro or false then buildGraph else graph}.${dep.packageId};
          crateRenames = lib.mapAttrs
            (_: map (dep: { inherit (dep) rename; inherit (crates.${dep.packageId}) version; }))
            (lib.groupBy (dep: dep.name) (lib.filter (dep: dep ? rename) (buildDeps ++ deps)));
        in
        builders.${graph} (config // {
          src = config.src or (fetch pkgsOf.${graph} config);
          features = sortedUnique features;
          dependencies = map drvOf deps;
          buildDependencies = map (dep: built.${buildGraph}.${dep.packageId}) buildDeps;
          inherit crateRenames;
          release = true;
        } // lib.optionalAttrs isRoot ({ buildTests = tests; } // root));
    in
    (crate "target" packageId true).overrideAttrs rootAttrs;
in
{ inherit build targetOf; }
