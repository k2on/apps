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
      # Opened and settled first, so the launch is never recorded; then
      # told to move, by the Darwin notification the showcase listens for.
      xcrun simctl launch booted "$bundle" -showcase "$id" >/dev/null
      sleep 4
      xcrun simctl io booted recordVideo --codec=h264 --force "$tmp/$id.mov" &
      rec=$!
      sleep 1
      xcrun simctl spawn booted notifyutil -p dev.arkui.showcase.go
      sleep 4
      kill -INT "$rec"
      wait "$rec" || true
      ffmpeg -nostdin -loglevel error -y -i "$tmp/$id.mov" -vf "fps=$fps,scale=540:-2:flags=lanczos,format=yuv420p" -an -c:v libx264 -preset slow -crf 24 -profile:v high -movflags +faststart "$out/$theme/$id.mp4"
      # What the device itself drew: the MP4 is $fps a second, a frame held
      # where the device drew none.
      echo "  recorded $(ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames -of csv=p=0 "$tmp/$id.mov") frames in $(ffprobe -v error -show_entries format=duration -of csv=p=0 "$tmp/$id.mov")s"
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
