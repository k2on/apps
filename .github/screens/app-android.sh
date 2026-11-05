#!/usr/bin/env bash
# An app's screenshots from a booted emulator (apps.md), the same for every
# app: what is the app's is its <app>/screens/shots.json. The APK installed
# as it is; signed in, if the app has a server, as a person signs in through
# arkui's onboarding — the address typed, Continue, Sign in, the server's
# dev sign-in page in the browser given the name shots.json says, a
# permission asked allowed; then, in light and in dark, each shot: the app
# opened, its steps done, the screen taken. Nothing in the app knows.
#
#   app-android.sh <out> <apk> <app> [server URL]
#
# A step is {"tap": "<label>"} — what says it or its id, in the screen's
# accessibility tree, as uiautomator reads it (ui.py) — with an optional
# "role" ("tab", "button", "field", "text") to prefer when several say
# it, and "where": "bottom" for the lowest of them on the screen; {"type": "<text>"}; or {"wait": <seconds>}. Writes <out>/device.json,
# <out>/<theme>/<id>.png, and beside each the tree it was taken from,
# <id>.tree.txt (ui.py --tree), which is what a step can name.
set -euo pipefail
out=$1
apk=$2
app=$3
server=${4:-}
here=$(dirname "$0")
shots="$app/screens/shots.json"
pkg=$(jq -r '.android' "$shots")
user=$(jq -r '.server.user // empty' "$shots")
mkdir -p "$out"

# The screen as uiautomator reads it.
dump() { adb exec-out uiautomator dump /dev/tty 2>/dev/null || true; }

# Tap what says `text` (or, as `--field`, the first text field), waiting
# up to `seconds` for it to be there; of several, the `role` one.
tap() {
  local text=$1 seconds=${2:-20} role=${3:-} where=${4:-} at
  for _ in $(seq 1 "$seconds"); do
    if at=$(dump | python3 -I "$here/ui.py" "$text" "$role" "$where"); then
      # shellcheck disable=SC2086
      adb shell input tap $at
      return 0
    fi
    sleep 1
  done
  echo "::error::$app: nothing on the screen says '$text'"
  adb exec-out screencap -p > "$out/stuck.png" || true
  dump | python3 -I "$here/ui.py" --tree > "$out/stuck.tree.txt" || true
  return 1
}

# Whether something on the screen says `text`, now.
says() { dump | python3 -I "$here/ui.py" "$1" >/dev/null; }

# A step of a shot, as JSON.
step() {
  local s=$1
  if jq -e 'has("tap")' <<< "$s" > /dev/null; then tap "$(jq -r .tap <<< "$s")" 20 "$(jq -r '.role // ""' <<< "$s")" "$(jq -r '.where // ""' <<< "$s")"
  elif jq -e 'has("type")' <<< "$s" > /dev/null; then adb shell input text "$(jq -r .type <<< "$s" | sed 's/ /%s/g')"
  elif jq -e 'has("wait")' <<< "$s" > /dev/null; then sleep "$(jq -r .wait <<< "$s")"
  else echo "::error::$app: no such step: $s"; return 1
  fi
}

open_app() {
  adb shell am force-stop "$pkg"
  adb shell am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS >/dev/null
  adb shell monkey -p "$pkg" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1
  sleep 4
}

adb install -r "$apk" >/dev/null
adb shell pm clear "$pkg" >/dev/null
read -r w h < <(adb shell wm size | tr -d '\r' | awk -F'[ x]' '/Physical/ {print $3, $4}')
printf '{"name":"Pixel 7","width":%s,"height":%s,"corner":110,"cutout":"hole"}\n' "$w" "$h" > "$out/device.json"

# As android.sh: no "isn't responding" dialog, a quiet status bar.
adb shell settings put global hide_error_dialogs 1
adb shell settings put global sysui_demo_allowed 1
demo() { adb shell am broadcast -a com.android.systemui.demo -e command "$@" >/dev/null; }
demo enter
demo clock -e hhmm 1200
demo battery -e level 100 -e plugged false
demo network -e wifi show -e level 4
demo notifications -e visible false

# Signed in once, through arkui's onboarding; the login is kept for every
# shot after.
if [ -n "$server" ]; then
  # Chrome straight to the page: no first run, no sign-in to Google.
  adb shell 'echo "_ --disable-fre --no-default-browser-check --no-first-run" > /data/local/tmp/chrome-command-line'
  adb shell am set-debug-app --persistent com.android.chrome >/dev/null 2>&1 || true
  open_app
  tap --field
  sleep 1
  adb shell input text "$server"
  tap Continue
  # The server checked, then its sign-in: an app may open the browser by
  # itself, or wait for the button.
  for _ in $(seq 1 20); do
    if says "Sign in"; then tap "Sign in" 2 || true; break; fi
    says "a name" && break
    sleep 1
  done
  # The server's dev sign-in page: a name, sent.
  tap --field 30
  sleep 1
  adb shell input text "$user"
  adb shell input keyevent 66
  # Back in the app; a permission it asks for is allowed, once; then
  # whatever its onboarding says until the server's page and Allow are gone.
  for _ in $(seq 1 30); do
    if says Allow; then tap Allow 2 || true; fi
    says "a name" || says "Server address" || says Allow || break
    sleep 1
  done
  sleep 3
fi

for theme in light dark; do
  if [ "$theme" = dark ]; then adb shell cmd uimode night yes >/dev/null; else adb shell cmd uimode night no >/dev/null; fi
  sleep 2
  mkdir -p "$out/$theme"
  while read -r shot <&3; do
    id=$(jq -r .id <<< "$shot")
    echo "$app android $theme $id"
    open_app
    while read -r s; do
      [ -n "$s" ] && step "$s"
    done < <(jq -c '.steps[]?' <<< "$shot")
    sleep 3
    adb exec-out screencap -p > "$out/$theme/$id.png"
    dump | python3 -I "$here/ui.py" --tree > "$out/$theme/$id.tree.txt" || true
  done 3< <(jq -c '.shots[]' "$shots")
done
