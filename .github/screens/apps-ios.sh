#!/usr/bin/env bash
# Every app's iOS screenshots on the simulator that is booted (apps.md): each
# app installed, then one run of the driver (drive-ios/) for all of them and
# both themes, so xcodebuild's first run is paid once for the device, not
# once for each app and theme. What is each app's is its
# <app>/screens/shots.json; nothing in the app knows, but for `-shots YES`,
# which ArkUI reads (ui/docs/shots.md): no motion, and the ready signal the
# driver waits on instead of any time. Fails, once all are taken, if any
# app's were not.
#
#   apps-ios.sh <out> <apps> <device name> <app>...
#
# <apps> holds simulator-<app>/ with the app's .app in it (ios-build.sh).
# Writes <out>/<app>/ios/<theme>/<id>.png (and .tree.txt) and its
# device.json; the driver's results and log to $RESULTS (a directory of its
# own; a temporary one when unset).
#
# FAMILY=ipad: the iPad booted as <device name> instead, and only the apps
# whose build takes the iPad (ios-family.sh): each upright,
# <out>/<app>/ipad-portrait/, then on its side, <out>/<app>/ipad-landscape/,
# signed in once. An app whose build is the iPhone's alone is passed over.
#
# PARTIAL, when set, is a file each app any of whose shots were not taken is
# added to: the workflow keeps no hash beside its shots, so it is taken
# again. DRIVER is where the driver is built (driver-ios.sh), once.
set -uo pipefail
out=$1
apps=$2
device=$3
shift 3
here=$(cd "$(dirname "$0")" && pwd)
family=${FAMILY:-iphone}
partial=${PARTIAL:-/dev/null}
results=${RESULTS:-$(mktemp -d)}
mkdir -p "$results" "$out"
derived=${DRIVER:-$(mktemp -d)}
udid=$(xcrun simctl list devices booted -j | jq -r --arg d "$device" '[.devices[][] | select(.state == "Booted")] | (map(select(.name == $d)) + .)[0].udid')
# The status bar as Apple's own screenshots have it, whatever the time; the
# battery full and not charging, as Android's demo mode has it (`plugged
# false`): a charging battery's bolt is drawn by an animation that runs
# again and again (dry run 42929213705: money never ready, "moving
# CALayer.contents CAKeyframeAnimation 0.8s", the status bar's).
xcrun simctl status_bar "$udid" override --time 9:41 --batteryState discharging --batteryLevel 100 --cellularBars 4 --wifiBars 3

