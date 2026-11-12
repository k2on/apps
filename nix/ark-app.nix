# An Ark app, from its directory (ui/docs/app.md, "Generated, Not
# Committed"): Expo's Continuous Native Generation, in nix. An app commits
# its app.toml, its icon.svg, its Rust (core/, app/, server/) and what is
# not written in Rust yet (native/); this generates everything native:
#
#   crates      the app's generated crates, in crate2nix's form, for
#               nix/rust.nix to resolve and build beside the workspace's:
#               `<id>-gen`, a main (ui/prebuild.py's `views`, from app.toml's
#               `[screens]` and `[views]`) linking the app's crate with
#               ark-ui-gen's command line, which writes its #[view]s as
#               SwiftUI and Compose.
#   ui          what the generator writes for the app: <Name>.swift, <Name>.kt.
#   native      the app's native projects (ui/prebuild.py): ios/ (XcodeGen's
#               project, the asset catalog, the icon, the entitlements) and
#               android/ (Gradle's project, the manifest, the resources), each
#               with the generated views and the app's entry points over the
#               app library (the `@main`, the Application and the activity,
#               the screens' holders; ui/docs/app.md, "The App Library");
#               laid out under `<app>/.native/`, where a build of the app's
#               source puts them.
#   prebuild    `nix run .#<app>-prebuild`: the same into the working tree,
#               with the phones' libraries linked beside them, for Xcode and
#               Android Studio (Expo's `npx expo prebuild`).
#
# The app's builds (`<app>-apk`, `<app>-ios-simulator`, `<app>-ios-archive`,
# `<app>-macos`) are flake.nix's, over `native`.
{ lib, self }:
let
  read = dir: (builtins.fromTOML (builtins.readFile (self + "/${dir}/app.toml"))) // { inherit dir; };
