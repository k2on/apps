#!/usr/bin/env bash
# The docs' iOS captures, from a booted simulator: every shot
# docs/site/catalogue.json says iOS has, in light and in dark, from the Debug
# showcase app (showcase/ios, `-showcase <id>`, ShowcaseApp.swift). A
# recording is the screen recorded while the open shot moves, once told to,
# as an H.264 MP4 at $fps frames a second.
#
#   ios.sh <out> <app> <bundle id> <device name>
#
# Writes <out>/device.json, <out>/<theme>/<id>.png and <out>/<theme>/<id>.mp4.
set -euo pipefail
out=$1
app=$2
bundle=$3
device=$4
fps=${FPS:-60}
# How many times slower than life a recording is taken, then sped back up
# (encode.sh), so its motion has $fps frames a second whatever the
# simulator can draw: the simulator's own Slow Animations, which slows the
# system's motion (a transition's zoom, a bar's collapse) by this much
# (slow-animations.swift), and the showcase's own clock set to match
# (`-slow`, ShowcaseApp.swift).
slow=${IOS_SLOW:-10}
tmp=$(mktemp -d)

xcrun swiftc -O .github/screens/slow-animations.swift -o "$tmp/slow-animations"
udid=$(xcrun simctl list devices booted -j | jq -r '[.devices[][] | select(.state == "Booted")][0].udid')

xcrun simctl install booted "$app"
xcrun simctl status_bar booted override --time 9:41 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3
mkdir -p "$out"

shots=$(jq -r '.components[].shots[] | select(.phones | index("ios")) | "\(.id) \(.kind)"' docs/site/catalogue.json)

# Record shot `id`, `taken` times slower than life, to `to`: opened and
# settled first, so the launch is never in it; then slowed and told to move
# (the Darwin notification the showcase listens for); then sped back up.
record() {
  local id=$1 taken=$2 to=$3
  xcrun simctl launch booted "$bundle" -showcase "$id" -slow "$taken" >/dev/null
  sleep 4
  xcrun simctl io booted recordVideo --codec=h264 --force "$tmp/$id.mov" &
  local rec=$!
  sleep 1
  [ "$taken" = 1 ] || "$tmp/slow-animations" "$udid" on
  xcrun simctl spawn booted notifyutil -p dev.arkui.showcase.go
  sleep $((4 * taken + 1))
  [ "$taken" = 1 ] || "$tmp/slow-animations" "$udid" off
  kill -INT "$rec"
  wait "$rec" || true
  bash .github/screens/encode.sh "$tmp/$id.mov" "$to" "$taken" "$fps"
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
      record "$id" "$slow" "$out/$theme/$id.mp4"
      # Once, to measure the simulator's factor against: the card at life's
      # speed (CALIBRATE, a branch's run only; not published).
      if [ -n "${CALIBRATE:-}" ] && [ "$id" = expanding-card ]; then
        xcrun simctl terminate booted "$bundle" 2>/dev/null || true
        mkdir -p "$out/calibrate"
        record "$id" 1 "$out/calibrate/$theme-$id-life.mp4"
      fi
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

for theme in light dark; do
  appearance "$theme"
  if ! capture "$theme"; then
    echo "::warning::the simulator was not yet $theme; setting it again and taking $theme again"
    sleep 10
    appearance "$theme"
    capture "$theme" || { echo "::error::the simulator would not show $theme"; exit 1; }
  fi
done

# The device, from what it drew.
read -r w h < <(sips -g pixelWidth -g pixelHeight "$(ls "$out"/light/*.png | head -1)" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}')
printf '{"name":"%s","width":%s,"height":%s,"corner":165,"cutout":"island"}\n' "$device" "$w" "$h" > "$out/device.json"
