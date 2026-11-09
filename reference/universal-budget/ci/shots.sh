#!/usr/bin/env bash
# The reference app's screenshots (SCREENS.md's recommended captures), each
# in light and dark, on GitHub's macOS runner
# (.github/workflows/reference-shots.yaml).
#
#   shots.sh ios  <out> <Budget.app (simulator)>   iPhone, then iPad portrait and landscape
#   shots.sh mac  <out> <Budget.app (macOS)>       the Mac window, 1440x900
#
# Writes <out>/<device>/<theme>/<screen>.png.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
what=$1
out=$2
app=$3
bundle=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist" 2> /dev/null ||
  /usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")
failed=0

# name|arguments (SCREENS.md; -appearance is added per theme).
iphone=(
  "01-budget|-screen budget"
  "02-category-dining|-screen category:dining"
  "03-accounts|-screen accounts"
  "04-account-visa|-screen account:visa"
  "05-review|-screen review"
  "06-transaction-receipt|-screen transaction:receipt"
  "07-add-transaction|-screen add-transaction"
  "08-insights|-screen insights"
)
ipad_landscape=(
  "01-category-vacation-inspector|-screen category:vacation -inspector 1 -columns all"
  "02-transaction-receipt-inspector|-screen transaction:receipt -inspector 1"
  "03-account-checking|-screen account:checking -columns all"
  "04-insights|-screen insights -columns all"
  "05-edit-transaction-receipt|-screen edit-transaction:receipt"
  "06-settings-household|-screen settings -settingsTab household"
)
ipad_portrait=(
  "01-budget|-screen budget"
  "02-transactions|-screen transactions"
  "03-category-dining-inspector|-screen category:dining -inspector 1"
  "04-review|-screen review"
)
mac=(
  "01-category-vacation-inspector|-screen category:vacation -inspector 1"
  "02-transactions|-screen transactions"
  "03-transaction-receipt-inspector|-screen transaction:receipt -inspector 1"
  "04-account-checking|-screen account:checking"
  "05-review|-screen review"
  "06-insights|-screen insights"
  "07-add-transaction|-screen add-transaction"
  "08-settings-banks|-screen settings -settingsTab banks"
)

# A simulator of the newest iOS whose name matches, booted; its udid printed.
boot() {
  local udid
  udid=$(xcrun simctl list devices available -j | jq -r --arg p "$1" '
    .devices | to_entries | map(select(.key | test("iOS"))) | sort_by(.key) | last | .value
    | map(select(.name | test($p))) | sort_by(.name) | last | .udid // empty')
  [ -n "$udid" ] || { echo "::error::no simulator matching $1" >&2; xcrun simctl list devices available >&2; return 1; }
  xcrun simctl boot "$udid" 2> /dev/null || true
  xcrun simctl bootstatus "$udid" -b > /dev/null
  xcrun simctl status_bar "$udid" override --time 9:41 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3
  echo "$udid"
}

# The simulator in `theme`, as it says back.
appearance() {
  for _ in $(seq 1 30); do
    xcrun simctl ui "$1" appearance "$2"
    [ "$(xcrun simctl ui "$1" appearance)" = "$2" ] && break
    sleep 2
  done
  sleep 2
}

# A simulator's screen once it is still: two seconds, then the same picture
# twice in a row a second apart (or what is there after twelve seconds).
still() {
  local udid=$1 to=$2 tmp prev="" now
  tmp=$(mktemp -d)
  sleep 2
  for i in $(seq 1 10); do
    xcrun simctl io "$udid" screenshot --type=png "$tmp/$i.png" > /dev/null 2>&1
    now=$(md5 -q "$tmp/$i.png")
    if [ "$now" = "$prev" ]; then break; fi
    prev=$now
    sleep 1
  done
  mv "$tmp/$i.png" "$to"
  rm -rf "$tmp"
}

# Portrait shots on a booted simulator: launched with each shot's arguments.
portrait() {
  local udid=$1 device=$2
  shift 2
  for theme in light dark; do
    appearance "$udid" "$theme"
    mkdir -p "$out/$device/$theme"
    for shot in "$@"; do
      name=${shot%%|*}
      read -ra args <<< "${shot#*|}"
      echo "$device $theme $name"
      xcrun simctl terminate "$udid" "$bundle" > /dev/null 2>&1 || true
      if ! xcrun simctl launch "$udid" "$bundle" "${args[@]}" -appearance "$theme" > /dev/null; then
        echo "::error::$device $theme $name: did not launch"; failed=1; continue
      fi
      still "$udid" "$out/$device/$theme/$name.png"
      # Crashed on the way: said, and the picture kept for what it shows.
      if ! xcrun simctl spawn "$udid" launchctl list 2> /dev/null | grep -q "UIKitApplication:$bundle"; then
        echo "::warning::$device $theme $name: the app was not running at the shot (crashed?)"
      fi
    done
    xcrun simctl terminate "$udid" "$bundle" > /dev/null 2>&1 || true
  done
}

case $what in
ios)
  udid=$(boot '^iPhone 17 Pro$') || exit 1
  xcrun simctl install "$udid" "$app"
  portrait "$udid" iphone "${iphone[@]}"
  xcrun simctl shutdown all || true

  udid=$(boot '^iPad Pro 13-inch') || exit 1
  xcrun simctl install "$udid" "$app"
  portrait "$udid" ipad-portrait "${ipad_portrait[@]}"

  # On its side, through XCUIDevice (landscape/Shots.swift).
  derived=$(mktemp -d)
  (cd "$here/landscape" && xcodegen generate > /dev/null)
  xcodebuild build-for-testing -project "$here/landscape/Landscape.xcodeproj" -scheme Shots \
    -destination "id=$udid" -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO -quiet || exit 1
  for theme in light dark; do
    appearance "$udid" "$theme"
    list=""
    for shot in "${ipad_landscape[@]}"; do list+="$shot -appearance $theme"$'\n'; done
    echo "ipad-landscape $theme"
    TEST_RUNNER_SHOTS_BUNDLE="$bundle" TEST_RUNNER_SHOTS_LIST="$list" TEST_RUNNER_SHOTS_OUT="$out/ipad-landscape/$theme" \
      xcodebuild test-without-building -project "$here/landscape/Landscape.xcodeproj" -scheme Shots \
        -destination "id=$udid" -derivedDataPath "$derived" > "$out/ipad-landscape-$theme.log" 2>&1 ||
      { echo "::error::iPad landscape ($theme) failed"; tail -n 40 "$out/ipad-landscape-$theme.log"; failed=1; }
  done
  rm -f "$out"/ipad-landscape-*.log
  # XCUITest's screenshot is in the screen's own (portrait) frame: turned
  # back the way the device was, landscape left (app-ios.sh).
  for png in "$out"/ipad-landscape/*/*.png; do
    [ -f "$png" ] || continue
    read -r w h < <(sips -g pixelWidth -g pixelHeight "$png" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}')
    if [ "$h" -gt "$w" ]; then sips -r 270 "$png" > /dev/null; fi
  done
  mkdir -p "$out/trees"
  for t in "$out"/ipad-landscape/*/*.tree.txt; do
    [ -f "$t" ] || continue
    mv "$t" "$out/trees/ipad-landscape-$(basename "$(dirname "$t")")-$(basename "$t")"
  done
  xcrun simctl shutdown all || true
  ;;
