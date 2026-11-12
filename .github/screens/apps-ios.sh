#!/usr/bin/env bash
# Every app's iOS screenshots on the simulator that is booted (apps.md).
# What is each app's is its <app>/demo/shots.json. Fails, once all are
# taken, if any app's were not. One command per device: the iPhone, or
# (FAMILY=ipad) the iPad, booted already.
#
# An app whose shots.json has a `walk` walks it inside itself (ui/docs/
# shots.md, "In the App"): installed afresh, its list (`phone` on the
# iPhone, `large` on the iPad) written as a plan into its own container,
# and opened with `-shots YES`; it goes through its first run, signs in,
# taps its way from shot to shot, draws each screen in both themes (and on
# the iPad both ways up, turning in the app), writes the pictures beside
# the plan and exits.
#
# Any other app (harken's, whose screens are its own, not ArkUI's views)
# is driven from outside: a run of the driver (drive-ios/) for all of them
# in each theme, the simulator's appearance set between them, so
# xcodebuild's first run is paid once for the device and theme, not once
# for each app; nothing in the app knows, but for `-shots YES`, which ArkUI
# reads (ui/docs/shots.md): no motion, and the ready signal the driver
# waits on instead of any time.
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
# <out>/<app>/ipad-portrait/, then on its side, <out>/<app>/ipad-landscape/
# (a walk turns itself in the app, from a new install each way; the
# driver's app is signed in once). An app whose build is the iPhone's alone
# is passed over.
#
# Each device's own, so the iPhone and the iPad can be taken at once:
# PORT_OFFSET, added to each server's shots.json port (the device's own
# instance of it, app-server.sh's `port`); RESULTS, PARTIAL and DRIVER, as
# below, a directory or file of its own each.
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

