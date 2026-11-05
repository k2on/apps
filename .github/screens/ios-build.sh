#!/usr/bin/env bash
# Each iPhone app as nix builds it at a commit, for its screenshots
# (apps.md): `<app>-ios-simulator` (flake.nix, `iosApp`), built or the
# cache's by ../nix-xcode.sh, copied out as <out>/simulator-<app>/<App>.app.
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
exit $failed