mac)
  exe="$app/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")"
  tmp=$(mktemp -d)
  # A process's window id (.github/screens/window.swift).
  xcrun swiftc -O "$repo/.github/screens/window.swift" -o "$tmp/window"
  dark_mode() {
    osascript -e "tell application \"System Events\" to tell appearance preferences to set dark mode to $1" &
    local asked=$!
    for _ in $(seq 1 20); do
      kill -0 "$asked" 2> /dev/null || { wait "$asked"; return; }
      sleep 1
    done
    kill "$asked" 2> /dev/null || true
    echo "::warning::System Events did not answer: the system appearance is unchanged"
  }
  for theme in light dark; do
    dark=false
    [ "$theme" = light ] || dark=true
    dark_mode "$dark"
    mkdir -p "$out/mac/$theme"
    for shot in "${mac[@]}"; do
      name=${shot%%|*}
      read -ra args <<< "${shot#*|}"
      echo "mac $theme $name"
      style=()
      [ "$theme" = dark ] && style=(-AppleInterfaceStyle Dark)
      "$exe" "${args[@]}" -window 1440x900 -ApplePersistenceIgnoreState YES -appearance "$theme" ${style[@]+"${style[@]}"} \
        > "$tmp/$theme-$name.log" 2>&1 &
      pid=$!
      wid=""
      for _ in $(seq 1 60); do
        wid=$("$tmp/window" "$pid" 2> /dev/null) && break
        kill -0 "$pid" 2> /dev/null || break
        sleep 1
      done
      if [ -z "$wid" ]; then
        echo "::error::mac $theme $name: no window"; tail -n 40 "$tmp/$theme-$name.log"; failed=1
        kill "$pid" 2> /dev/null || true; wait "$pid" 2> /dev/null || true
        continue
      fi
      # Still: the window's picture the same twice a second apart.
      sleep 2
      prev=""
      for i in $(seq 1 10); do
        wid=$("$tmp/window" "$pid" 2> /dev/null) || break
        screencapture -x -o -l "$wid" "$tmp/shot.png"
        now=$(md5 -q "$tmp/shot.png")
        [ "$now" = "$prev" ] && break
        prev=$now
        sleep 1
      done
      cp "$tmp/shot.png" "$out/mac/$theme/$name.png"
      # The whole screen too, for a shot with a second window (Settings) or a sheet.
      case $name in *settings*|*add-transaction*) screencapture -x "$out/mac/$theme/$name-screen.png" ;; esac
      kill "$pid" 2> /dev/null || true
      wait "$pid" 2> /dev/null || true
      sleep 1
    done
  done
  dark_mode false || true
  ;;
esac
exit $failed
