#!/usr/bin/env bash
# Listen's screenshots (SCREENS.md), each in light and dark, on GitHub's
# macOS runner (.github/workflows/reference-listen-shots.yaml).
#
#   shots.sh ios  <out> <Listen.app (simulator)>   iPhone, then iPad portrait and landscape
#   shots.sh mac  <out> <Listen.app (macOS)>       the Mac window, 1440x900
#
# Writes <out>/<device>/<theme>/<screen>.png. Shots whose list entry has a
# third field (steps) are taken by the drive UI test (drive/Shots.swift),
# which opens menus, context menus and submenus; the rest are launched
# directly (simctl on iOS, LaunchServices on the Mac).
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
what=$1
out=$2
app=$3
bundle=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist" 2> /dev/null ||
  /usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")
failed=0

# name|arguments[|steps] (SCREENS.md; -appearance is added per theme).
iphone=(
  "01-home|-screen home"
  "02-library|-screen library"
  "03-songs|-screen songs"
  "04-album-harbor-lights|-screen album:harbor-lights"
  "05-podcasts|-screen podcasts"
  "06-show-grace-hill|-screen show:grace-hill"
  "07-episode-transcript|-screen episode:grace-hill-1 -playing episode:grace-hill-1"
  "08-player|-screen home -player expanded"
  "09-player-lyrics|-screen home -player expanded -panel lyrics"
  "10-player-transcript|-screen home -player expanded -panel lyrics -playing episode:grace-hill-1"
  "11-player-upnext|-screen home -player expanded -panel upnext"
  "12-household|-screen household"
  "13-search-bach|-screen search:bach"
  "14-settings-playback|-screen settings -settingsTab playback"
  "20-popover-output|-screen home -player expanded -popover output"
  "21-dialog-handoff|-screen household -dialog handoff:kitchen"
  "22-sheet-new-playlist|-screen playlists -sheet new-playlist"
  "23-dialog-remove-download|-screen downloads -dialog remove-download:grace-hill-1"
  "24-search-suggestions|-screen search:sl -searchActive YES"
  "30-menu-sort|-screen songs|tap:menu.sort"
  "31-context-song|-screen album:harbor-lights|context:song.harbor-lights-3"
  "32-context-song-submenu|-screen album:harbor-lights|context:song.harbor-lights-3 hover:Add_to_Playlist"
  "33-context-album|-screen albums|context:album.paper-satellites"
  "34-context-episode|-screen show:workbench|context:episode.workbench-2"
  "35-menu-speed|-screen home -player expanded -playing episode:grace-hill-1|tap:menu.speed"
)
ipad_portrait=(
  "01-home|-screen home"
  "02-songs|-screen songs"
  "03-album-clair-de-lune|-screen album:clair-de-lune"
  "04-show-grace-hill|-screen show:grace-hill"
  "05-player|-screen home -player expanded"
  "06-household|-screen household"
  "07-search-bach|-screen search:bach"
  "08-settings|-screen settings -settingsTab downloads"
  "20-popover-output|-screen songs -popover output"
  "21-popover-upnext|-screen albums -popover upnext"
  "22-dialog-handoff|-screen household -dialog handoff:kitchen"
  "23-sheet-new-playlist|-screen home -sheet new-playlist"
  "30-menu-sort|-screen songs|tap:menu.sort"
  "32-context-song-submenu|-screen songs|context:song.goldberg-3 hover:Add_to_Playlist"
  "33-context-album|-screen albums|context:album.paper-satellites"
  "35-menu-speed|-screen home -player expanded -playing episode:grace-hill-1|tap:menu.speed"
)
# All through the drive test (it turns the device): steps may be empty.
ipad_landscape=(
  "01-songs-inspector-lyrics|-screen songs -inspector lyrics|"
  "02-album-inspector-upnext|-screen album:clair-de-lune -inspector upnext -playing track:clair-de-lune-4|"
  "03-show-transcript-inspector|-screen show:grace-hill -playing episode:grace-hill-1 -inspector lyrics|"
  "04-player|-screen home -player expanded|"
  "05-player-transcript|-screen home -player expanded -panel lyrics -playing episode:grace-hill-1|"
  "06-household|-screen household|"
  "07-home|-screen home|"
  "08-settings-household|-screen settings -settingsTab household|"
  "20-popover-output|-screen home -popover output|"
  "30-menu-sort|-screen songs|tap:menu.sort"
  "32-context-song-submenu|-screen songs|context:song.goldberg-3 hover:Add_to_Playlist"
  "34-context-episode|-screen show:workbench|context:episode.workbench-2"
)
mac=(
  "01-songs|-screen songs"
  "02-album-harbor-lights|-screen album:harbor-lights"
  "03-songs-inspector-lyrics|-screen songs -inspector lyrics"
  "04-episode-transcript-inspector|-screen episode:grace-hill-1 -playing episode:grace-hill-1 -inspector lyrics"
  "05-home|-screen home"
  "06-player|-screen home -player expanded"
  "07-household|-screen household"
  "08-search-bach|-screen search:bach"
  "09-settings-playback|-screen settings -settingsTab playback"
  "20-popover-output|-screen songs -popover output"
  "21-popover-upnext|-screen albums -popover upnext"
  "22-dialog-handoff|-screen household -dialog handoff:kitchen"
  "23-sheet-new-playlist|-screen playlist:road-trip -sheet new-playlist"
  "24-search-suggestions|-screen search:sl -searchActive YES"
)
mac_driven=(
  "30-menu-sort|-screen songs|tap:menu.sort"
  "31-context-song-submenu|-screen songs|context:song.goldberg-3 hover:Add_to_Playlist"
  "33-context-album|-screen albums|context:album.paper-satellites"
  "34-context-episode|-screen show:workbench|context:episode.workbench-2"
  "35-menu-speed|-screen home -player expanded -playing episode:grace-hill-1|tap:menu.speed"
  "36-menu-controls|-screen album:harbor-lights|tap:Controls"
)

