#!/usr/bin/env bash
# Each app's iOS screenshots in turn (app-ios.sh), on the simulator that is
# booted; fails, once all are tried, if any did.
#
#   apps-ios.sh <out> <apps> <device name> <app>...
#
# <apps> holds simulator-<app>/ with the app's .app in it (ios.yaml's
# artifacts, unpacked). Writes <out>/<app>/ios/.
#
# FAMILY=ipad: the iPad booted as <device name> instead, and only the apps
# whose build takes the iPad (ios-family.sh); each in portrait,
# <out>/<app>/ipad-portrait/, then on its side, <out>/<app>/ipad-landscape/,
# signed in once. An app whose build is the iPhone's alone is passed over,
# quietly.
#
# PARTIAL, when set, is a file each app any of whose shots failed is added
# to: the workflow keeps no hash beside its shots, so it is taken again.
set -uo pipefail
out=$1
apps=$2
device=$3
shift 3
here=$(dirname "$0")
family=${FAMILY:-iphone}
partial=${PARTIAL:-/dev/null}
# The driver, built by the first app and run by every one after (app-ios.sh).
DRIVER=${DRIVER:-$(mktemp -d)}
export DRIVER
failed=0
for a in "$@"; do
  app=$(find "$apps/simulator-$a" -maxdepth 1 -name '*.app' 2>/dev/null | head -1)
  # Nothing published for it yet: said where it was fetched, and its shots kept.
  [ -n "$app" ] || { echo "$a: no iPhone build to take; skipped"; continue; }
  server=$(jq -r '.server.port // empty | "http://127.0.0.1:\(.)"' "$a/screens/shots.json")
  if [ "$family" = ipad ]; then
    [ -n "$(bash "$here/ios-family.sh" "$apps" "$a")" ] || continue
    if ! ORIENTATION=portrait CORNER=110 CUTOUT=none \
      bash "$here/app-ios.sh" "$out/$a/ipad-portrait" "$app" "$a" "$device" "$server"; then
      echo "::error::$a's iPad (portrait) screenshots failed"; echo "$a" >> "$partial"; failed=1
    elif ! ORIENTATION=landscape CORNER=110 CUTOUT=none SIGNED_IN=1 \
      bash "$here/app-ios.sh" "$out/$a/ipad-landscape" "$app" "$a" "$device" "$server"; then
      echo "::error::$a's iPad (landscape) screenshots failed"; echo "$a" >> "$partial"; failed=1
    fi
  else
    bash "$here/app-ios.sh" "$out/$a/ios" "$app" "$a" "$device" "$server" ||
      { echo "::error::$a's iOS screenshots failed"; echo "$a" >> "$partial"; failed=1; }
  fi
done
exit $failed
