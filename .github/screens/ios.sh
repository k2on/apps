#!/usr/bin/env bash
# The docs' iOS captures, from a booted simulator: every shot
# docs/site/catalogue.json says iOS has, in light and in dark, from the Debug
# build's showcase (`-showcase <id>`, Showcase.swift).
#
#   ios.sh <out> <app> <bundle id> <device name>
#
# Writes <out>/device.json, <out>/<theme>/<id>.png and <out>/<theme>/<id>/NNN.jpg.
set -euo pipefail
out=$1
app=$2
bundle=$3
device=$4
fps=${FPS:-10}
tmp=$(mktemp -d)

xcrun simctl install booted "$app"
xcrun simctl status_bar booted override --time 9:41 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3
mkdir -p "$out"

shots=$(jq -r '.components[].shots[] | select(.phones | index("ios")) | "\(.id) \(.kind)"' docs/site/catalogue.json)

for theme in light dark; do
  xcrun simctl ui booted appearance "$theme"
  sleep 2
  mkdir -p "$out/$theme"
  while read -r id kind <&3; do
    [ -n "$id" ] || continue
    echo "ios $theme $id ($kind)"
    xcrun simctl terminate booted "$bundle" 2>/dev/null || true
    if [ "$kind" = recording ]; then
      command -v ffmpeg >/dev/null || brew install ffmpeg
      xcrun simctl io booted recordVideo --codec=h264 --force "$tmp/$id.mov" &
      rec=$!
      sleep 1
      xcrun simctl launch booted "$bundle" -showcase "$id" >/dev/null
      sleep 6
      kill -INT "$rec"
      wait "$rec" || true
      mkdir -p "$out/$theme/$id"
      ffmpeg -nostdin -loglevel error -y -i "$tmp/$id.mov" -vf "fps=$fps,scale=540:-2" -q:v 4 "$out/$theme/$id/%03d.jpg"
    else
      xcrun simctl launch booted "$bundle" -showcase "$id" >/dev/null
      sleep 4
      xcrun simctl io booted screenshot "$out/$theme/$id.png" >/dev/null
    fi
  done 3<<< "$shots"
done

# The device, from what it drew.
read -r w h < <(sips -g pixelWidth -g pixelHeight "$(ls "$out"/light/*.png | head -1)" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}')
printf '{"name":"%s","width":%s,"height":%s,"corner":165,"cutout":"island"}\n' "$device" "$w" "$h" > "$out/device.json"
