#!/usr/bin/env bash
# An app's Mac window (apps.md, "The wide windows"), on the runner's own
# screen: the app nix built for the Mac, walking shots.json's `wide` inside
# itself (ArkApp's ShotsWalk, the iPhone's walk: each step taken in Rust by
# the id it names), the same ids in the same order as the desktop's window
# and the browser. The demo's household (`-demo`, on DEMO_DAY, else
# `wide.today`), at `wide.mac`'s size; light, then reset and dark, the app
# drawing itself in each (`NSApp.appearance`). Before each picture the walk
# makes the app the active one and its window key (a launch from here came
# up behind: grey traffic lights, a faded toolbar, app-screens
# 43796348626), then asks for it (`<plan>/out/want`: the window's number,
# the theme, the id); this takes it with `screencapture -l` (the window
# alone, no shadow) and takes the ask away. And first, `welcome`: the
# first run's page, the app opened with no demo, light and dark (the
# system's dark mode), a Mac addition the others have no page of.
#
#   app-mac.sh <out> <Foo.app> <app>
#
# Writes <out>/device.json and <out>/<theme>/<id>.png, each walked shot's
# <id>.tree.txt beside it, and <out>/walk.log.
set -euo pipefail
out=$1
bundle_app=$2
app=$3
here=$(cd "$(dirname "$0")" && pwd)
shots="$app/src/demo/shots.json"
# A Mac app's Info.plist is under Contents/ (an iPhone app's at its root).
exe="$bundle_app/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$bundle_app/Contents/Info.plist")"
tmp=$(mktemp -d)
mkdir -p "$out/light" "$out/dark"
# The window of a process, by its pid (window.swift).
began=$(date +%s)
xcrun swiftc -O "$here/window.swift" -o "$tmp/window"
echo "  window.swift compiled in $(( $(date +%s) - began ))s"
# The display big enough for the window, at twice the pixels where it can
# be (display.swift): the wide windows' pictures alike.
xcrun swiftc -O "$here/display.swift" -o "$tmp/display"
read -r ww wh < <(jq -r '.wide.window | "\(.[0]) \(.[1])"' "$shots")
"$tmp/display" "$ww" "$((wh + 40))" || echo "::warning::the display's mode could not be set"

# Asked of System Events, given twenty seconds: a prompt for leave to drive
# it would never end.
events() {
  osascript -e "tell application \"System Events\" to $1" > /dev/null &
  local asked=$!
  for _ in $(seq 1 20); do
    kill -0 "$asked" 2> /dev/null || { wait "$asked"; return; }
    sleep 1
  done
  kill "$asked" 2> /dev/null || true
  echo "::warning::System Events did not answer: $1"
  return 1
}
# The process of `pid` the frontmost: the app the active one.
front() { events "set frontmost of (first process whose unix id is $1) to true" || true; }
# The system light: what the app's own appearance is drawn over.
events "tell appearance preferences to set dark mode to false" || true

# The first run's page, its window taken once the app is in front and still.
welcome() {
  local theme=$1
  shift
  "$exe" -shots YES "$@" > "$tmp/welcome-$theme.log" 2>&1 &
  local pid=$! wid=""
  for _ in $(seq 1 60); do
    wid=$("$tmp/window" "$pid" 2> /dev/null) && break
    kill -0 "$pid" 2> /dev/null || break
    sleep 1
  done
  if [ -z "$wid" ]; then
    echo "::error::$app's Mac window never opened (welcome $theme)"
    tail -n 40 "$tmp/welcome-$theme.log" || true
    kill "$pid" 2> /dev/null || true
    return 1
  fi
  front "$pid"
  sleep 4
  screencapture -x -o -l "$wid" "$out/$theme/welcome.png"
  kill "$pid" 2> /dev/null || true
  wait "$pid" 2> /dev/null || true
  echo "  $theme welcome: taken"
}
failed=0
welcome light || failed=1
# Dark by the system's own dark mode: on macOS 26 a launch's
# `-AppleInterfaceStyle Dark` left the page white (app-screens 43926273208).
events "tell appearance preferences to set dark mode to true" || true
welcome dark || failed=1
events "tell appearance preferences to set dark mode to false" || true

# The walk's plan: the Mac's of `wide` (a shot or a step `on` others alone
# left out), in the app's reach.
plan="$tmp/walk"
mkdir -p "$plan"
jq '{shots: [.wide.shots[] | select((.on // ["mac"]) | index("mac"))
             | .steps = [(.steps // [])[] | select((.on // ["mac"]) | index("mac")) | del(.on)] | del(.on)]}' \
  "$shots" > "$plan/plan.json"
day=${DEMO_DAY:-$(jq -r '.wide.today' "$shots")}
mac=()
while IFS= read -r a; do mac+=("$a"); done < <(jq -r '.wide.mac // [] | .[]' "$shots")
echo "$app mac walk: $(jq '.shots | length' "$plan/plan.json") shots, the demo on $day"
began=$(date +%s)
"$exe" -shots YES -shotsPlan "$plan/plan.json" -demo "$day" ${mac[@]+"${mac[@]}"} > "$tmp/walk.log" 2>&1 &
pid=$!
fronted=""
# Each picture the walk asks for, until it ends (or 15 minutes: a guard).
while kill -0 "$pid" 2> /dev/null; do
  if [ -z "$fronted" ] && "$tmp/window" "$pid" > /dev/null 2>&1; then front "$pid"; fronted=1; fi
  if [ -f "$plan/out/want" ]; then
    read -r wid theme id < "$plan/out/want"
    front "$pid"
    mkdir -p "$plan/out/$theme"
    screencapture -x -o -l "$wid" "$plan/out/$theme/$id.png" || echo "::error::$id $theme: no picture of window $wid"
    rm -f "$plan/out/want"
  fi
  if [ $(( $(date +%s) - began )) -gt 900 ]; then
    echo "::error::$app's Mac walk still going after 900s"
    kill "$pid" 2> /dev/null || true
    break
  fi
  sleep 0.1
done
wait "$pid" 2> /dev/null || true
cat "$plan/out/walk.log" 2> /dev/null || tail -n 60 "$tmp/walk.log"
cp "$plan/out/walk.log" "$out/walk.log" 2> /dev/null || true
for theme in light dark; do
  cp "$plan/out/$theme"/* "$out/$theme/" 2> /dev/null || true
done
if ! [ -f "$plan/out/done" ]; then
  echo "::error::$app's Mac walk failed: $(cat "$plan/out/failed" 2> /dev/null || echo 'it ended with no word')"
  tail -n 40 "$tmp/walk.log" || true
  failed=1
fi
echo "  the walk: $(( $(date +%s) - began ))s"

read -r w h < <(sips -g pixelWidth -g pixelHeight "$out/light/welcome.png" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}')
printf '{"name":"Mac","width":%s,"height":%s,"corner":20,"cutout":"none"}\n' "$w" "$h" > "$out/device.json"
exit $failed
