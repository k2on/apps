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
# {"relaunch": true} stops and opens the app again; {"type": "<text>"}; or {"wait": <seconds>}.
# "hold": true on a tap presses it long (a context menu, as a finger opens
# one). A shot's "args", its launch arguments as the iPhone's are given
# (`["-screen", "budget"]`), are the intent's string extras here, each
# `-<key> <value>` pair `--es <key> <value>` (the app reads
# `intent.getStringExtra("screen")`), every time the app is opened for that
# shot; signing in opens it with none. Writes <out>/device.json,
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

# The app's ready signal (ArkUI's `Ready`, ui/docs/shots.md): launched with
# `-shots 1`, it says `ready <generation> <route>` in the log once its
# screen is what it will stay, and `busy ...` whenever it may change. Read
# as it comes, on a descriptor of its own: nothing here waits for a time,
# and each wait's limit only fails it.
logcat_fd=
gen=-1
listen() {
  adb logcat -c
  exec {logcat_fd}< <(adb logcat -v raw -s ArkReady:I)
}
# What the log said since, passed over: the app was stopped, and what its
# last process said is not the next one's.
forget() {
  local line
  while read -r -t 0.01 line <&"$logcat_fd"; do :; done
  gen=-1
}
# Until the app says it is ready at a generation after `gen` (up to a
# minute, which fails): `gen` is then that one.
ready() {
  local word g rest until=$((SECONDS + 60))
  while [ "$SECONDS" -lt "$until" ]; do
    IFS=' ' read -r -t $((until - SECONDS)) word g rest <&"$logcat_fd" || break
    if [ "$word" = ready ] && [ "${g:-0}" -gt "$gen" ]; then gen=$g; return 0; fi
  done
  echo "::error::$app: not ready (after generation $gen) within a minute"
  return 1
}

