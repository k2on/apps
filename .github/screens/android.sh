#!/usr/bin/env bash
# The docs' Android captures, from a booted emulator: every shot
# docs/site/catalogue.json says Android has, in light and in dark, from the
# showcase app (showcase/android, ShowcaseActivity). A screenshot is a PNG a few
# seconds after the shot opens; a recording is the screen recorded while the
# open shot moves, once told to, as an animated WebP, which a page plays in an <img>.
#
#   android.sh <out> <apk>
#
# Writes <out>/device.json, <out>/<theme>/<id>.png and <out>/<theme>/<id>.webp.
set -euo pipefail
out=$1
apk=$2
fps=${FPS:-10}
pkg=dev.arkui.showcase
tmp=$(mktemp -d)

adb install -r "$apk" >/dev/null
read -r w h < <(adb shell wm size | tr -d '\r' | awk -F'[ x]' '/Physical/ {print $3, $4}')
mkdir -p "$out"
printf '{"name":"Pixel 7","width":%s,"height":%s,"corner":110,"cutout":"hole"}\n' "$w" "$h" > "$out/device.json"

# No "isn't responding" dialog over a capture: a slow emulator's launcher
# otherwise gets one, and it lands in the picture.
adb shell settings put global hide_error_dialogs 1
adb shell am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS >/dev/null

# A quiet status bar: noon, full, no notifications.
adb shell settings put global sysui_demo_allowed 1
demo() { adb shell am broadcast -a com.android.systemui.demo -e command "$@" >/dev/null; }
demo enter
demo clock -e hhmm 1200
demo battery -e level 100 -e plugged false
demo network -e wifi show -e level 4
demo notifications -e visible false

shots=$(jq -r '.components[].shots[] | select(.phones | index("android")) | "\(.id) \(.kind)"' docs/site/catalogue.json)

for theme in light dark; do
  if [ "$theme" = dark ]; then adb shell cmd uimode night yes >/dev/null; else adb shell cmd uimode night no >/dev/null; fi
  sleep 2
  mkdir -p "$out/$theme"
  while read -r id kind <&3; do
    [ -n "$id" ] || continue
    echo "android $theme $id ($kind)"
    adb shell am force-stop "$pkg"
    adb shell am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS >/dev/null
    if [ "$kind" = recording ]; then
      # Opened and settled first, so the launch is never recorded; then
      # told to move, by the same shot asked again with `go`.
      adb shell am start -W -n "$pkg/.ShowcaseActivity" --es shot "$id" >/dev/null
      sleep 4
      adb shell rm -f /sdcard/shot.mp4
      adb shell screenrecord --time-limit 4 /sdcard/shot.mp4 &
      rec=$!
      sleep 0.5
      adb shell am start -n "$pkg/.ShowcaseActivity" --es shot "$id" --ez go true >/dev/null
      wait "$rec" || true
      sleep 1
      adb pull /sdcard/shot.mp4 "$tmp/$id.mp4" >/dev/null
      ffmpeg -nostdin -loglevel error -y -i "$tmp/$id.mp4" -vf "fps=$fps,scale=540:-2" -an -c:v libwebp -lossless 0 -q:v 70 -loop 0 "$out/$theme/$id.webp"
    else
      adb shell am start -W -n "$pkg/.ShowcaseActivity" --es shot "$id" >/dev/null
      sleep 3
      adb exec-out screencap -p > "$out/$theme/$id.png"
    fi
  done 3<<< "$shots"
done
