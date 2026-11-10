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
#
# On the booted simulator called <device name> (an iPhone and an iPad can be
# booted at once). ORIENTATION=landscape turns it on its side for the shots
# (the driver's XCUIDevice), =portrait upright (unset: as it is), and
# SIGNED_IN=1 says the app is installed and
# signed in on it already (the iPad's landscape, after its portrait): no
# server's page is waited for. CORNER and CUTOUT say how hero.py draws it
# (an iPhone Pro's, unset).
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
kind=$(basename "$out")
# The shots, then each of the `screens` that is `on` this device (ios,
# ipad-portrait, ipad-landscape): opened by its launch arguments alone,
# after shots.json's `launch.ios` (the iPhone's and the iPad's own: a demo
# on a fixed day).
shots=$(jq -c --arg k "$kind" '(.launch.ios // []) as $l | .shots + [.screens // [] | .[] | select((.on // []) | index($k)) | {id, args: ($l + (.args // [])), steps: (.steps // [])}]' "$app/screens/shots.json")
bundle=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle_app/Info.plist")
orientation=${ORIENTATION:-}
udid=$(xcrun simctl list devices booted -j | jq -r --arg d "$device" '[.devices[][] | select(.state == "Booted")] | (map(select(.name == $d)) + .)[0].udid')
mkdir -p "$out"

xcrun simctl install "$udid" "$bundle_app"
xcrun simctl status_bar "$udid" override --time 9:41 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3

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
    xcrun simctl ui "$udid" appearance "$1"
    [ "$(xcrun simctl ui "$udid" appearance)" = "$1" ] && break
    sleep 2
  done
  sleep 3
}

# Which device's, as its directory says (ios, ipad-portrait, …): the
# results of each its own (`kind`, above).
for theme in light dark; do
  echo "$app $kind $theme"
  appearance "$theme"
  # The test's own lines, and why it failed if it did; its whole log beside its results.
  status=0
  # Signed in once, in light: dark opens signed in already, and with no
  # server given does not wait for the server's page to be there.
  signin=$server
  if [ "$theme" != light ] || [ -n "${SIGNED_IN:-}" ]; then signin=; fi
  TEST_RUNNER_APP_BUNDLE="$bundle" TEST_RUNNER_APP_SERVER="$signin" TEST_RUNNER_APP_USER="$user" \
  TEST_RUNNER_APP_SHOTS="$shots" TEST_RUNNER_APP_OUT="$out/$theme" TEST_RUNNER_APP_ORIENTATION="$orientation" \
    xcodebuild test-without-building -project "$here/drive-ios/Drive.xcodeproj" -scheme Drive \
      -destination "id=$udid" -derivedDataPath "$derived" -resultBundlePath "$results/$app-$kind-$theme.xcresult" \
      > "$results/$app-$kind-$theme.log" 2>&1 || status=$?
  grep -E "error:|failed|passed|Test Case|XCTAssert|\*\* TEST" "$results/$app-$kind-$theme.log" || true
  [ "$status" -eq 0 ] || exit 1
done

# Each shot's size, as it was drawn.
dims() { sips -g pixelWidth -g pixelHeight "$1" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}'; }

# On its side: a screenshot XCUITest gives in the screen's own (portrait)
# frame is turned the way the device was, landscape left: a quarter turn
# counter-clockwise (seen on an iPad Pro 13-inch, Xcode 26.3).
if [ "$orientation" = landscape ]; then
  for png in "$out"/*/*.png; do
    read -r w h < <(dims "$png")
    if [ "$h" -gt "$w" ]; then sips -r 270 "$png" > /dev/null; fi
  done
fi

read -r w h < <(dims "$(find "$out" -mindepth 2 -maxdepth 2 -name '*.png' | sort | head -1)")
printf '{"name":"%s","width":%s,"height":%s,"corner":%s,"cutout":"%s"}\n' "$device" "$w" "$h" "${CORNER:-165}" "${CUTOUT:-island}" > "$out/device.json"
