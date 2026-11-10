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
# it, "where": "bottom" for the lowest of them on the screen, and "unless":
# "<label>" to skip the step when that is on the screen already (what it
# opens is open), "optional": true to pass over a tap with nothing to tap
# (after a few seconds), and `a|b` for whichever of two is there; {"await": "<label>"} waits for that label to be there (`a|b`, either);
# {"relaunch": true} stops and opens the app again; {"type": "<text>"}; or {"wait": <seconds>}. Writes <out>/device.json,
# <out>/<theme>/<id>.png, and beside each the tree it was taken from,
# <id>.tree.txt (ui.py --tree), which is what a step can name.
#
# Which device it is, as the environment says (apps-android.sh sets it for
# each emulator profile): DEVICE_NAME, what device.json and the phone's
# own name say (a Pixel 7's); CORNER and CUTOUT, how hero.py draws it.
# SIGNED_IN=1: the app installed and signed in already on this emulator
# (the Fold's second posture), so it is neither installed afresh nor
# cleared, and not signed in again.
set -euo pipefail
out=$1
apk=$2
app=$3
server=${4:-}
here=$(dirname "$0")
shots="$app/screens/shots.json"
# The package, as the APK itself says (aapt2, the SDK's newest build tools).
aapt2=$(find "$ANDROID_HOME/build-tools" -name aapt2 -type f | sort -V | tail -1)
pkg=$("$aapt2" dump packagename "$apk")
user=$(jq -r '.server.user // empty' "$shots")
name=${DEVICE_NAME:-Pixel 7}
signed_in=${SIGNED_IN:-}
mkdir -p "$out"

# The activity of the category `$1` (of package `$2`, when given) that MAIN
# opens, `<package>/<activity>`: the app's launcher activity, the launcher's
# HOME.
resolve() {
  adb shell cmd package resolve-activity --brief -a android.intent.action.MAIN -c "$1" ${2:+"$2"} 2>/dev/null | tr -d '\r' | grep / | tail -1
}

# The screen as uiautomator reads it.
dump() { adb exec-out uiautomator dump /dev/tty 2>/dev/null || true; }

# Tap what says `text` (or, as `--field`, the first text field), waiting
# up to `seconds` for it to be there; of several, the `role` one.
# `a|b`: whichever is there. `optional`: nothing there is no failure.
tap() {
  local text=$1 seconds=${2:-20} role=${3:-} where=${4:-} optional=${5:-} at one screen
  local -a each
  IFS='|' read -ra each <<< "$text"
  for _ in $(seq 1 "$seconds"); do
    screen=$(dump)
    for one in "${each[@]}"; do
      if at=$(python3 -I "$here/ui.py" "$one" "$role" "$where" <<< "$screen"); then
        # shellcheck disable=SC2086
        adb shell input tap $at
        return 0
      fi
    done
    sleep 1
  done
  [ -z "$optional" ] || { echo "$app: nothing says '$text'; an optional tap, passed over"; return 0; }
  echo "::error::$app: nothing on the screen says '$text'"
  adb exec-out screencap -p > "$out/stuck.png" || true
  dump | python3 -I "$here/ui.py" --tree > "$out/stuck.tree.txt" || true
  return 1
}

# Whether something on the screen says `text`, now.
says() { dump | python3 -I "$here/ui.py" "$1" >/dev/null; }

# A step of a shot, as JSON.
step() {
  local s=$1 skip
  skip=$(jq -r '.unless // ""' <<< "$s")
  if [ -n "$skip" ] && says "$skip"; then return 0; fi
  if jq -e 'has("relaunch")' <<< "$s" > /dev/null; then
    open_app
  elif jq -e 'has("await")' <<< "$s" > /dev/null; then
    local label one
    label=$(jq -r '.await' <<< "$s")
    # `a|b`: either will do.
    for _ in $(seq 1 20); do
      IFS='|' read -ra each <<< "$label"
      for one in "${each[@]}"; do says "$one" && return 0; done
      sleep 1
    done
    echo "::error::$app: '$label' never came"; return 1
  elif jq -e 'has("tap")' <<< "$s" > /dev/null; then
    if jq -e '.optional == true' <<< "$s" > /dev/null; then
      tap "$(jq -r .tap <<< "$s")" 3 "$(jq -r '.role // ""' <<< "$s")" "$(jq -r '.where // ""' <<< "$s")" optional
    else
      tap "$(jq -r .tap <<< "$s")" 20 "$(jq -r '.role // ""' <<< "$s")" "$(jq -r '.where // ""' <<< "$s")"
    fi
  elif jq -e 'has("type")' <<< "$s" > /dev/null; then adb shell input text "$(jq -r .type <<< "$s" | sed 's/ /%s/g')"
  elif jq -e 'has("wait")' <<< "$s" > /dev/null; then sleep "$(jq -r .wait <<< "$s")"
  else echo "::error::$app: no such step: $s"; return 1
  fi
}

