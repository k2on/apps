#!/usr/bin/env bash
# Each app's Android screenshots in turn (app-android.sh), on the emulator
# that is booted: one that fails is said, and the others are still taken.
#
#   apps-android.sh <out> <apks> <app>...
#
# <apks> holds apk-<app>/ with the app's APK in it (build.yaml's artifacts);
# an app's server, if its shots.json has one, is reached at the host's
# loopback, which the emulator calls 10.0.2.2. Writes <out>/<app>/android/.
set -uo pipefail
out=$1
apks=$2
shift 2
for a in "$@"; do
  apk=$(find "$apks/apk-$a" -name '*.apk' 2>/dev/null | head -1)
  # Nothing published for it yet: said where it was fetched, and its shots kept.
  [ -n "$apk" ] || { echo "$a: no APK to take; skipped"; continue; }
  server=$(jq -r '.server.port // empty | "http://10.0.2.2:\(.)"' "$a/screens/shots.json")
  bash "$(dirname "$0")/app-android.sh" "$out/$a/android" "$apk" "$a" "$server" ||
    echo "::error::$a's Android screenshots failed"
done
