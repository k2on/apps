#!/usr/bin/env bash
# Each app's desktop screenshots in turn (app-desktop.sh), and its page's
# (app-web.sh), the same wide walk; fails, once all are tried, if any did.
#
#   apps-desktop.sh <out> <windows> <app>...
#
# <windows> holds desktop-<app> and web-<app>, each app's builds
# (desktop-build.sh); one with neither is passed over. Writes
# <out>/<app>/desktop/ and <out>/<app>/web/.
set -uo pipefail
out=$1
windows=$2
shift 2
failed=0
for a in "$@"; do
  if [ -e "$windows/web-$a" ]; then
    bash "$(dirname "$0")/app-web.sh" "$out/$a/web" "$windows/web-$a" "$a" ||
      { echo "::error::$a's browser screenshots failed"; failed=1; }
  fi
  [ -e "$windows/desktop-$a" ] || { echo "$a: no desktop window to take; skipped"; continue; }
  server=$(jq -r '.server.port // empty | "http://127.0.0.1:\(.)"' "$a/demo/shots.json")
  bash "$(dirname "$0")/app-desktop.sh" "$out/$a/desktop" "$windows/desktop-$a" "$a" "$server" ||
    { echo "::error::$a's desktop screenshots failed"; failed=1; }
done
exit $failed
