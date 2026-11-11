#!/usr/bin/env bash
# Each iPhone app as nix builds it at a commit, for its screenshots
# (apps.md): `<app>-ios-simulator` (flake.nix, `iosApp`), built or the
# cache's by ../nix-xcode.sh, copied out as <out>/simulator-<app>/<App>.app;
# and its Mac build, `<app>-macos`, where the commit has one, as
# <out>/macos-<app>/<App>.app, which apps-mac.sh takes. Every build the
# shots need is here, before any capture: none builds anything.
#
#   ios-build.sh <checkout> <Xcode.app> <out> <app>...
#
# An app the commit has no such package for is said and left out; one that
# fails to build is said, and the others are still built.
set -uo pipefail
src=$1
xcode=$2
out=$3
shift 3
failed=0
for a in "$@"; do
  if ! line=$(bash "$(dirname "$0")/../nix-xcode.sh" "$src" "$xcode" "$a-ios-simulator"); then
    case $? in
      2) echo "$a: this commit has no $a-ios-simulator; skipped" ;;
      *) failed=1 ;;
    esac
    continue
  fi
  path=${line#* }
  mkdir -p "$out/simulator-$a"
  cp -R "$path"/*.app "$out/simulator-$a/"
  chmod -R u+w "$out/simulator-$a"
  echo "$a: $path"
done
# The Mac's, of each app the commit has one for (listed, not evaluated).
names=$(nix eval --json "git+file://$(cd "$src" && pwd)#packages.aarch64-darwin" --apply builtins.attrNames 2> /dev/null || echo '[]')
for a in "$@"; do
  jq -e --arg p "$a-macos" 'index($p) != null' <<< "$names" > /dev/null || continue
  if ! line=$(bash "$(dirname "$0")/../nix-xcode.sh" "$src" "$xcode" "$a-macos"); then
    failed=1
    continue
  fi
  app=$(find "${line#* }" -maxdepth 2 -name '*.app' | head -1)
  [ -n "$app" ] || { echo "::error::$a-macos has no .app in it"; failed=1; continue; }
  mkdir -p "$out/macos-$a"
  cp -R "$app" "$out/macos-$a/"
  chmod -R u+w "$out/macos-$a"
  echo "$a (Mac): $app"
done
exit $failed
