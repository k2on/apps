#!/usr/bin/env bash
# An app's Mac window (apps.md, "The Mac"), on the runner's own screen: the
# app nix built for the Mac opened as it is, given the server CI started
# and the name to sign in as (`--server <URL> --user <name>`, what the
# desktop window is given: an app that takes them opens signed in, its
# sample data synced), left to settle, and its window taken
# (`screencapture -l`, the window alone, no shadow), in light and in dark.
#
#   app-mac.sh <out> <Foo.app> <app> [server URL]
#
# Writes <out>/device.json and <out>/<theme>/window.png, and each of the
# shots.json's `screens` that is `on` "mac", <out>/<theme>/<id>.png: the
# app opened with that screen's launch arguments (`args`, after
# shots.json's `launch.mac`, the Mac's own: a demo, a window size), so a
# screen is opened with no steps. No steps beyond that: a Mac window has
# no XCUITest driver here (drive-ios is the iPhone's).
set -euo pipefail
out=$1
bundle_app=$2
app=$3
server=${4:-}
here=$(cd "$(dirname "$0")" && pwd)
user=$(jq -r '.server.user // empty' "$app/demo/shots.json")
# A Mac app's Info.plist is under Contents/ (an iPhone app's at its root).
exe="$bundle_app/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$bundle_app/Contents/Info.plist")"
tmp=$(mktemp -d)
mkdir -p "$out"
# The window of a process, by its pid (window.swift).
xcrun swiftc -O "$here/window.swift" -o "$tmp/window"

# The system's appearance, dark or not (System Events): given twenty
# seconds, as a prompt for leave to drive System Events would never end.
dark_mode() {
  osascript -e "tell application \"System Events\" to tell appearance preferences to set dark mode to $1" &
  local asked=$!
  for _ in $(seq 1 20); do
    kill -0 "$asked" 2> /dev/null || { wait "$asked"; return; }
    sleep 1
  done
  kill "$asked" 2> /dev/null || true
  echo "::error::the Mac's appearance could not be set (System Events did not answer)"
  return 1
}

args=()
if [ -n "$server" ]; then args=(--server "$server" --user "$user"); fi
# The Mac's own launch arguments, then each screen's id and arguments.
base=()
while IFS= read -r a; do base+=("$a"); done < <(jq -r '.launch.mac // [] | .[]' "$app/demo/shots.json")
screens=$(jq -c '[.screens // [] | .[] | select((.on // []) | index("mac"))]' "$app/demo/shots.json")

# The app opened with `$@`, its window taken as <out>/<theme>/<name>.png.
shoot() {
  local theme=$1 name=$2
  shift 2
  # In shots mode (ArkUI's `Shots`, ui/docs/shots.md): every move at its end at once.
  "$exe" -shots YES "$@" > "$tmp/$theme-$name.log" 2>&1 &
  local pid=$! wid=""
  for _ in $(seq 1 60); do
    wid=$("$tmp/window" "$pid" 2> /dev/null) && break
    kill -0 "$pid" 2> /dev/null || break
    sleep 1
  done
  if [ -z "$wid" ]; then
    echo "::error::$app's Mac window never opened ($name)"
    tail -n 40 "$tmp/$theme-$name.log" || true
    kill "$pid" 2> /dev/null || true
    return 1
  fi
  # Signed in and synced (or the demo opened), then still.
  sleep 8
  # Its front window now, not the first it opened: Settings is a window
  # of its own, opened after the main one (08-settings-banks was the
  # Budget's window in 42977527249 and 43083062235).
  wid=$("$tmp/window" "$pid" 2> /dev/null || echo "$wid")
  mkdir -p "$out/$theme"
  screencapture -x -o -l "$wid" "$out/$theme/$name.png"
  kill "$pid" 2> /dev/null || true
  wait "$pid" 2> /dev/null || true
}

failed=0
for theme in light dark; do
  echo "$app mac $theme"
  dark=false
  [ "$theme" = light ] || dark=true
  dark_mode "$dark"
  shoot "$theme" window ${args[@]+"${args[@]}"} || exit 1
  n=$(jq length <<< "$screens")
  for i in $(seq 0 $((n - 1))); do
    id=$(jq -r ".[$i].id" <<< "$screens")
    each=()
    while IFS= read -r a; do each+=("$a"); done < <(jq -r ".[$i].args // [] | .[]" <<< "$screens")
    echo "$app mac $theme $id"
    shoot "$theme" "$id" ${base[@]+"${base[@]}"} ${each[@]+"${each[@]}"} || failed=1
  done
done
dark_mode false || true

read -r w h < <(sips -g pixelWidth -g pixelHeight "$out/light/window.png" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}')
printf '{"name":"Mac","width":%s,"height":%s,"corner":20,"cutout":"none"}\n' "$w" "$h" > "$out/device.json"
exit $failed
