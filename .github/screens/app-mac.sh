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
# Writes <out>/device.json and <out>/<theme>/window.png. No steps yet: a
# Mac window has no XCUITest driver here (drive-ios is the iPhone's), so
# the one shot is the window as it opens.
set -euo pipefail
out=$1
bundle_app=$2
app=$3
server=${4:-}
here=$(cd "$(dirname "$0")" && pwd)
user=$(jq -r '.server.user // empty' "$app/screens/shots.json")
exe="$bundle_app/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$bundle_app/Info.plist")"
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
for theme in light dark; do
  echo "$app mac $theme"
  dark=false
  [ "$theme" = light ] || dark=true
  dark_mode "$dark"
  "$exe" ${args[@]+"${args[@]}"} > "$tmp/$theme.log" 2>&1 &
  pid=$!
  wid=""
  for _ in $(seq 1 60); do
    wid=$("$tmp/window" "$pid" 2> /dev/null) && break
    kill -0 "$pid" 2> /dev/null || break
    sleep 1
  done
  if [ -z "$wid" ]; then
    echo "::error::$app's Mac window never opened"
    tail -n 40 "$tmp/$theme.log" || true
    kill "$pid" 2> /dev/null || true
    exit 1
  fi
  # Signed in and synced, then still.
  sleep 8
  mkdir -p "$out/$theme"
  screencapture -x -o -l "$wid" "$out/$theme/window.png"
  kill "$pid" 2> /dev/null || true
  wait "$pid" 2> /dev/null || true
done
dark_mode false || true

read -r w h < <(sips -g pixelWidth -g pixelHeight "$out/light/window.png" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}')
printf '{"name":"Mac","width":%s,"height":%s,"corner":20,"cutout":"none"}\n' "$w" "$h" > "$out/device.json"
