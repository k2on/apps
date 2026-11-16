#!/usr/bin/env bash
# The newest iPhone Pro of the pinned Xcode's iOS (ios-runtime.sh), booted,
# its name written to $RUNNER_TEMP/device: what app-screens' ios job runs in
# the background (meanwhile.sh), one simulator for every app.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
rt=$(bash "$here/ios-runtime.sh")
device=$(xcrun simctl list devices available -j | jq -r --arg rt "$rt" '
  .devices[$rt]
  | map(select(.name | test("^iPhone [0-9]+ Pro$"))) | sort_by(.name) | last | .udid + " " + .name')
udid=${device%% *} device=${device#* }
echo "Booting $device"
echo "$device" > "$RUNNER_TEMP/device"
start=$(date +%s)
xcrun simctl boot "$udid"
xcrun simctl bootstatus "$udid" -b
echo "booted in $(( $(date +%s) - start ))s"
