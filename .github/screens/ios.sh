#!/usr/bin/env bash
# The docs' iOS captures, from a booted simulator: every shot
# docs/site/catalogue.json says iOS has, in each of $THEMES (light and dark
# unless it says otherwise; the workflow takes each on a runner of its
# own), from the Debug
# showcase app (showcase/ios, `-showcase <id>`, ShowcaseApp.swift). A
# recording is the screen recorded while the open shot moves, once told to,
# as an H.264 MP4 at $fps frames a second, with the recorder and the time
# warp ios-tools.sh built into <tools>.
#
#   ios.sh <out> <app> <bundle id> <device name> <tools>
#
# Writes <out>/device.json, <out>/<theme>/<id>.png and <out>/<theme>/<id>.mp4.
set -euo pipefail
out=$1
app=$2
bundle=$3
device=$4
tools=$5
fps=${FPS:-60}
tmp=$(mktemp -d)

udid=$(xcrun simctl list devices booted -j | jq -r '[.devices[][] | select(.state == "Booted")][0].udid')

xcrun simctl install booted "$app"
xcrun simctl status_bar booted override --time 9:41 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3
mkdir -p "$out"

shots=$(jq -r '.components[].shots[] | select(.phones | index("ios")) | "\(.id) \(.kind)"' docs/site/catalogue.json)

# How many times slower than life a shot's moves are taken, then sped back
# up (encode.sh), so its motion has $fps frames a second whatever the
# simulator composes (a tenth of that, for the system's zoom): time slowed
# in the showcase for the frameworks that animate (timewarp.c, loaded by
# SIMCTL_CHILD_DYLD_INSERT_LIBRARIES), from when the showcase says a move
# begins to when it says it ends; the stills between take life's time.
# The simulator composes a scrolled list at some fifteen to fifty-five
# frames a second, as busy as the runner is, so a scroll needs ten times;
# the system's zoom, at ten to twenty, needs thirty. $IOS_SLOW, if set, is
# every shot's.
slowness() {
  case $1 in
    scroll-to-top) echo "${IOS_SLOW:-10}" ;;
    *) echo "${IOS_SLOW:-30}" ;;
  esac
}

# The showcase saying `name` (dev.arkui.showcase.<name>, a Darwin
# notification): `listen` before what makes it say so, then `heard` waits
# for it, up to `seconds`.
listen() {
  xcrun simctl spawn booted notifyutil -1 "dev.arkui.showcase.$1" >/dev/null &
  listening=$!
}
heard() {
  local seconds=$1
  for _ in $(seq 1 $((seconds * 2))); do
    kill -0 "$listening" 2>/dev/null || { wait "$listening" || true; return 0; }
    sleep 0.5
  done
  kill "$listening" 2>/dev/null || true
  return 1
}

# Record shot `id` to `to`: opened and left to settle, so its launch is
# never in it, then told to move (dev.arkui.showcase.go), recorded until it
# says it is done, then made a page's MP4.
record() {
  local id=$1 to=$2 slow
  slow=$(slowness "$id")
  listen ready
  SIMCTL_CHILD_DYLD_INSERT_LIBRARIES="$tools/timewarp.dylib" SIMCTL_CHILD_TIMEWARP="$slow" \
    xcrun simctl launch booted "$bundle" -showcase "$id" -slow "$slow" >/dev/null
  heard 30 || echo "::warning::$id did not say it was ready"
  listen done
  "$tools/record" "$udid" $((10 * slow)) 540 "$tmp/$id.mov" > >(sed 's/^/  /') &
  local rec=$!
  sleep 1
  xcrun simctl spawn booted notifyutil -p dev.arkui.showcase.go
  # The screen as the shot is told to go, which it opens on.
  kill -USR1 "$rec"
  heard $((10 * slow)) || echo "::warning::$id did not say it was done"
  kill -INT "$rec" 2>/dev/null || true
  wait "$rec"
  xcrun simctl terminate booted "$bundle" 2>/dev/null || true
  local documents
  documents="$(xcrun simctl get_app_container booted "$bundle" data)/Documents"
  sed 's/^/  /' "$documents/frames.log" 2>/dev/null || true
  bash .github/screens/encode.sh "$tmp/$id.mov" "$to" "$documents/timewarp.log" "$fps" | tee "$tmp/$id.encoded"
}

# How many distinct frames a recording of shot `id` must have: a good one
# of the card has some forty-five to sixty, of the scroll thirty-five to
# forty, and one taken while a just-booted simulator is still busy with
# itself (which can last minutes) far fewer, or holds frames a tenth of a
# second at a time.
least() {
  case $1 in
    expanding-card) echo 45 ;;
    *) echo 30 ;;
  esac
}

# `record`, taken again while it has fewer distinct frames than it should,
# up to three times.
recording() {
  local id=$1 to=$2 distinct least
  least=$(least "$id")
  for try in 1 2 3; do
    record "$id" "$to"
    distinct=$(grep -o '[0-9]* distinct' "$tmp/$id.encoded" | tr -dc 0-9)
    [ "$distinct" -ge "$least" ] && return 0
    echo "::warning::$id had $distinct distinct frames (try $try); taking it again"
    xcrun simctl terminate booted "$bundle" 2>/dev/null || true
  done
}

# The simulator in `theme`, as it says back: just after a first boot it
# can take the setting and not yet show it.
appearance() {
  for _ in $(seq 1 30); do
    xcrun simctl ui booted appearance "$1"
    [ "$(xcrun simctl ui booted appearance)" = "$1" ] && break
    sleep 2
  done
  sleep 3
}

# How bright a capture's first frame is, 0 to 255.
brightness() {
  ffmpeg -nostdin -loglevel error -i "$1" -vf "scale=1:1,format=gray" -frames:v 1 -f rawvideo - | od -An -tu1 | tr -d ' \n'
}

# Every shot in `theme`; fails if a capture is not in it (light bright,
# dark dark), which a theme set but not yet shown gives.
capture() {
  theme=$1
  mkdir -p "$out/$theme"
  while read -r id kind <&3; do
    [ -n "$id" ] || continue
    echo "ios $theme $id ($kind)"
    xcrun simctl terminate booted "$bundle" 2>/dev/null || true
    if [ "$kind" = recording ]; then
      recording "$id" "$out/$theme/$id.mp4"
    else
      xcrun simctl launch booted "$bundle" -showcase "$id" >/dev/null
      sleep 4
      xcrun simctl io booted screenshot "$out/$theme/$id.png" >/dev/null
    fi
    file=$(ls "$out/$theme/$id".* | head -1)
    level=$(brightness "$file")
    if { [ "$theme" = light ] && [ "$level" -lt 128 ]; } || { [ "$theme" = dark ] && [ "$level" -ge 128 ]; }; then
      echo "  $id is not $theme (brightness $level)"
      return 1
    fi
  done 3<<< "$shots"
}

for theme in ${THEMES:-light dark}; do
  appearance "$theme"
  if ! capture "$theme"; then
    echo "::warning::the simulator was not yet $theme; setting it again and taking $theme again"
    sleep 10
    appearance "$theme"
    capture "$theme" || { echo "::error::the simulator would not show $theme"; exit 1; }
  fi
done

# The device, from what it drew.
read -r w h < <(sips -g pixelWidth -g pixelHeight "$(ls "$out"/*/*.png | head -1)" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}')
printf '{"name":"%s","width":%s,"height":%s,"corner":165,"cutout":"island"}\n' "$device" "$w" "$h" > "$out/device.json"