# Each driven app's part of the plan (Drive.swift's `Entry`): one for the
# iPhone, two for the iPad (upright, then on its side, signed in already);
# and the walks, one each app.
plan=$results/plan-$family.json
echo '[]' > "$plan"
taken=()
walks=()
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
  if jq -e '.walk' "$a/demo/shots.json" > /dev/null; then
    # One walk a device: the iPad's turns on its side in the app, at
    # every stop, so both ways up come of the one walk.
    walks+=("$a:$family:$bundle:$app")
    for k in $kinds; do taken+=("$a:$k"); done
    continue
  fi
  # Opened once and closed: a simulator just booted took longer than the
  # driver's 90 seconds to launch an app the first time (the iPad on
  # macos-26, app-screens 42959400546: "Timed out attempting to launch").
  xcrun simctl launch "$udid" "$bundle" > /dev/null 2>&1 || true
  xcrun simctl terminate "$udid" "$bundle" > /dev/null 2>&1 || true
  scheme=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleURLTypes:0:CFBundleURLSchemes:0' "$app/Info.plist" 2> /dev/null || true)
  server=$(jq -r --argjson off "${PORT_OFFSET:-0}" '.server.port // empty | "http://127.0.0.1:\(. + $off)"' "$a/demo/shots.json")
  user=$(jq -r '.server.user // empty' "$a/demo/shots.json")
  first=true
  for k in $kinds; do
    kind=${k%%:*} orientation=${k#*:}
    # Its shots, signed in to the app's server: the household every phone
    # and tablet is of.
    shots=$(jq -c '.shots' "$a/demo/shots.json")
    jq --arg b "$bundle" --arg s "$server" --arg u "$user" --arg sc "$scheme" --arg o "$out/$a/$kind" \
      --arg or "$orientation" --argjson signed "$([ "$first" = true ] && echo false || echo true)" --argjson shots "$shots" \
      '. + [{bundle: $b, server: $s, user: $u, scheme: $sc, out: $o, orientation: $or, signedIn: $signed, shots: $shots}]' \
      "$plan" > "$plan.next" && mv "$plan.next" "$plan"
    first=false
    taken+=("$a:$kind:$orientation")
  done
done
[ ${#taken[@]} -gt 0 ] || { echo "no app to take on the $family"; exit 0; }

# The walks, first: each a new install (and the simulator's keychain
# emptied, so no login of a walk before is remembered), its plan in its
# container, opened with `-shots YES`, and waited for until it says it is
# over (`done`, or `failed` and why) or its process ends. Its pictures,
# drawn upright by the app whichever way it is held, then taken from the
# container; its log (each shot's step, switch and picture times) said.
status=0
for w in ${walks[@]+"${walks[@]}"}; do
  IFS=: read -r a kind bundle app <<< "$w"
  list=phone
  [ "$family" = ipad ] && list=large
  server=$(jq -r --argjson off "${PORT_OFFSET:-0}" '.server.port // empty | "http://127.0.0.1:\(. + $off)"' "$a/demo/shots.json")
  t=$(date +%s)
  xcrun simctl terminate "$udid" "$bundle" > /dev/null 2>&1 || true
  xcrun simctl uninstall "$udid" "$bundle" > /dev/null 2>&1 || true
  xcrun simctl keychain "$udid" reset > /dev/null 2>&1 || true
  xcrun simctl install "$udid" "$app" || { echo "::error::$a did not install"; echo "$a" >> "$partial"; status=1; continue; }
  # What the app would ask a person for, said yes to already.
  xcrun simctl privacy "$udid" grant all "$bundle" > /dev/null 2>&1 || true
  data=$(xcrun simctl get_app_container "$udid" "$bundle" data)
  shots="$data/Library/Application Support/shots"
  mkdir -p "$shots"
  jq --arg s "$server" --arg l "$list" \
    '{orientations: (if $l == "large" then ["portrait", "landscape"] else null end), theme: .walk.theme, shots: .walk[$l]} | with_entries(select(.value != null))
     | walk(if type == "string" then gsub("\\{server\\}"; $s) else . end)' "$a/demo/shots.json" > "$shots/plan.json"
  pid=$(xcrun simctl launch "$udid" "$bundle" -shots YES 2>&1 | awk -F': ' '/: [0-9]+$/ {print $NF}')
  echo "$a $kind: the walk launched (pid ${pid:-unknown})"
  until=$(( $(date +%s) + 1200 ))
  while [ "$(date +%s)" -lt "$until" ]; do
    [ -f "$shots/out/done" ] || [ -f "$shots/out/failed" ] && break
    if [ -n "$pid" ] && ! kill -0 "$pid" 2> /dev/null; then
      sleep 1
      [ -f "$shots/out/done" ] || [ -f "$shots/out/failed" ] || echo "::error::$a $kind: the app ended with its walk unsaid"
      break
    fi
    sleep 1
  done
  sed 's/^/  /' "$shots/out/walk.log" 2> /dev/null || true
  cp "$shots/out/walk.log" "$results/walk-$a-$kind.log" 2> /dev/null || true
  echo "$a $kind: the walk took $(( $(date +%s) - t ))s"
  # The iPhone's pictures under ios/, the iPad's under ipad-portrait/ and
  # ipad-landscape/ (the walk's out/portrait/ and out/landscape/).
  dir=$out/$a/ios
  for from in "" portrait landscape; do
    [ -n "$from" ] && dir=$out/$a/ipad-$from
    for theme in light dark; do
      [ -d "$shots/out/${from:+$from/}$theme" ] || continue
      mkdir -p "$dir/$theme"
      cp "$shots/out/${from:+$from/}$theme"/* "$dir/$theme/"
    done
  done
  if [ ! -f "$shots/out/done" ]; then
    echo "::error::$a $kind: the walk failed: $(cat "$shots/out/failed" 2> /dev/null || echo 'it said nothing')"
    cp "$shots/out/failed" "$dir/stuck.why.txt" 2> /dev/null || true
    status=1
  fi
  xcrun simctl terminate "$udid" "$bundle" > /dev/null 2>&1 || true
  rm -f "$shots/plan.json"
done

if [ "$(jq length "$plan")" = 0 ]; then
  echo "nothing for the driver on the $family"
else
[ -f "$derived/built" ] || bash "$here/driver-ios.sh" "$derived"

# A run a theme: every app, in the simulator's appearance set first, as
# the simulator says it back (`simctl ui`; just booted, it can take the
# setting and not yet have it). Set by the driver alone (XCUIDevice), every
# dark pass drew light, byte for byte (43131582873 to 43159033586); the
# driver now also fails a shot the app says it drew in the other theme.
# Each run's own lines, and why it failed if it did; its whole log beside
# its results, and every run's in <family>.log.
echo "the $family's plan: $(jq -c 'map({bundle, out, shots: (.shots | length)})' "$plan")"
start=$(date +%s)
appearance() {
  for _ in $(seq 1 30); do
    xcrun simctl ui "$udid" appearance "$1"
    [ "$(xcrun simctl ui "$udid" appearance)" = "$1" ] && return 0
    sleep 2
  done
  echo "::error::the simulator would not be $1"
  return 1
}
drive() {
  TEST_RUNNER_APP_PLAN="$3" TEST_RUNNER_THEME="$2" \
    xcodebuild test-without-building -project "$here/drive-ios/Drive.xcodeproj" -scheme Drive \
      -destination "id=$udid" -derivedDataPath "$derived" -resultBundlePath "$results/$1.xcresult" \
      > "$results/$1.log" 2>&1
}
# Of the plan $1, the entries any of whose shots in theme $2 are not
# there, as a plan of their own.
missing() {
  local keep='[]' n i out want have
  n=$(jq length "$1")
  for ((i = 0; i < n; i++)); do
    out=$(jq -r ".[$i].out" "$1")
    want=$(jq ".[$i].shots | length" "$1")
    have=$(find "$out/$2" -maxdepth 1 -name '*.png' ! -name 'stuck.png' 2> /dev/null | wc -l | tr -d ' ')
    if [ "$have" -lt "$want" ]; then
      keep=$(jq -c --argjson e "$(jq -c ".[$i]" "$1")" '. + [$e]' <<< "$keep")
    fi
  done
  printf '%s\n' "$keep"
}

: > "$results/$family.log"
for theme in light dark; do
  this=$plan
  run=0
  if appearance "$theme"; then
    drive "$family-$theme" "$theme" "$this" || run=$?
  else
    run=1
    : > "$results/$family-$theme.log"
  fi
  grep -E "^Drive: |error:|failed|passed|Test Case|XCTAssert|\*\* TEST" "$results/$family-$theme.log" || true
  # A run that ended early, once, run again on what it did not take: an app
  # the simulator would not launch in time, or XCUITest losing the app
  # ("Failed to get matching snapshots: … kAXErrorServerNotFound", "Timed out
  # while evaluating UI query", app-screens 43113069044), which ends the test
  # and every shot after it. Each entry left whole is taken whole again in
  # this theme; signed in already in the light one, it is not signed in again.
  if [ "$run" != 0 ]; then
    again=$results/plan-$family-$theme-again.json
    missing "$this" "$theme" > "$again"
    if [ "$(jq length "$again")" -gt 0 ] && appearance "$theme"; then
      echo "the driver ended early; run again on $(jq -c 'map(.out | split("/") | .[-2:] | join("/"))' "$again") ($theme)"
      run=0
      drive "$family-$theme-again" "$theme" "$again" || run=$?
      grep -E "^Drive: |error:|failed|passed|Test Case|XCTAssert|\*\* TEST" "$results/$family-$theme-again.log" || true
      cat "$results/$family-$theme-again.log" >> "$results/$family-$theme.log"
    fi
  fi
  cat "$results/$family-$theme.log" >> "$results/$family.log"
  [ "$run" = 0 ] || status=$run
done
xcrun simctl ui "$udid" appearance light || true
echo "the driver ran for $(( $(date +%s) - start ))s (status $status)"
fi

# Each shot's size, as it was drawn.
dims() { sips -g pixelWidth -g pixelHeight "$1" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}'; }

failed=0
for t in "${taken[@]}"; do
  a=${t%%:*} rest=${t#*:} kind=${rest%%:*} orientation=${rest#*:}
  dir=$out/$a/$kind
  walked=$(jq -r 'if .walk then "yes" else "" end' "$a/demo/shots.json")
  if [ -n "$walked" ]; then
    want=$(jq --arg l "$([ "$family" = ipad ] && echo large || echo phone)" '.walk[$l] | length' "$a/demo/shots.json")
  else
    want=$(jq --arg o "$dir" '[.[] | select(.out == $o) | .shots[]] | length' "$plan")
  fi
  for theme in light dark; do
    have=$(find "$dir/$theme" -maxdepth 1 -name '*.png' ! -name 'stuck.png' 2> /dev/null | wc -l | tr -d ' ')
    if [ "$have" -lt "$want" ]; then
      echo "::error::$a's $kind ($theme): $have of $want shots taken"
      echo "$a" >> "$partial"
      failed=1
    fi
  done
  # On its side: each picture wider than tall (a walk's, drawn by the app
  # as its scene is; the driver's turned upright by it, Drive.swift's
  # `picture`, which checks its pixels too), and a driven app laid out so:
  # its window, in the tree taken with it, wider than tall. A wide canvas
  # alone let a portrait picture with a black band beside it through
  # (43131582873). One that is not fails the app.
  if [ "$orientation" = landscape ]; then
    for png in "$dir"/*/*.png; do
      [ -f "$png" ] || continue
      [ "$(basename "$png")" = stuck.png ] && continue
      read -r w h < <(dims "$png")
      if [ -n "$walked" ]; then
        [ "$w" -gt "$h" ] || { echo "::error::$a's $kind: $(basename "$png") is ${w}x${h}: not on its side"; echo "$a" >> "$partial"; failed=1; }
        continue
      fi
      win=$(grep -m1 -E '^ *Window' "${png%.png}.tree.txt" 2> /dev/null | grep -oE '\{[0-9.]+, [0-9.]+\}\}' | tr -d '{}' || true)
      ww=${win%%,*} wh=${win#*, }
      if [ "$h" -ge "$w" ] || [ -z "$win" ] || ! awk -v a="$ww" -v b="$wh" 'BEGIN {exit !(a > b)}'; then
        echo "::error::$a's $kind: $(basename "$png") is ${w}x${h}, its window ${win:-not in its tree}: not on its side"
        echo "$a" >> "$partial"
        failed=1
      fi
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
