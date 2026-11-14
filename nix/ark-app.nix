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
#               SwiftUI and Compose; `<id>-ffi`, the phones' library; and
#               `<id>-web`, the browser's page (`webSrc`).
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

  # The phones' library, generated (ui/prebuild.py's `ffi`, from app.toml's
  # `[ffi]` and `[screens]`): `<id>-ffi` (lib and UniFFI namespace
  # `<id>_ffi`), the UniFFI twins of what crosses and the objects the phone
  # holds over the app's crate, run by ark-ffi's `Hosted`, as a static
  # library (the iPhone, the Mac) and a shared one (Android). Its `lib.rs`,
  # and the Cargo.toml the iPhone's cargo build adds to the workspace at
  # `<dir>/.ffi` (nix/rust.nix builds the same crate from `crates` below).
  # Nothing of it is committed.
  ffiSrc = pkgs: cargoMetadata: a:
    let
      members = map (m: normal "rust/${m}") (builtins.fromTOML (builtins.readFile (self + "/rust/Cargo.toml"))).workspace.members;
      files = [ "ui/prebuild.py" "ui/theme.py" "ui/theme.toml" "${a.dir}/app.toml" "rust/Cargo.toml" ] ++ map (m: "${m}/Cargo.toml") members;
      src = lib.cleanSourceWith {
        src = self;
        filter = path: _:
          let p = lib.removePrefix "${toString self}/" (toString path); in
          lib.elem p files || lib.any (f: lib.hasPrefix "${p}/" f) files
          || p == "modules" || (lib.hasPrefix "modules/" p && (lib.length (lib.splitString "/" p) == 2 || baseNameOf p == "module.toml"));
      };
    in
    pkgs.runCommand "${a.id}-ffi-src" { nativeBuildInputs = [ pkgs.python3 ]; } ''
      cd ${src}
      export ARK_CARGO_METADATA=${cargoMetadata}
      python3 ui/prebuild.py ffi ${a.dir} --root . --out $out
    '';
  # The browser's page, generated (ui/prebuild.py's `web`, from app.toml,
  # the phones' library's twin): `<id>-web`'s main.rs and Cargo.toml, run by
  # arkui-web's `WebHost` over the app's crate with the browser's device of
  # each module whose module.toml says `[web]`, its icon beside it; and the
  # page that loads it, index.html, with favicon.svg. Nothing of it is
  # committed; a cargo build adds the crate to the workspace at `<dir>/.web`.
  webSrc = pkgs: cargoMetadata: a:
    let
      members = map (m: normal "rust/${m}") (builtins.fromTOML (builtins.readFile (self + "/rust/Cargo.toml"))).workspace.members;
      files = [ "ui/prebuild.py" "ui/theme.py" "ui/theme.toml" "${a.dir}/app.toml" "${a.dir}/${a.icon.svg or "icon.svg"}" "rust/Cargo.toml" ] ++ map (m: "${m}/Cargo.toml") members;
      src = lib.cleanSourceWith {
        src = self;
        filter = path: _:
          let p = lib.removePrefix "${toString self}/" (toString path); in
          lib.elem p files || lib.any (f: lib.hasPrefix "${p}/" f) files
          || p == "modules" || (lib.hasPrefix "modules/" p && (lib.length (lib.splitString "/" p) == 2 || baseNameOf p == "module.toml"));
      };
    in
    pkgs.runCommand "${a.id}-web-src" { nativeBuildInputs = [ pkgs.python3 ]; } ''
      cd ${src}
      export ARK_CARGO_METADATA=${cargoMetadata}
      python3 ui/prebuild.py web ${a.dir} --root . --out $out
    '';
  webName = a: "${a.id}-web";
  # The crates the page names (ui/prebuild.py's `web_crates`): the app's,
  # its domain's, its demo's and the runtime's.
  webCrates = a: lib.unique ([ a.crate (lib.replaceStrings [ "_" ] [ "-" ] (lib.head (lib.splitString "::" a.ffi.domain))) ]
    ++ lib.optional (a.ffi ? demo) a.ffi.demo
    ++ [ "ark" "ark-app" "ark-peer" ]);
  # The modules whose browser device the page registers (module.toml's `[web]`).
  webModules = a: map (m: m.name) (lib.filter (m: m.toml ? web) (modulesOf a));

  # `money/app/../../modules/ark-camera` as `modules/ark-camera`.
  normal = p: lib.concatStringsSep "/" (lib.foldl'
    (acc: seg: if seg == ".." then lib.init acc else if seg == "." || seg == "" then acc else acc ++ [ seg ])
    [ ] (lib.splitString "/" p));
  # The device modules an app takes on: the crates its app crate
  # (`<dir>/app`) depends on, directly or through its other path
  # dependencies (normal and build, never dev), whose Cargo.toml says
  # `[package.metadata.ark] module` — what ui/prebuild.py reads of `cargo
  # metadata`, read here off the same manifests while nix evaluates. Each
  # by its crate's name, with its module.toml.
  modulesOf = a:
    let
      manifest = dir: builtins.fromTOML (builtins.readFile (self + "/${dir}/Cargo.toml"));
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
  # The crates the library names, by their packages' names (a lib's name
  # with dashes, which ui/prebuild.py's check holds): the app's, every
  # twin's, its domain's and its demo's (ui/prebuild.py's `ffi_crates`).
  ffiCrates = a: lib.unique ([ a.crate ]
    ++ map (lib.replaceStrings [ "_" ] [ "-" ]) (lib.attrNames (a.ffi.twins or { }) ++ [ "ark_app" ])
    ++ [ (lib.replaceStrings [ "_" ] [ "-" ] (lib.head (lib.splitString "::" a.ffi.domain))) ]
    ++ lib.optional (a.ffi ? demo) a.ffi.demo
    ++ [ "ark" "ark-app" ]);
  # Android's features are the library's default; the iPhone's build takes none.
  ffiFeatures = a: platform: map (f: "ark-ffi/${f}") ((a.ffi or { }).${platform} or [ ]);

  # The app's generated crates, in crate2nix's form (nix/rust.nix's
  # `extraCrates`).
  # An app of Kotlin alone (no `crate`: revelation) has none.
  crates = pkgs: cargoMetadata: dir:
    let a = read dir; in lib.optionalAttrs (a ? crate && a ? ffi) {
      "${ffiName a}" = {
        crateName = ffiName a;
        version = "0.1.0";
        edition = "2021";
        src = ffiSrc pkgs cargoMetadata a;
        libPath = "lib.rs";
        libName = ffiLib a;
        type = [ "lib" "cdylib" "staticlib" ];
        dependencies = map (c: { name = c; packageId = c; }) (ffiCrates a) ++ [
          { name = "ark-ffi"; packageId = "ark-ffi"; features = arkFfiFeatures a; }
          { name = "uniffi"; packageId = "uniffi"; usesDefaultFeatures = false; }
        ];
        # Android's are its default; the iPhone's build takes none.
        features = { default = ffiFeatures a "android"; };
      };
    } // lib.optionalAttrs (a ? crate && a ? ffi && lib.elem "web" (a.platforms or [ ])) {
      "${webName a}" = {
        crateName = webName a;
        version = "0.1.0";
        edition = "2021";
        src = webSrc pkgs cargoMetadata a;
        crateBin = [{ name = webName a; path = "main.rs"; requiredFeatures = [ ]; }];
        dependencies = map (c: { name = c; packageId = c; } // lib.optionalAttrs (c == "ark-peer") { usesDefaultFeatures = false; }) (webCrates a) ++ [
          { name = "arkui-web"; packageId = "arkui-web"; usesDefaultFeatures = false; features = [ "host" ]; }
        ] ++ map (m: { name = m; packageId = m; features = [ "web" ]; target = { target, features }: ("wasm32" == target."arch" or null); }) (webModules a);
        features = { default = [ "csr" ]; csr = [ "arkui-web/csr" ]; };
      };
    } // lib.optionalAttrs (a ? crate) {
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

  # The app's entry point that draws the app's own mark and is not
  # generated yet: its desktop (`iced/`) reads its icon.svg as cargo finds
  # it, beside its crate (`include_str!("../../icon.svg")`). It is built
  # from the app's directory with only that crate, the icon and app.toml in
  # it, the crate its workspace member, rather than from the crate's own
  # directory, which has no icon. The browser's is generated (`webSrc`),
  # its icon written beside it.
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
    entry "iced";

  # What else of the app's directory an entry point's build holds beside
  # its crate (`entrySources`): app.toml and the icon, by their paths in
  # the repository; none for a member that is not an Ark app's entry point.
  # For a build that takes the crate's tree by its path (flake.nix's
  # `crateTests`), so the icon is where `include_str!` looks for it.
  entryFiles = apps: member:
    let dir = dirOf member; in
    if lib.elem dir apps && baseNameOf member == "iced"
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
