# The release channels (docs/releasing.md), the same for every app the
# flake publishes: what makes an app's unstable build out of its stable
# one. Nothing here names an app (flake.nix's `checks.channels` holds it
# to that); each function is handed the app's own files.
#
#   stable     a `v*` tag's build: the app's own id and name, the APK in
#              fdroid/repo, the page at /apps/<app>/, TestFlight.
#   unstable   every build of main: a second app beside it, its id with
#              `.unstable` after it and its name with " Unstable", its
#              icon banded (nix/channel.py), the APK in
#              fdroid/unstable/repo, the page at /apps/unstable/<app>/.
#
# An app is in both unless its app.toml's `[channels]` says otherwise
# (`stable = false`, `unstable = false`); an app with no app.toml is in
# both. `ios-unstable` (default false) is the hook for an iPhone app's
# unstable TestFlight app, `<bundle>.unstable`, which main would send once
# its App Store Connect record exists (.github/workflows/ios.yaml).
{ lib, pkgs, self }:
let
  py = "${pkgs.python3}/bin/python3 -I ${./channel.py}";
  theme = ../ui/theme.toml;
  defaults = { stable = true; unstable = true; ios-unstable = false; };
in
rec {
  suffix = ".unstable";
  label = " Unstable";
  # Where the site is served from (GitHub Pages' project path).
  siteBase = "/apps/";

  # Whether the app in `dir` is in `channel`, from its app.toml.
  enabled = dir: channel:
    let
      toml = self + "/${dir}/app.toml";
      said = if builtins.pathExists toml then (builtins.fromTOML (builtins.readFile toml)).channels or { } else { };
    in
    said.${channel} or defaults.${channel};

  # An Android project's source made the unstable app's, in place: run in
  # the project's root (a gradle derivation's postPatch), its id `appid`.
  androidPatch = appid: ''
    ${py} android app/src/main/res app/src/main/AndroidManifest.xml ${appid} ${theme}
  '';

  # An icon with the band over it.
  badge = name: svg: pkgs.runCommand "${name}-unstable.svg" { } ''
    ${py} badge ${svg} ${theme} $out
  '';

  # An app's F-Droid metadata made the unstable app's.
  fdroidYml = name: yml: pkgs.runCommand "${name}-unstable.yml" { } ''
    ${py} fdroid-yml ${yml} $out
  '';

  # A browser page (index.html beside pkg/) as the site serves it at
  # `path`, in `channel`.
  webPage = { name, page, path, channel }: pkgs.runCommand "${name}-page" { } ''
    ${py} web ${page} $out ${siteBase}${path}/ ${theme} ${channel}
  '';

  # That no app is named in the channel code: `names` each app's
  # directory, id and name.
  check = { names, files }: pkgs.runCommand "channels-generic" { } ''
    ${py} check ${lib.escapeShellArg (lib.concatStringsSep "," names)} ${lib.concatStringsSep " " files}
    touch $out
  '';
}
