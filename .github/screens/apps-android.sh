#!/usr/bin/env bash
# Each app's Android screenshots in turn (app-android.sh), on the emulator
# that is booted: one that fails is said, and the others are still taken;
# then, if any failed, it fails (as apps-ios.sh), so that the job says so
# and not only its log.
#
#   apps-android.sh <out> <apks> <app>...
#
# <apks> holds apk-<app>/ with the app's APK in it (build.yaml's artifacts);
# an app's server, if its shots.json has one, is reached at the host's
# loopback, which the emulator calls 10.0.2.2.
#
# DEVICE says which emulator profile is booted, and so where its shots go:
#
#   phone   (pixel_7, the default)  <out>/<app>/android/
#   tablet  (pixel_tablet)          <out>/<app>/android-tablet/, in landscape
#   fold    (a foldable)            <out>/<app>/fold-unfolded/, then the same
#                                   app folded, <out>/<app>/fold-folded/
#
# The same APK, the same driver and the same shots on each.
set -uo pipefail
out=$1
apks=$2
shift 2
here=$(dirname "$0")
device=${DEVICE:-phone}
failed=0

# The screen's size as it is drawn now, "<width> <height>".
size() {
  adb exec-out screencap -p | python3 -I -c 'import struct, sys; d = sys.stdin.buffer.read(24); print(*struct.unpack(">II", d[16:24]))'
}

# Whether the screen's size moved from `before`, within ten seconds.
moved() {
  local after
  for _ in $(seq 1 10); do
    sleep 1
    after=$(size)
    [ "$after" = "$1" ] || { sleep 2; echo "  $1 -> $after"; return 0; }
  done
  return 1
}

# The Fold in a posture, `fold` or `unfold`: told by the emulator's console
# (its hinge), then by Android's own device state (CLOSED, OPENED: `cmd
# device_state`, which apps read the posture from). On GitHub's runner the
# emulator's generic foldable takes both and still draws its one screen
# whole, the inner one; then, folded, Android is given the Pixel Fold's
# outer screen's size and density (`wm size`, `wm density`: 1080x2092 at
# 420), and opened, its own again. Fails when the screen it draws did not
# change size. What each said is printed.
posture() {
  local before states name id
  before=$(size)
  echo "$1: the emulator says $(adb emu "$1" 2>&1 | tr -d '\r' | tr '\n' ' ')"
  moved "$before" && return 0
  name=$([ "$1" = fold ] && echo CLOSED || echo OPENED)
  states=$(adb shell cmd device_state print-states 2>&1 | tr -d '\r')
  id=$(grep -o "identifier=[0-9]*, name='$name'" <<< "$states" | grep -o '[0-9]\+' | head -1)
  if [ -n "$id" ]; then
    echo "$1: device state $id ($name) $(adb shell cmd device_state state "$id" 2>&1 | tr -d '\r' | tr '\n' ' ')"
    moved "$before" && return 0
  fi
  if [ "$1" = fold ]; then
    adb shell wm size 1080x2092
    adb shell wm density 420
  else
    adb shell wm size reset
    adb shell wm density reset
  fi
  echo "$1: wm $(adb shell wm size | tr -d '\r' | tr '\n' ' ')"
  moved "$before"
}

case $device in
  tablet)
    # Landscape, the Pixel Tablet's natural way up, and held there.
    adb shell settings put system accelerometer_rotation 0
    adb shell settings put system user_rotation 0
    sleep 2
    read -r w h < <(size)
    if [ "$h" -gt "$w" ]; then adb shell settings put system user_rotation 1; sleep 2; fi
    echo "tablet: $(size)"
    ;;
  fold)
    adb shell settings put system accelerometer_rotation 0
    adb shell settings put system user_rotation 0
    ;;
esac

for a in "$@"; do
  apk=$(find "$apks/apk-$a" -name '*.apk' 2>/dev/null | head -1)
  # Nothing published for it yet: said where it was fetched, and its shots kept.
  [ -n "$apk" ] || { echo "$a: no APK to take; skipped"; continue; }
  server=$(jq -r '.server.port // empty | "http://10.0.2.2:\(.)"' "$a/screens/shots.json")
  case $device in
    phone)
      bash "$here/app-android.sh" "$out/$a/android" "$apk" "$a" "$server" ||
        { echo "::error::$a's Android screenshots failed"; failed=1; }
      ;;
    tablet)
      DEVICE_NAME="Pixel Tablet" CORNER=60 CUTOUT=none \
        bash "$here/app-android.sh" "$out/$a/android-tablet" "$apk" "$a" "$server" ||
        { echo "::error::$a's Android tablet screenshots failed"; failed=1; }
      ;;
    fold)
      # Open first, signed in there; then folded, as it is, the login kept.
      posture unfold || echo "$a: the Fold's screen kept its size: open already"
      if DEVICE_NAME="${FOLD_NAME:-Pixel Fold}" CORNER=60 CUTOUT=hole \
        bash "$here/app-android.sh" "$out/$a/fold-unfolded" "$apk" "$a" "$server"; then
        if posture fold; then
          DEVICE_NAME="${FOLD_NAME:-Pixel Fold}" CORNER=110 CUTOUT=hole SIGNED_IN=1 \
            bash "$here/app-android.sh" "$out/$a/fold-folded" "$apk" "$a" "$server" ||
            { echo "::error::$a's Fold (folded) screenshots failed"; failed=1; }
        else
          echo "::error::$a: the emulator did not fold (its screen kept its size); the folded shots not taken"; failed=1
        fi
      else
        echo "::error::$a's Fold (open) screenshots failed"; failed=1
      fi
      ;;
    *) echo "::error::no such Android device: $device"; exit 1 ;;
  esac
done
exit $failed
