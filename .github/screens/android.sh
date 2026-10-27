#!/usr/bin/env bash
# The docs' Android captures, from a booted emulator: every shot
# rust/docs/catalogue.json says Android has, in light and in dark, from the
# debug build's showcase (ShowcaseActivity). A screenshot is a PNG a few
# seconds after the shot opens; a recording is the screen recorded while it
# moves by itself, cut into JPEG frames.
#
#   android.sh <out> <apk>
#
# Writes <out>/device.json, <out>/<theme>/<id>.png and <out>/<theme>/<id>/NNN.jpg.
set -euo pipefail
out=$1
apk=$2
fps=${FPS:-10}
pkg=dev.harken.android
tmp=$(mktemp -d)

adb install -r "$apk" >/dev/null
read -r w h < <(adb shell wm size | tr -d '\r' | awk -F'[ x]' '/Physical/ {print $3, $4}')
mkdir -p "$out"
printf '{"name":"Pixel 7","width":%s,"height":%s,"corner":110,"cutout":"hole"}\n' "$w" "$h" > "$out/device.json"

# A quiet status bar: noon, full, no notifications.
adb shell settings put global sysui_demo_allowed 1
demo() { adb shell am broadcast -a com.android.systemui.demo -e command "$@" >/dev/null; }
demo enter
demo clock -e hhmm 1200
demo battery -e level 100 -e plugged false
demo network -e wifi show -e level 4
demo notifications -e visible false

shots=$(jq -r '.components[].shots[] | select(.phones | index("android")) | "\(.id) \(.kind)"' rust/docs/catalogue.json)

for theme in light dark; do
  if [ "$theme" = dark ]; then adb shell cmd uimode night yes >/dev/null; else adb shell cmd uimode night no >/dev/null; fi
  sleep 2
  mkdir -p "$out/$theme"
  while read -r id kind <&3; do
    [ -n "$id" ] || continue
    echo "android $theme $id ($kind)"
    adb shell am force-stop "$pkg"
    if [ "$kind" = recording ]; then
      adb shell rm -f /sdcard/shot.mp4
      adb shell screenrecord --time-limit 6 /sdcard/shot.mp4 &
      rec=$!
      sleep 1
      adb shell am start -W -n "$pkg/.ShowcaseActivity" --es shot "$id" >/dev/null
      wait "$rec" || true
      sleep 1
      adb pull /sdcard/shot.mp4 "$tmp/$id.mp4" >/dev/null
      mkdir -p "$out/$theme/$id"
      ffmpeg -nostdin -loglevel error -y -i "$tmp/$id.mp4" -vf "fps=$fps,scale=540:-2" -q:v 4 "$out/$theme/$id/%03d.jpg"
    else
      adb shell am start -W -n "$pkg/.ShowcaseActivity" --es shot "$id" >/dev/null
      sleep 3
      adb exec-out screencap -p > "$out/$theme/$id.png"
    fi
  done 3<<< "$shots"
done
