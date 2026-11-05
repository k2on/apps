#!/usr/bin/env bash
# An app's screenshots from a booted simulator (apps.md), the same for every
# app: what is the app's is its <app>/screens/shots.json. The app built for
# the simulator from the commit sent to TestFlight (or a PR's), installed,
# and driven by the UI test in drive-ios/ — signed in through arkui's
# onboarding to the server CI started (app-server.sh), then each shot's
# steps done — in light and then in dark. Nothing in the app knows.
#
#   app-ios.sh <out> <Foo.app> <app> <device name> [server URL]
#
# Writes <out>/device.json and <out>/<theme>/<id>.png, and each theme's test
# results to $RESULTS (a directory of its own, so they are never taken for
# shots; a temporary one when unset).
set -euo pipefail
out=$1
bundle_app=$2
app=$3
device=$4
server=${5:-}
results=${RESULTS:-$(mktemp -d)}
mkdir -p "$results"
here=$(cd "$(dirname "$0")" && pwd)
user=$(jq -r '.server.user // empty' "$app/screens/shots.json")
shots=$(jq -c '.shots' "$app/screens/shots.json")
bundle=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle_app/Info.plist")
udid=$(xcrun simctl list devices booted -j | jq -r '[.devices[][] | select(.state == "Booted")][0].udid')
mkdir -p "$out"

xcrun simctl install booted "$bundle_app"
xcrun simctl status_bar booted override --time 9:41 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3

# The driver, built once: in $DRIVER when set, where the app taken before
# this one on the same simulator left it built (apps-ios.sh), so it is not
# built again; nothing in it is any one app's.
derived=${DRIVER:-$(mktemp -d)}
if ! [ -f "$derived/built" ]; then
  (cd "$here/drive-ios" && xcodegen generate >/dev/null)
  xcodebuild build-for-testing -project "$here/drive-ios/Drive.xcodeproj" -scheme Drive \
    -destination "id=$udid" -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO -quiet
  touch "$derived/built"
fi

# The simulator in `theme`, as it says back (ios.sh says why).
appearance() {
  for _ in $(seq 1 30); do
    xcrun simctl ui booted appearance "$1"
    [ "$(xcrun simctl ui booted appearance)" = "$1" ] && break
    sleep 2
  done
  sleep 3
}

for theme in light dark; do
  echo "$app ios $theme"
  appearance "$theme"
  # The test's own lines, and why it failed if it did; its whole log beside its results.
  status=0
  TEST_RUNNER_APP_BUNDLE="$bundle" TEST_RUNNER_APP_SERVER="$server" TEST_RUNNER_APP_USER="$user" \
  TEST_RUNNER_APP_SHOTS="$shots" TEST_RUNNER_APP_OUT="$out/$theme" \
    xcodebuild test-without-building -project "$here/drive-ios/Drive.xcodeproj" -scheme Drive \
      -destination "id=$udid" -derivedDataPath "$derived" -resultBundlePath "$results/$app-ios-$theme.xcresult" \
      > "$results/$app-ios-$theme.log" 2>&1 || status=$?
  grep -E "error:|failed|passed|Test Case|XCTAssert|\*\* TEST" "$results/$app-ios-$theme.log" || true
  [ "$status" -eq 0 ] || exit 1
done

read -r w h < <(sips -g pixelWidth -g pixelHeight "$(ls "$out"/*/*.png | head -1)" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}')
printf '{"name":"%s","width":%s,"height":%s,"corner":165,"cutout":"island"}\n' "$device" "$w" "$h" > "$out/device.json"
