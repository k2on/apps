#!/usr/bin/env bash
# Each app's desktop screenshots in turn (app-desktop.sh); fails, once all
# are tried, if any did.
#
#   apps-desktop.sh <out> <windows> <app>...
#
# <windows> holds desktop-<app>, each app's build (desktop-build.sh); an
# app with none is passed over. Writes <out>/<app>/desktop/.
set -uo pipefail
out=$1
windows=$2
shift 2
failed=0
for a in "$@"; do
  [ -e "$windows/desktop-$a" ] || { echo "$a: no desktop window to take; skipped"; continue; }
  server=$(jq -r '.server.port // empty | "http://127.0.0.1:\(.)"' "$a/screens/shots.json")
  bash "$(dirname "$0")/app-desktop.sh" "$out/$a/desktop" "$windows/desktop-$a" "$a" "$server" ||
    { echo "::error::$a's desktop screenshots failed"; failed=1; }
done
exit $failed
