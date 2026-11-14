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
# The Mac's, of each app the commit has one for (listed, not evaluated).
names=$(nix eval --json "git+file://$(cd "$src" && pwd)#packages.aarch64-darwin" --apply builtins.attrNames 2> /dev/null || echo '[]')
attrs=()
for a in "$@"; do
  if jq -e --arg p "$a-ios-simulator" 'index($p) != null' <<< "$names" > /dev/null; then
    attrs+=("$a-ios-simulator")
  else
    echo "$a: this commit has no $a-ios-simulator; skipped"
  fi
  if jq -e --arg p "$a-macos" 'index($p) != null' <<< "$names" > /dev/null; then attrs+=("$a-macos"); fi
done
[ ${#attrs[@]} -gt 0 ] || exit 0
# All at once (../nix-xcode.sh): the apps' builds side by side, over the
# ArkUI each platform's share. Each line `<attr> <path>`, of each built.
start=$(date +%s)
built=$(bash "$(dirname "$0")/../nix-xcode.sh" "$src" "$xcode" "${attrs[@]}") || failed=1
echo "every app built or fetched in $(( $(date +%s) - start ))s"
for attr in "${attrs[@]}"; do
  path=$(awk -v p="$attr" '$1 == p {print $2}' <<< "$built")
  if [ -z "$path" ]; then failed=1; continue; fi
  case $attr in
    *-ios-simulator)
      a=${attr%-ios-simulator}
      mkdir -p "$out/simulator-$a"
      cp -R "$path"/*.app "$out/simulator-$a/"
      chmod -R u+w "$out/simulator-$a"
      echo "$a: $path"
      ;;
    *-macos)
      a=${attr%-macos}
      app=$(find "$path" -maxdepth 2 -name '*.app' | head -1)
      [ -n "$app" ] || { echo "::error::$attr has no .app in it"; failed=1; continue; }
      mkdir -p "$out/macos-$a"
      cp -R "$app" "$out/macos-$a/"
      chmod -R u+w "$out/macos-$a"
      echo "$a (Mac): $app"
      ;;
  esac
done
exit $failed