# The window that has the focus now, as each display's contents say it
# ("Window{… <package>/<activity>}", a line a display), or nothing. Of that
# section alone (`dumpsys window displays`): the whole `dumpsys window`
# begins with the state at the last ANR, the focus as it was then (a
# launcher slow on a booting emulator), which read as now said the
# launcher had it while the app was drawn and resumed.
focus() { adb shell dumpsys window displays 2>/dev/null | tr -d '\r' | grep -oE 'mCurrentFocus=Window\{[^}]*\}' || true; }

# Until the window with the focus is package `$1`'s own activity (not its
# splash, not a dialog over it), up to `$2` seconds.
front() {
  for _ in $(seq 1 "${2:-30}"); do
    focus | grep -qF " $1/" && return 0
    sleep 1
  done
  return 1
}

# Why the app is not what is on the screen: what has the focus, what the
# activity manager has resumed, and what crashed (hide_error_dialogs keeps
# a "keeps stopping" off the screen, so only the crash log says so).
why() {
  echo "focus: $(focus)"
  echo "== every focus dumpsys window names, by its section"
  adb shell dumpsys window 2>/dev/null | tr -d '\r' | grep -E '^WINDOW MANAGER|mCurrentFocus|mFocusedApp' || true
  echo "== the activity manager's"
  adb shell dumpsys activity activities 2>/dev/null | tr -d '\r' | grep -E 'ResumedActivity|mFocusedApp' || true
  echo "== crashes"
  adb logcat -d -b crash 2>/dev/null | tail -n 80 || true
  echo "== starts, draws, deaths, ANRs"
  adb logcat -d -s ActivityTaskManager:I ActivityManager:I AndroidRuntime:E 2>/dev/null |
    grep -E 'START u0|Displayed|has died|ANR|FATAL|preloadRecents|setVisibility' | tail -n 60 || true
}

# The app opened afresh as the launcher opens it (its MAIN/LAUNCHER
# activity in a task of its own: `am start -W`, which returns once it has
# drawn), then waited for until its own window has the focus; not a fixed
# four seconds after `monkey`, which says nothing when a launch is lost.
open_app() {
  local said
  adb shell am force-stop "$pkg"
  adb shell am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS >/dev/null
  said=$(adb shell am start -W -a android.intent.action.MAIN -c android.intent.category.LAUNCHER -f 0x10200000 -n "$activity" 2>&1 | tr -d '\r')
  if ! front "$pkg" 30; then
    echo "::error::$app: not in front 30 seconds after it was opened (am start: $(grep -E '^(Status|LaunchState|Error|Warning)' <<< "$said" | tr '\n' ' '))"
    why > "$out/stuck.why.txt" 2>&1 || true
    adb exec-out screencap -p > "$out/stuck.png" || true
    return 1
  fi
  # What it shows first (a list, a cover) a moment to draw.
  sleep 2
}

# The system in theme `$1` (light, dark) with the app stopped, and settled
# before the app is opened: the night mode as `cmd uimode` says it back,
# and the launcher, which the change re-creates (the tablet's and the
# Fold's with their taskbar, slowly), in front again. Told while the app
# was in front, its sign-in only just done, and the app opened two seconds
# on, the launch could be lost under the re-creation: money's dark tablet
# and Fold showed the launcher a minute later (app-screens 42695364285).
theme_set() {
  local want=no
  [ "$1" = dark ] && want=yes
  adb shell am force-stop "$pkg"
  adb shell cmd uimode night "$want" >/dev/null
  for _ in $(seq 1 30); do
    adb shell cmd uimode night 2>/dev/null | tr -d '\r' | grep -qix "night mode: $want" && break
    sleep 1
  done
  front "${home%%/*}" 30 || echo "$app: the launcher not in front after the night mode ($want): $(focus)"
}