# Each app's part of the plan (Drive.swift's `Entry`): one for the iPhone,
# two for the iPad (upright, then on its side, signed in already).
plan=$results/plan-$family.json
echo '[]' > "$plan"
taken=()
for a in "$@"; do
  app=$(find "$apps/simulator-$a" -maxdepth 1 -name '*.app' 2>/dev/null | head -1)
  # Nothing published for it yet: said where it was fetched, and its shots kept.
  [ -n "$app" ] || { echo "$a: no iPhone build to take; skipped"; continue; }
  if [ "$family" = ipad ]; then
    [ -n "$(bash "$here/ios-family.sh" "$apps" "$a")" ] || continue
    kinds="ipad-portrait:portrait ipad-landscape:landscape"
  else
    kinds="ios:"
  fi
  xcrun simctl install "$udid" "$app" || { echo "::error::$a did not install"; echo "$a" >> "$partial"; continue; }
  bundle=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")
  # Opened once and closed: a simulator just booted took longer than the
  # driver's 90 seconds to launch an app the first time (the iPad on
  # macos-26, app-screens 42959400546: "Timed out attempting to launch").
  xcrun simctl launch "$udid" "$bundle" > /dev/null 2>&1 || true
  xcrun simctl terminate "$udid" "$bundle" > /dev/null 2>&1 || true
  scheme=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleURLTypes:0:CFBundleURLSchemes:0' "$app/Info.plist" 2> /dev/null || true)
  server=$(jq -r '.server.port // empty | "http://127.0.0.1:\(.)"' "$a/screens/shots.json")
  user=$(jq -r '.server.user // empty' "$a/screens/shots.json")
  first=true
  for k in $kinds; do
    kind=${k%%:*} orientation=${k#*:}
    # The shots, then each of the `screens` that is `on` this device:
    # opened by its launch arguments alone, after shots.json's `launch.ios`
    # (a demo on a fixed day).
    shots=$(jq -c --arg k "$kind" '(.launch.ios // []) as $l | .shots + [.screens // [] | .[] | select((.on // []) | index($k)) | {id, args: ($l + (.args // [])), steps: (.steps // []), route}] | map(with_entries(select(.value != null)))' "$a/screens/shots.json")
    jq --arg b "$bundle" --arg s "$server" --arg u "$user" --arg sc "$scheme" --arg o "$out/$a/$kind" \
      --arg or "$orientation" --argjson signed "$([ "$first" = true ] && echo false || echo true)" --argjson shots "$shots" \
      '. + [{bundle: $b, server: $s, user: $u, scheme: $sc, out: $o, orientation: $or, signedIn: $signed, shots: $shots}]' \
      "$plan" > "$plan.next" && mv "$plan.next" "$plan"
    first=false
    taken+=("$a:$kind:$orientation")
  done
done
[ ${#taken[@]} -gt 0 ] || { echo "no app to take on the $family"; exit 0; }

[ -f "$derived/built" ] || bash "$here/driver-ios.sh" "$derived"

# One run: every app, both themes. Its own lines, and why it failed if it
# did; its whole log beside its results.
echo "the $family's plan: $(jq -c 'map({bundle, out, shots: (.shots | length)})' "$plan")"
status=0
start=$(date +%s)
drive() {
  TEST_RUNNER_APP_PLAN="$plan" \
    xcodebuild test-without-building -project "$here/drive-ios/Drive.xcodeproj" -scheme Drive \
      -destination "id=$udid" -derivedDataPath "$derived" -resultBundlePath "$results/$1.xcresult" \
      > "$results/$1.log" 2>&1
}
drive "$family" || status=$?
# An app the simulator would not launch in time, once: run again, once.
if [ "$status" != 0 ] && grep -q "Timed out attempting to launch" "$results/$family.log"; then
  echo "an app did not launch in time; the driver run again"
  status=0
  drive "$family-again" || status=$?
  cp "$results/$family-again.log" "$results/$family.log"
fi
grep -E "error:|failed|passed|Test Case|XCTAssert|\*\* TEST" "$results/$family.log" || true
echo "the driver ran for $(( $(date +%s) - start ))s (status $status)"

# Each shot's size, as it was drawn.
dims() { sips -g pixelWidth -g pixelHeight "$1" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}'; }

failed=0
for t in "${taken[@]}"; do
  a=${t%%:*} rest=${t#*:} kind=${rest%%:*} orientation=${rest#*:}
  dir=$out/$a/$kind
  want=$(jq --arg o "$dir" '[.[] | select(.out == $o) | .shots[]] | length' "$plan")
  for theme in light dark; do
    have=$(find "$dir/$theme" -maxdepth 1 -name '*.png' ! -name 'stuck.png' 2> /dev/null | wc -l | tr -d ' ')
    if [ "$have" -lt "$want" ]; then
      echo "::error::$a's $kind ($theme): $have of $want shots taken"
      echo "$a" >> "$partial"
      failed=1
    fi
  done
  # On its side: a screenshot XCUITest gives in the screen's own (portrait)
  # frame is turned the way the device was, landscape left: a quarter turn
  # counter-clockwise (seen on an iPad Pro 13-inch, Xcode 26.3).
  if [ "$orientation" = landscape ]; then
    for png in "$dir"/*/*.png; do
      [ -f "$png" ] || continue
      read -r w h < <(dims "$png")
      if [ "$h" -gt "$w" ]; then sips -r 270 "$png" > /dev/null; fi
    done
  fi
  first=$(find "$dir" -mindepth 2 -maxdepth 2 -name '*.png' ! -name 'stuck.png' 2> /dev/null | sort | head -1)
  [ -n "$first" ] || continue
  read -r w h < <(dims "$first")
  if [ "$family" = ipad ]; then corner=110 cutout=none; else corner=${CORNER:-165} cutout=${CUTOUT:-island}; fi
  printf '{"name":"%s","width":%s,"height":%s,"corner":%s,"cutout":"%s"}\n' "$device" "$w" "$h" "$corner" "$cutout" > "$dir/device.json"
done
[ "$status" -eq 0 ] || failed=1
exit $failed