# Entries with steps (a third field), or without, into the array `list`
# (no mapfile: the image's /bin/bash is 3.2).
driven() { list=(); for s in "$@"; do [[ $s == *"|"*"|"* ]] && list+=("$s"); done; return 0; }
direct() { list=(); for s in "$@"; do [[ $s == *"|"*"|"* ]] || list+=("$s"); done; return 0; }

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

# The app's saved state wiped, so each launch is a first launch.
fresh() {
  local data
  data=$(xcrun simctl get_app_container "$1" "$bundle" data 2> /dev/null) || return 0
  rm -rf "$data/Library/Saved Application State" "$data/Library/Preferences" "$data/Library/SplitViewState"
}

# Direct shots on a booted simulator: launched with each shot's arguments.
direct_ios() {
  local udid=$1 device=$2 theme=$3
  shift 3
  mkdir -p "$out/$device/$theme"
  for shot in "$@"; do
    name=${shot%%|*}
    read -ra args <<< "${shot#*|}"
    echo "$device $theme $name"
    xcrun simctl terminate "$udid" "$bundle" > /dev/null 2>&1 || true
    fresh "$udid"
    if ! xcrun simctl launch "$udid" "$bundle" "${args[@]}" -appearance "$theme" > /dev/null; then
      echo "::error::$device $theme $name: did not launch"; failed=1; continue
    fi
    still "$udid" "$out/$device/$theme/$name.png"
  done
  xcrun simctl terminate "$udid" "$bundle" > /dev/null 2>&1 || true
}

# The drive project, generated once.
derived=$(mktemp -d)
(cd "$here/drive" && xcodegen generate > /dev/null)