if [ -z "$signed_in" ]; then
  adb install -r "$apk" >/dev/null
  adb shell pm clear "$pkg" >/dev/null
fi
activity=$(resolve android.intent.category.LAUNCHER "$pkg")
home=$(resolve android.intent.category.HOME)
[ -n "$activity" ] || { echo "::error::$app: $pkg has no launcher activity"; exit 1; }
# The phone's name, as a Pixel 7 (or the device it is) out of the box has
# it (Settings, About phone, Device name): the emulator's own is its build's
# ("sdk_gphone64…"), which the app's device chip and picker would say.
if [ -n "${DEVICE_NAME:-}" ]; then
  # Quoted for the emulator's shell, which `adb shell` hands its words to
  # joined: the whole name, parentheses and all.
  adb shell settings put global device_name "'$name'"
else
  # The phone's as it has always been set (its shots compared with main's).
  adb shell settings put global device_name "$name"
fi

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
if [ -n "$server" ] && [ -z "$signed_in" ]; then
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
  # The server signs in its own user (ARK_DEV_USER, app-server.sh), and the
  # browser comes straight back; a server that asks instead shows its page,
  # which is given the name.
  for _ in $(seq 1 30); do
    if says "a name"; then
      tap --field 5
      sleep 1
      adb shell input text "$user"
      adb shell input keyevent 66
      break
    fi
    says "Server address" || says "Sign in" || break
    sleep 1
  done
  # Back in the app; a permission it asks for is allowed, once; then
  # whatever its onboarding says until the server's page and Allow are gone.
  for _ in $(seq 1 30); do
    if says Allow; then tap Allow 2 || true; fi
    says "a name" || says "Server address" || says Allow || break
    sleep 1
  done
  sleep 3
fi

# Both themes, or the one THEMES names: CI takes each on an emulator of
# its own, at once (app-screens.yaml).
for theme in ${THEMES:-light dark}; do
  theme_set "$theme"
  mkdir -p "$out/$theme"
  while read -r shot <&3; do
    id=$(jq -r .id <<< "$shot")
    echo "$app $(basename "$out") $theme $id"
    # What the screen was after each step, for the run's results (not the
    # shots): where a shot went other than it was written to.
    steps="${RESULTS:-$out/../steps}/$app/$(basename "$out")/$theme/$id"
    mkdir -p "$steps"
    open_app || { cp "$out/stuck.why.txt" "$steps/0-failed.txt" 2>/dev/null || true; exit 1; }
    n=0
    # The steps on a descriptor of their own: `adb shell` reads standard
    # input, and on it would take every step after the first (a shot of
    # one step never showed it; harken's player, of six, ran only one).
    while read -r s <&4; do
      [ -n "$s" ] || continue
      n=$((n + 1))
      # A step that fails fails the capture (as the iPhone's test does):
      # under `set -e`, a failure inside `a && b` would be passed over.
      step "$s" || { echo "::error::$app $(basename "$out") $theme $id: step $s failed"; { dump | python3 -I "$here/ui.py" --tree; why; } > "$steps/$n-failed.txt" 2>&1 || true; exit 1; }
      { echo "$s"; dump | python3 -I "$here/ui.py" --tree; } > "$steps/$n.txt" 2>&1 || true
    done 4< <(jq -c '.steps[]?' <<< "$shot")
    sleep 3
    { echo "before the picture"; dump | python3 -I "$here/ui.py" --tree; } > "$steps/shot.txt" 2>&1 || true
    adb exec-out screencap -p > "$out/$theme/$id.png"
    dump | python3 -I "$here/ui.py" --tree > "$out/$theme/$id.tree.txt" || true
  done 3< <(jq -c '.shots[]' "$shots")
done

# The device, from what it drew: the screen as it was taken (a tablet's
# landscape, a Fold's open or folded), not the panel's own size.
first=$(find "$out" -mindepth 2 -maxdepth 2 -name '*.png' | sort | head -1)
read -r w h < <(python3 -I -c 'import struct, sys; d = open(sys.argv[1], "rb").read(24); print(*struct.unpack(">II", d[16:24]))' "$first")
printf '{"name":"%s","width":%s,"height":%s,"corner":%s,"cutout":"%s"}\n' "$name" "$w" "$h" "${CORNER:-110}" "${CUTOUT:-hole}" > "$out/device.json"
