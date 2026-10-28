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

# The recorder (record.swift): every frame the simulator composes, from its
# framebuffer, in place of simctl's recordVideo, which keeps a sixth of them.
xcrun swiftc -O .github/screens/record.swift -o "$tmp/record"
udid=$(xcrun simctl list devices booted -j | jq -r '[.devices[][] | select(.state == "Booted")][0].udid')

xcrun simctl install booted "$app"
xcrun simctl status_bar booted override --time 9:41 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3
mkdir -p "$out"

shots=$(jq -r '.components[].shots[] | select(.phones | index("ios")) | "\(.id) \(.kind)"' docs/site/catalogue.json)

# How many times slower than life a shot is taken, then sped back up
# (encode.sh), so its motion has $fps frames a second whatever the
# simulator composes (a tenth of that, for the system's zoom): time slowed
# in the showcase for the frameworks that animate (timewarp.c, loaded by
# SIMCTL_CHILD_DYLD_INSERT_LIBRARIES, from the moment it is told to go),
# and its own pauses stretched to match (`-slow`, ShowcaseApp.swift).
slow=${IOS_SLOW:-20}
xcrun -sdk iphonesimulator clang -arch arm64 -mios-simulator-version-min=17.0 -O2 -Wall \
  -dynamiclib -o "$tmp/timewarp.dylib" .github/screens/timewarp.c
codesign --force --sign - "$tmp/timewarp.dylib"

# Record shot `id` to `to`: opened and left to settle, so its launch is
# never in it, then told to move (the Darwin notification the showcase and
# timewarp listen for), then made a page's MP4.
record() {
  local id=$1 to=$2
  SIMCTL_CHILD_DYLD_INSERT_LIBRARIES="$tmp/timewarp.dylib" SIMCTL_CHILD_TIMEWARP="$slow" \
    xcrun simctl launch booted "$bundle" -showcase "$id" -slow "$slow" >/dev/null
  sleep 7
  "$tmp/record" "$udid" $((3 * slow + 2)) 540 "$tmp/$id.mov" | sed 's/^/  /' &
  local rec=$!
  sleep 1
  xcrun simctl spawn booted notifyutil -p dev.arkui.showcase.go
  wait "$rec"
  xcrun simctl terminate booted "$bundle" 2>/dev/null || true
  sed 's/^/  /' "$(xcrun simctl get_app_container booted "$bundle" data)/Documents/frames.log" 2>/dev/null || true
  bash .github/screens/encode.sh "$tmp/$id.mov" "$to" "$slow" "$fps"
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
      record "$id" "$out/$theme/$id.mp4"
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