# Driven shots: one test run for the list, the theme added to each launch.
drive() {
  local scheme=$1 dest=$2 device=$3 theme=$4 orientation=$5
  shift 5
  local list="" shot name rest args steps
  for shot in "$@"; do
    name=${shot%%|*}; rest=${shot#*|}; args=${rest%%|*}; steps=${rest#*|}
    list+="$name|$args -appearance $theme|$steps"$'\n'
  done
  [ -n "$list" ] || return 0
  mkdir -p "$out/$device/$theme"
  local attempt missing line
  for attempt in 1 2; do
    echo "$device $theme: driving $(($(wc -l <<< "$list") - 1)) shots (attempt $attempt)"
    TEST_RUNNER_SHOTS_BUNDLE="$bundle" TEST_RUNNER_SHOTS_LIST="$list" TEST_RUNNER_SHOTS_OUT="$out/$device/$theme" \
      TEST_RUNNER_SHOTS_ORIENTATION="$orientation" \
      xcodebuild test-without-building -project "$here/drive/Drive.xcodeproj" -scheme "$scheme" \
        -destination "$dest" -derivedDataPath "$derived" > "$out/drive-$device-$theme-$attempt.log" 2>&1 && break
    grep -E "error:|Failed" "$out/drive-$device-$theme-$attempt.log" | head -n 5 || true
    missing=""
    while IFS= read -r line; do
      [ -n "$line" ] || continue
      [ -f "$out/$device/$theme/${line%%|*}.png" ] || missing+="$line"$'\n'
    done <<< "$list"
    [ -n "$missing" ] || break
    if [ $attempt = 2 ]; then
      echo "::error::$device ($theme) driven shots failed: $(cut -d'|' -f1 <<< "$missing" | tr '\n' ' ')"
      tail -n 40 "$out/drive-$device-$theme-$attempt.log"; failed=1
    fi
    list=$missing
  done
  grep -h "^SHOT " "$out/drive-$device-$theme"-*.log | sed 's/^/::warning::/' || true
  mkdir -p "$out/trees"
  for t in "$out/$device/$theme"/*.tree.txt; do
    [ -f "$t" ] || continue
    mv "$t" "$out/trees/$device-$theme-$(basename "$t")"
  done
}

case $what in
ios)
  xcodebuild build-for-testing -project "$here/drive/Drive.xcodeproj" -scheme Shots \
    -destination 'generic/platform=iOS Simulator' -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO -quiet || exit 1

  udid=$(boot '^iPhone 17 Pro$') || exit 1
  xcrun simctl install "$udid" "$app"
  for theme in light dark; do
    appearance "$udid" "$theme"
    direct "${iphone[@]}"
    direct_ios "$udid" iphone "$theme" ${list[@]+"${list[@]}"}
    driven "${iphone[@]}"
    drive Shots "id=$udid" iphone "$theme" portrait ${list[@]+"${list[@]}"}
  done
  xcrun simctl shutdown all || true

  udid=$(boot '^iPad Pro 13-inch') || exit 1
  xcrun simctl install "$udid" "$app"
  for theme in light dark; do
    appearance "$udid" "$theme"
    direct "${ipad_portrait[@]}"
    direct_ios "$udid" ipad-portrait "$theme" ${list[@]+"${list[@]}"}
    driven "${ipad_portrait[@]}"
    drive Shots "id=$udid" ipad-portrait "$theme" portrait ${list[@]+"${list[@]}"}
    fresh "$udid"
    drive Shots "id=$udid" ipad-landscape "$theme" landscape "${ipad_landscape[@]}"
  done
  # XCUITest's screenshot is in the screen's own (portrait) frame: turned
  # back the way the device was, landscape left.
  for png in "$out"/ipad-landscape/*/*.png; do
    [ -f "$png" ] || continue
    read -r w h < <(sips -g pixelWidth -g pixelHeight "$png" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}')
    if [ "$h" -gt "$w" ]; then sips -r 270 "$png" > /dev/null; fi
  done
  xcrun simctl shutdown all || true
  ;;
mac)
  exe="$app/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")"
  tmp=$(mktemp -d)
  # A process's window id (.github/screens/window.swift).
  xcrun swiftc -O "$repo/.github/screens/window.swift" -o "$tmp/window"
  # A screen that holds a 1440x900 window (display.swift).
  xcrun swiftc -O "$here/display.swift" -o "$tmp/display" && "$tmp/display" | grep -v '^mode' || true
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
  # The UI test drives the app by its bundle id: LaunchServices must know it,
  # at a path the test runner can read (not the job's temporary directory).
  lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
  rm -rf /Applications/Listen.app && ditto "$app" /Applications/Listen.app && app=/Applications/Listen.app
  "$lsregister" -u "$3" > /dev/null 2>&1 || true
  "$lsregister" -f "$app" || true
  exe="$app/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")"
  sudo automationmodetool enable-automationmode-without-authentication > /dev/null 2>&1 || true
  mac_built=0
  xcodebuild build-for-testing -project "$here/drive/Drive.xcodeproj" -scheme ShotsMac \
    -destination 'platform=macOS' -derivedDataPath "$derived" -quiet \
    CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM= ENABLE_HARDENED_RUNTIME=NO && mac_built=1 ||
    { echo "::error::the Mac drive test did not build"; failed=1; }
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
      # Through LaunchServices, so the app comes to the front and its
      # window is key (one run from the shell stays inactive and grey).
      open -n -a "$app" --stdout "$tmp/$theme-$name.log" --stderr "$tmp/$theme-$name.log" --args \
        "${args[@]}" -window 1440x900 -ApplePersistenceIgnoreState YES -appearance "$theme" ${style[@]+"${style[@]}"}
      pid=""
      for _ in $(seq 1 30); do
        pid=$(pgrep -n -f "$exe") && break
        sleep 0.5
      done
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
      rm -f "$tmp/shot.png"
      for i in $(seq 1 10); do
        wid=$("$tmp/window" "$pid" 2> /dev/null) || break
        screencapture -x -o -l "$wid" "$tmp/shot.png"
        now=$(md5 -q "$tmp/shot.png")
        [ "$now" = "$prev" ] && break
        prev=$now
        sleep 1
      done
      if [ -f "$tmp/shot.png" ]; then
        cp "$tmp/shot.png" "$out/mac/$theme/$name.png"
      else
        echo "::error::mac $theme $name: the window went away (crashed?)"; tail -n 20 "$tmp/$theme-$name.log"; failed=1
      fi
      # The whole screen too, for a second window (Settings), a popover or a sheet.
      case $name in *settings*|*popover*|*sheet*|*dialog*|*suggestions*) screencapture -x "$out/mac/$theme/$name-screen.png" ;; esac
      kill "$pid" 2> /dev/null || true
      for _ in $(seq 1 50); do kill -0 "$pid" 2> /dev/null || break; sleep 0.2; done
      sleep 1
    done
    # Menus are windows of their own: the driven shots are of the whole screen.
    if [ $mac_built = 1 ]; then
      list=()
      for s in "${mac_driven[@]}"; do
        n=${s%%|*}; r=${s#*|}
        list+=("$n|${r%%|*} -window 1440x900 -ApplePersistenceIgnoreState YES ${style[*]+${style[*]}}|${r#*|}")
      done
      drive ShotsMac "platform=macOS" mac "$theme" portrait "${list[@]}"
    fi
  done
  dark_mode false || true
  ;;
esac
exit $failed
