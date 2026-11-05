#!/usr/bin/env bash
# Each app's iOS screenshots in turn (app-ios.sh), on the simulator that is
# booted; fails, once all are tried, if any did.
#
#   apps-ios.sh <out> <apps> <device name> <app>...
#
# <apps> holds simulator-<app>/ with the app's .app in it (ios.yaml's
# artifacts, unpacked). Writes <out>/<app>/ios/.
set -uo pipefail
out=$1
apps=$2
device=$3
shift 3
failed=0
for a in "$@"; do
  app=$(find "$apps/simulator-$a" -maxdepth 1 -name '*.app' 2>/dev/null | head -1)
  # Nothing published for it yet: said where it was fetched, and its shots kept.
  [ -n "$app" ] || { echo "$a: no iPhone build to take; skipped"; continue; }
  server=$(jq -r '.server.port // empty | "http://127.0.0.1:\(.)"' "$a/screens/shots.json")
  bash "$(dirname "$0")/app-ios.sh" "$out/$a/ios" "$app" "$a" "$device" "$server" ||
    { echo "::error::$a's iOS screenshots failed"; failed=1; }
done
exit $failed