# Tap what says `text` (or, as `--field`, the first text field), waiting
# up to `seconds` for it to be there; of several, the `role` one.
# `a|b`: whichever is there. `optional`: nothing there is no failure.
# HOLD=1: held for a second and a half (a long press: a row's menu).
tap() {
  local text=$1 seconds=${2:-20} role=${3:-} where=${4:-} optional=${5:-} hold=${6:-} at one screen
  local -a each
  IFS='|' read -ra each <<< "$text"
  # In a shot the screen is ready when a tap is looked for: what it shows
  # is all there is, so it is read once (SHOTS). Signing in, the browser's
  # pages are not the app's, and are looked at until they come.
  [ -z "${SHOTS:-}" ] || seconds=1
  for _ in $(seq 1 "$seconds"); do
    screen=$(dump)
    for one in "${each[@]}"; do
      if at=$(python3 -I "$here/ui.py" "$one" "$role" "$where" <<< "$screen"); then
        # A long press: a swipe that does not move, held a second.
        # shellcheck disable=SC2086
        if [ -n "${HOLD:-}" ] || [ -n "$hold" ]; then
          read -r x y <<< "$at"
          adb shell input swipe "$x" "$y" "$x" "$y" 1500
        else
          adb shell input tap $at
        fi
        return 0
      fi
    done
    [ -n "${SHOTS:-}" ] || sleep 1
  done
  [ -z "$optional" ] || { echo "$app: nothing says '$text'; an optional tap, passed over"; return 0; }
  echo "::error::$app: nothing on the screen says '$text'"
  adb exec-out screencap ${SCREENCAP_DISPLAY:+-d "$SCREENCAP_DISPLAY"} -p > "$out/stuck.png" || true
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
    open_app "${at_args:-}"
  elif jq -e 'has("await")' <<< "$s" > /dev/null; then
    local label one
    label=$(jq -r '.await' <<< "$s")
    # `a|b`: either will do; looked for each time the app is ready again.
    for _ in $(seq 1 20); do
      IFS='|' read -ra each <<< "$label"
      for one in "${each[@]}"; do says "$one" && return 0; done
      ready || break
    done
    echo "::error::$app: '$label' never came"; return 1
  elif jq -e 'has("tap")' <<< "$s" > /dev/null; then
    local hold
    hold=$(jq -r 'if .hold == true then "hold" else "" end' <<< "$s")
    if jq -e '.optional == true' <<< "$s" > /dev/null; then
      tap "$(jq -r .tap <<< "$s")" 3 "$(jq -r '.role // ""' <<< "$s")" "$(jq -r '.where // ""' <<< "$s")" optional "$hold"
    else
      tap "$(jq -r .tap <<< "$s")" 20 "$(jq -r '.role // ""' <<< "$s")" "$(jq -r '.where // ""' <<< "$s")" "" "$hold"
    fi
    ready
  elif jq -e 'has("press")' <<< "$s" > /dev/null; then
    HOLD=1 tap "$(jq -r .press <<< "$s")" 20 "$(jq -r '.role // ""' <<< "$s")" "$(jq -r '.where // ""' <<< "$s")"
    ready
  elif jq -e 'has("type")' <<< "$s" > /dev/null; then
    adb shell input text "$(jq -r .type <<< "$s" | sed 's/ /%s/g')"
    ready
  # Not slept: every step is followed by the app's ready signal instead.
  elif jq -e 'has("wait")' <<< "$s" > /dev/null; then :
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
# Given launch arguments (a shot of shots.json's `screens`), they go with
# it as the intent's string array `ark.args` (ark_app::shell::Launch), and
# each `-key value` pair as a string extra too (`extras`, set per shot).
extras=()
open_app() {
  local said extra=()
  # `-shots 1` first: no motion, and the ready signal (ArkUI's Shots.kt).
  extra=(--esa ark.args "-shots,1${1:+,$1}")
  adb shell am force-stop "$pkg"
  forget
  adb shell am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS >/dev/null
  said=$(adb shell am start -W -a android.intent.action.MAIN -c android.intent.category.LAUNCHER -f 0x10200000 -n "$activity" ${extra[@]+"${extra[@]}"} ${extras[@]+"${extras[@]}"} 2>&1 | tr -d '\r')
  if ! front "$pkg" 30; then
    echo "::error::$app: not in front 30 seconds after it was opened (am start: $(grep -E '^(Status|LaunchState|Error|Warning)' <<< "$said" | tr '\n' ' '))"
    why > "$out/stuck.why.txt" 2>&1 || true
    adb exec-out screencap ${SCREENCAP_DISPLAY:+-d "$SCREENCAP_DISPLAY"} -p > "$out/stuck.png" || true
    return 1
  fi
  # What it shows first (a list, a cover), drawn: its ready signal.
  ready
}

# How a shot's `route` reaches the app: the one place that says it, so a
# router's own spelling (a whole URL) is a change here alone. A link to the
# running app (VIEW, its package), no relaunch, then its ready signal.
scheme=$(jq -r '.scheme // empty' "$shots")
# Whether the app opens a screen's launch arguments by a link as it runs.
links=$(jq -r '.links == true' "$shots")
open_route() {
  local url=$1
  case $url in *://*) ;; *) url="${scheme:-$app}://$url" ;; esac
  adb shell am start -a android.intent.action.VIEW -d "'$url'" "$pkg" > /dev/null
  ready
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

# The launcher's home laid out before the app is installed: an app
# installed while a fresh emulator's launcher still lays out its default
# home took its place in the taskbar before or after Messages as it fell
# (the tablet's shots of two runs, the same but for that). Its home
# brought up, and its screen read until it reads the same twice running.
launcher_settled() {
  local h prev="" now
  h=$(resolve android.intent.category.HOME || true)
  [ -n "$h" ] || return 0
  adb shell am start -W -a android.intent.action.MAIN -c android.intent.category.HOME > /dev/null 2>&1 || true
  front "${h%%/*}" 30 || true
  for _ in $(seq 1 30); do
    # Never fatal (pipefail, set -e): a screen ui.py cannot read (the
    # Fold's two displays) is only a screen that reads as nothing.
    now=$( { dump | python3 -I "$here/ui.py" --tree 2> /dev/null || true; } | md5sum)
    [ "$now" = "$prev" ] && return 0
    prev=$now
    sleep 1
  done
  echo "$app: the launcher's home still moving after 30 seconds; installing anyway"
}

if [ -z "$signed_in" ]; then
  launcher_settled
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
# The system's "Viewing full screen" told as seen already, for every app: in
# shots mode the app's window hides the navigation bars (ui/kotlin/shots),
# and a first hiding brought that confirmation up over the first launch
# (dry run 42984084410).
adb shell settings put secure immersive_mode_confirmations confirmed
# The system's own motion off too (the app's is, in shots mode): the status
# bar's icons recoloured for a new theme and the gesture bar's shade, each
# eased in by SystemUI, were drawn part way in a dark run's shots and not in
# another's (dry runs 42897638770 and 42909207650: rows 51-83 and the
# handle differed, the app's own pixels the same).
for scale in animator_duration_scale transition_animation_scale window_animation_scale; do
  adb shell settings put global "$scale" 0
done
adb shell settings put global sysui_demo_allowed 1
demo() { adb shell am broadcast -a com.android.systemui.demo -e command "$@" >/dev/null; }
demo enter
demo clock -e hhmm 1200
demo battery -e level 100 -e plugged false
demo network -e wifi show -e level 4
demo notifications -e visible false
# Every icon of the status bar said, so none changes under a shot (two runs'
# shots differed in the Wi-Fi and the clock's corner, rows 51-83).
demo network -e mobile hide -e wifi show -e level 4 -e fully true
demo status -e volume hide -e bluetooth hide -e location hide -e alarm hide -e sync hide -e tty hide -e eri hide -e mute hide -e speakerphone hide -e zen hide

# The app's ready signal, read from here on.
listen

# Signed in once, through arkui's onboarding; the login is kept for every
# shot after. Its waits are the browser's and the system's, which are not
# the app's to say; no shot is taken from them. The address typed (cleared
# and typed again until the field says it: a tablet's first tap could land before the
# field took the keys, and Continue then went with nothing, app-screens
# 42977527249's money and 43052816932's harken, both on the tablet), Continue,
# Sign in, the server's page if it asks, a permission allowed. Each phase
# said with its time.
sign_in() {
  local t=$SECONDS
  open_app
  for try in 1 2 3; do
    tap --field
    sleep 1
    # What a try before left in it, gone (a backspace for each character).
    [ "$try" = 1 ] || adb shell input keyevent KEYCODE_MOVE_END $(printf '67 %.0s' $(seq 1 64))
    adb shell input text "$server"
    says "$server" && break
    echo "$app: the address did not reach the field (try $try)"
  done
  tap Continue
  echo "  sign-in: the address sent ($((SECONDS - t))s)"
  # The server checked, then its sign-in: an app may open the browser by
  # itself, or wait for the button.
  for _ in $(seq 1 20); do
    if says "Sign in"; then tap "Sign in" 2 || true; break; fi
    says "a name" && break
    sleep 1
  done
  echo "  sign-in: to the server's page ($((SECONDS - t))s)"
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
  echo "  sign-in: back in the app ($((SECONDS - t))s)"
}

if [ -n "$server" ] && [ -z "$signed_in" ]; then
  # Chrome straight to the page: no first run, no sign-in to Google.
  adb shell 'echo "_ --disable-fre --no-default-browser-check --no-first-run" > /data/local/tmp/chrome-command-line'
  adb shell am set-debug-app --persistent com.android.chrome >/dev/null 2>&1 || true
  sign_in
  # Still at the onboarding's first page: what it showed kept, and once more.
  if says "Server address"; then
    echo "::warning::$app: still not signed in; signing in once more (signin-stuck.png is what was there)"
    adb exec-out screencap ${SCREENCAP_DISPLAY:+-d "$SCREENCAP_DISPLAY"} -p > "$out/signin-stuck.png" || true
    dump | python3 -I "$here/ui.py" --tree > "$out/signin-stuck.tree.txt" || true
    sign_in
  fi
fi
# Every shot from here on: a tap is looked for once, on a ready screen.
SHOTS=1

# Both themes, or the one THEMES names: CI takes each on an emulator of
# its own, at once (app-screens.yaml).
for theme in ${THEMES:-light dark}; do
  theme_set "$theme"
  # Stopped by the theme: the next shot launches it.
  running=
  at_args=
  mkdir -p "$out/$theme"
  while read -r shot <&3; do
    id=$(jq -r .id <<< "$shot")
    echo "$app $(basename "$out") $theme $id"
    # What the screen was after each step, for the run's results (not the
    # shots): where a shot went other than it was written to.
    steps="${RESULTS:-$out/../steps}/$app/$(basename "$out")/$theme/$id"
    mkdir -p "$steps"
    # Its launch arguments: `-key value` pairs, as string extras.
    extras=()
    while read -r k v; do
      [ -n "$k" ] && extras+=(--es "'${k#-}'" "'$v'")
    done < <(jq -r '(.args // []) as $a | range(0; $a | length; 2) as $i | "\($a[$i]) \($a[$i + 1] // "")"' <<< "$shot")
    route=$(jq -r '.route // empty' <<< "$shot")
    args=$(jq -r '.args // [] | join(",")' <<< "$shot")
    began=$(date +%s.%N)
    # In the app as it runs, one shot after another: launched afresh only
    # for the theme's first shot, one that says it needs a clean start
    # (`fresh`), or one whose launch arguments the app takes only at
    # launch; a screen's arguments otherwise as a launch link (`links`:
    # the app's shell puts back what it showed, then opens that screen),
    # its route as a link, or its steps from where the last left it.
    if [ -z "${running:-}" ] || jq -e '.fresh == true' <<< "$shot" > /dev/null ||
      { [ -n "$args" ] && [ "$links" != true ] && [ "$args" != "$at_args" ]; }; then
      open_app "$args" || { cp "$out/stuck.why.txt" "$steps/0-failed.txt" 2>/dev/null || true; exit 1; }
      running=1
      at_args=$args
      how=launched
    elif [ -n "$args" ]; then
      open_route "$(jq -r '.args as $a | [range(0; $a | length; 2) as $i | "\($a[$i] | sub("^-+"; ""))=\($a[$i + 1] // "" | @uri)"] | "?" + join("&")' <<< "$shot")" || exit 1
      at_args=$args
      how=linked
    else
      how=continued
    fi
    # Where the router can take it: in the app as it runs.
    [ -z "$route" ] || open_route "$route" || exit 1
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
    # The screen is ready: each step, and the launch, waited for it.
    # The tree the picture is taken from, read once: the run's results'
    # "before the picture" and the shot's own <id>.tree.txt (each was a
    # dump of its own, two seconds apiece on the emulator).
    tree() {
      dump | python3 -I "$here/ui.py" --tree > "$out/$theme/$id.tree.txt" || true
      { echo "before the picture"; cat "$out/$theme/$id.tree.txt"; } > "$steps/shot.txt"
    }
    tree
    adb exec-out screencap ${SCREENCAP_DISPLAY:+-d "$SCREENCAP_DISPLAY"} -p > "$out/$theme/$id.png"
    # A picture, or the capture fails: a warning written in front of one
    # (screencap on two displays, none named) is no PNG.
    if [ "$(head -c 8 "$out/$theme/$id.png" | od -An -tx1 | tr -d ' \n')" != 89504e470d0a1a0a ]; then
      echo "::error::$app $(basename "$out") $theme $id: the screenshot is not a PNG ($(head -c 60 "$out/$theme/$id.png" | tr -cd '[:print:]'))"
      exit 1
    fi
    echo "  $id: $how, $(( $(jq -r '.steps // [] | length' <<< "$shot") )) steps, $(awk -v a="$began" -v b="$(date +%s.%N)" 'BEGIN { printf "%.1f", b - a }')s"
  done 3< <(jq -c --arg k "$(basename "$out")" '.shots[], (.screens // [] | .[] | select((.on // []) | index($k)))' "$shots")
done

# Each shot its own screen: none the same picture as another.
bash "$here/distinct.sh" "$out" || exit 1

# The device, from what it drew: the screen as it was taken (a tablet's
# landscape, a Fold's open or folded), not the panel's own size.
first=$(find "$out" -mindepth 2 -maxdepth 2 -name '*.png' | sort | head -1)
read -r w h < <(python3 -I -c 'import struct, sys; d = open(sys.argv[1], "rb").read(24); print(*struct.unpack(">II", d[16:24]))' "$first")
printf '{"name":"%s","width":%s,"height":%s,"corner":%s,"cutout":"%s"}\n' "$name" "$w" "$h" "${CORNER:-110}" "${CUTOUT:-hole}" > "$out/device.json"