in
rec {
  inherit read;

  # The generator's main for the app in `dir`: its source, one file,
  # written by ui/prebuild.py (`views`) from app.toml's `[screens]` and
  # `[views]`: each #[view] of the app's crate named with its runtime's
  # holders and the names ark-ui-gen writes it under, the first run's
  # among them. No crate of the app's is committed for it.
  genMain = pkgs: a:
    let
      files = [ "ui/prebuild.py" "ui/theme.py" "ui/theme.toml" "${a.dir}/app.toml" ];
      src = lib.cleanSourceWith {
        src = self;
        filter = path: _:
          let p = lib.removePrefix "${toString self}/" (toString path); in
          lib.elem p files || lib.any (f: lib.hasPrefix "${p}/" f) files;
      };
    in
    pkgs.runCommand "${a.id}-gen-src" { nativeBuildInputs = [ pkgs.python3 ]; } ''
      cd ${src}
      python3 ui/prebuild.py views ${a.dir} --root . --out $out/src/main.rs
    '';

  # The phones' library: a shim that links the app's bridge (app.toml's
  # `[ffi] crate`, an adapter crate in the app's `[ffi] dir`: the UniFFI
  # twins of the core's and the app's types and the exports over them) as a
  # static library (the iPhone, the Mac) and a shared one (Android), every
  # UniFFI export of the bridge and of what it links in it. `lib.rs`, and
  # the Cargo.toml the iPhone's cargo build adds to the workspace at
  # `<dir>/.ffi` (nix/rust.nix builds the same crate from `crates` below).
  # The device modules an app takes on: the crates its app crate
  # (`<dir>/app`) depends on, directly or through its other path
  # dependencies (normal and build, never dev), whose Cargo.toml says
  # `[package.metadata.ark] module` — what ui/prebuild.py reads of `cargo
  # metadata`, read here off the same manifests while nix evaluates. Each
  # by its crate's name, with its module.toml.
  modulesOf = a:
    let
      manifest = dir: builtins.fromTOML (builtins.readFile (self + "/${dir}/Cargo.toml"));
      # `money/app/../../modules/ark-camera` as `modules/ark-camera`.
      normal = p: lib.concatStringsSep "/" (lib.foldl'
        (acc: seg: if seg == ".." then lib.init acc else if seg == "." || seg == "" then acc else acc ++ [ seg ])
        [ ] (lib.splitString "/" p));
      paths = dir:
        let m = manifest dir; in
        lib.concatMap (d: lib.optional (builtins.isAttrs d && d ? path) (normal "${dir}/${d.path}"))
          (builtins.attrValues ((m.dependencies or { }) // (m.build-dependencies or { })));
      reached = builtins.genericClosure {
        startSet = [{ key = "${a.dir}/app"; }];
        operator = item: map (key: { inherit key; }) (paths item.key);
      };
      isModule = dir: ((manifest dir).package.metadata.ark.module or null) != null;
    in
    lib.optionals (a ? crate) (map
      (r: {
        name = (manifest r.key).package.name;
        toml = builtins.fromTOML (builtins.readFile (self + "/${r.key}/${(manifest r.key).package.metadata.ark.module}"));
      })
      (lib.filter (r: isModule r.key) reached));
  # ark-ffi's feature for each module whose device the phone hands the
  # process (module.toml's `[phone]`): `camera` for ark-camera.
  arkFfiFeatures = a: map (m: lib.removePrefix "ark-" m.name) (lib.filter (m: m.toml ? phone) (modulesOf a));

  ffiName = a: "${a.id}-ffi";
  ffiLib = a: "${a.id}_ffi";
  bridge = a: a.ffi.crate;
  ffiFeatures = a: platform: map (f: "${bridge a}/${f}") ((a.ffi or { }).${platform} or [ ]);
  ffiSource = a: ''
    //! Generated by nix/ark-app.nix: ${a.name}'s library for its phones,
    //! every UniFFI export of ${bridge a} and of what it links. Never
    //! committed.
    pub use ${lib.replaceStrings [ "-" ] [ "_" ] (bridge a)}::*;
  '';
  ffiCargo = a: ''
    # Generated by nix/ark-app.nix; never committed.
    [package]
    name = "${ffiName a}"
    version = "0.1.0"
    edition = "2021"
    workspace = "../../rust"

    [lib]
    name = "${ffiLib a}"
    path = "lib.rs"
    crate-type = ["lib", "cdylib", "staticlib"]

    [dependencies]
    ${bridge a} = { path = "../${a.ffi.dir}" }
    # The devices of the modules ${a.crate} depends on, registered by the app library.
    ark-ffi = { path = "../../rust/ark-ffi", features = [${lib.concatMapStringsSep ", " (f: "\"${f}\"") (arkFfiFeatures a)}] }

    [features]
    default = [${lib.concatMapStringsSep ", " (f: "\"${f}\"") (ffiFeatures a "android")}]
  '';

  # The app's generated crates, in crate2nix's form (nix/rust.nix's
  # `extraCrates`).
  # An app of Kotlin alone (no `crate`: revelation) has none.
  crates = pkgs: dir:
    let a = read dir; in lib.optionalAttrs (a ? crate) {
      "${ffiName a}" = {
        crateName = ffiName a;
        version = "0.1.0";
        edition = "2021";
        src = pkgs.writeTextDir "src/lib.rs" (ffiSource a);
        libName = ffiLib a;
        type = [ "lib" "cdylib" "staticlib" ];
        dependencies = [
          { name = bridge a; packageId = bridge a; }
          { name = "ark-ffi"; packageId = "ark-ffi"; features = arkFfiFeatures a; }
        ];
        # Android's are its default; the iPhone's build takes none.
        features = { default = ffiFeatures a "android"; };
      };
      "${a.id}-gen" = {
        crateName = "${a.id}-gen";
        version = "0.1.0";
        edition = "2021";
        src = genMain pkgs a;
        crateBin = [{ name = "${a.id}-gen"; path = "src/main.rs"; requiredFeatures = [ ]; }];
        dependencies = [
          { name = a.crate; packageId = a.crate; }
          { name = "ark-app"; packageId = "ark-app"; }
          { name = "ark-ui-gen"; packageId = "ark-ui-gen"; }
        ];
        features = { };
      };
    };

  # The app's entry points that draw the app's own mark: its desktop
  # (`iced/`) and its browser (`web/`) read its icon.svg as cargo finds it,
  # beside their crate (`include_str!("../../icon.svg")`). Each is built
  # from the app's directory with only that crate, the icon and app.toml in
  # it, the crate its workspace member, rather than from the crate's own
  # directory, which has no icon. When they are generated (ui/docs/app.md)
  # the generator places them so.
  entrySources = crates: dir:
    let
      a = read dir;
      root = "${self}/${dir}";
      icon = a.icon.svg or "icon.svg";
      entry = sub: lib.mapAttrs
        (_: c: c // {
          src = lib.cleanSourceWith {
            src = self + "/${dir}";
            filter = path: _:
              let p = toString path; in
              p == "${root}/${icon}" || p == "${root}/app.toml" || p == "${root}/${sub}" || lib.hasPrefix "${root}/${sub}/" p;
          };
          workspace_member = sub;
        })
        (lib.filterAttrs (_: c: c ? src && toString (c.src.origSrc or c.src) == "${root}/${sub}") crates);
    in
    entry "iced" // entry "web";

  # What else of the app's directory an entry point's build holds beside
  # its crate (`entrySources`): app.toml and the icon, by their paths in
  # the repository; none for a member that is not an Ark app's entry point.
  # For a build that takes the crate's tree by its path (flake.nix's
  # `crateTests`), so the icon is where `include_str!` looks for it.
  entryFiles = apps: member:
    let dir = dirOf member; in
    if lib.elem dir apps && lib.elem (baseNameOf member) [ "iced" "web" ]
    then [ "${dir}/app.toml" "${dir}/${(read dir).icon.svg or "icon.svg"}" ]
    else [ ];

  # The app's packages: `build` is flake.nix's `crate` (a crate of the
  # workspace, or of `crates`, for the host); `only` its source filter.
  packages = { pkgs, build, only, bundlePrefix, cargoMetadata }: dir:
    let
      a = read dir;
      gen = build { packageId = "${a.id}-gen"; };
      # Its screens' SwiftUI and Compose; none for an app with no crate.
      # Every view app.toml names is among them (what money/views' test
      # held when the generator's main was a crate of its own).
      ui = if a ? crate then pkgs.runCommand "${a.id}-ui" { nativeBuildInputs = [ pkgs.python3 ]; } ''
        ${gen}/bin/${a.id}-gen write $out --package ${a.android.package or "dev.${a.id}.android"}
        cd ${only [ "ui/prebuild.py" "ui/theme.py" "ui/theme.toml" "${dir}/app.toml" ]}
        python3 ui/prebuild.py views ${dir} --root . --written $out
      '' else null;
      native = pkgs.runCommand "${a.id}-native" {
        nativeBuildInputs = [ pkgs.python3 pkgs.librsvg ];
        # Of the app's directory, only what ui/prebuild.py's `write` reads:
        # its app.toml, its icon, its native/ (each phone's own code and
        # resources) and its own modules; not its Rust, its server or its
        # seed, an edit to which would otherwise write the same projects
        # again and build every phone's app over them.
        src = only [ "ui/prebuild.py" "ui/theme.py" "ui/theme.toml" "modules"
          "${dir}/app.toml" "${dir}/${a.icon.svg or "icon.svg"}" "${dir}/native" "${dir}/modules" ];
      } ''
        cd $src
        # Its device modules: its crate's dependencies, in the workspace's metadata.
        export ARK_CARGO_METADATA=${cargoMetadata}
        python3 ui/prebuild.py write ${dir} ${lib.optionalString (ui != null) "--views ${ui}"} --bundle-prefix ${bundlePrefix} --out $out/${dir}/.native
      '';
    in
    {
      inherit a gen ui native;
      # From the repository's root: the projects, writable, and the phones'
      # libraries where the projects look for them (`../ffi/<platform>`),
      # when `libs` names them.
      prebuild = libs: pkgs.writeShellApplication {
        name = "${a.id}-prebuild";
        text = ''
          if [ ! -f ${dir}/app.toml ]; then
            echo "${a.id}-prebuild: run from the repository's root (no ${dir}/app.toml here)" >&2
            exit 1
          fi
          rm -rf ${dir}/.native/ios ${dir}/.native/android
          mkdir -p ${dir}/.native/ffi
          cp -r --no-preserve=mode ${native}/${dir}/.native/. ${dir}/.native/
          ${lib.concatStrings (lib.mapAttrsToList (platform: path: ''
            ln -sfn ${path} ${dir}/.native/ffi/${platform}
          '') libs)}
          echo "${dir}/.native: ios/ (xcodegen generate, then open ${a.apple.product or a.name}.xcodeproj) and android/ (open in Android Studio)"
        '';
      };
    };
}
