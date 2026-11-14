#!/usr/bin/env bash
# Each app's Mac window (app-mac.sh), of the apps whose commit has a macOS
# build: the flake's `<app>-macos` for the Mac (aarch64-darwin), built or
# the cache's by ../nix-xcode.sh with the pinned Xcode, as the iPhone's
# are. An app with none is passed over quietly (today every one: the
# outputs are looked for, not asked of any app). Fails, once all are
# tried, if a build or a capture did.
#
#   apps-mac.sh <out> <checkout> <app>...
#
# $XCODE as xcode-pinned.sh sets it. Writes <out>/<app>/mac/; each app any
# of whose shots failed is added to $PARTIAL, as apps-ios.sh's. With $BUILT
# (ios-build.sh's <out>), the app it built there, macos-<app>/<App>.app:
# nothing is built here then, and an app with none there has no Mac build.
set -uo pipefail
out=$1
src=$(cd "$2" && pwd)
shift 2
here=$(dirname "$0")
partial=${PARTIAL:-/dev/null}
# Each phase said with its time: what the Mac's step costs.
p=$(date +%s)
ph() { echo "  $1 $(( $(date +%s) - p ))s"; p=$(date +%s); }
# The checkout's Mac packages, by name: nothing of them is evaluated.
names=$(nix eval --json "git+file://$src#packages.aarch64-darwin" --apply builtins.attrNames 2> /dev/null) || {
  echo "the checkout's Mac packages could not be listed; no Mac windows"
  exit 0
}
ph "the Mac's packages listed"
failed=0
for a in "$@"; do
  if [ -n "${BUILT:-}" ]; then
    app=$(find "$BUILT/macos-$a" -maxdepth 1 -name '*.app' 2> /dev/null | head -1)
    [ -n "$app" ] || { echo "$a: no Mac build ($BUILT/macos-$a); no Mac window"; continue; }
    server=$(jq -r '.server.port // empty | "http://127.0.0.1:\(.)"' "$a/demo/shots.json")
    bash "$here/app-mac.sh" "$out/$a/mac" "$app" "$a" "$server" ||
      { echo "::error::$a's Mac screenshots failed"; echo "$a" >> "$partial"; failed=1; }
    ph "$a: its Mac window taken"
    continue
  fi
  if ! jq -e --arg p "$a-macos" 'index($p) != null' <<< "$names" > /dev/null; then
    echo "$a: the commit has no $a-macos; no Mac window"
    continue
  fi
  if ! line=$(bash "$here/../nix-xcode.sh" "$src" "$XCODE" "$a-macos"); then
    echo "::error::$a-macos did not build"; echo "$a" >> "$partial"; failed=1
    continue
  fi
  app=$(find "${line#* }" -maxdepth 2 -name '*.app' | head -1)
  if [ -z "$app" ]; then
    echo "::error::$a-macos has no .app in it"; echo "$a" >> "$partial"; failed=1
    continue
  fi
  server=$(jq -r '.server.port // empty | "http://127.0.0.1:\(.)"' "$a/demo/shots.json")
  ph "$a: built"
  bash "$here/app-mac.sh" "$out/$a/mac" "$app" "$a" "$server" ||
    { echo "::error::$a's Mac screenshots failed"; echo "$a" >> "$partial"; failed=1; }
  ph "$a: its Mac window taken"
done
exit $failed
