#!/usr/bin/env bash
# Every app's iOS screenshots on the simulator that is booted (apps.md),
# one command per device: the iPhone, or (FAMILY=ipad) the iPad, booted
# already. What is each app's is its <app>/demo/shots.json's `walk`, which
# the app walks inside itself (ui/docs/shots.md, "In the App"): installed
# afresh, its list (`phone` on the iPhone, `large` on the iPad) written as
# a plan into its own container, and opened with `-shots YES`; it goes
# through its first run, signs in, taps its way from shot to shot, draws
# each screen every way (both themes; on the iPad both ways up, turning in
# the app), writes the pictures beside the plan and exits. Fails, once all
# are taken, if any app's were not.
#
#   [PHASE=install] apps-ios.sh <out> <apps> <device name> <app>...
#
# PHASE=install installs each app afresh and writes its plan, and walks
# nothing: what the workflow runs in the background as soon as the
# simulator has booted, while the servers start. A later run without it
# walks what was installed so (each app marked in $INSTALLED, a directory
# of the device's own), and installs any that was not.
#
# On the iPad each app is installed as one that takes the whole screen
# (UIRequiresFullScreen, written into the simulator's build here alone):
# iPadOS 26 turns a scene on its side for the app only then ("The current
# windowing mode does not allow for programmatic changes to interface
# orientation", app-screens 43327637228). The builds for people are not
# touched: they take Split View and Slide Over as ever.
#
# <apps> holds simulator-<app>/ with the app's .app in it (ios-build.sh).
# Writes <out>/<app>/ios/<theme>/<id>.png (and .tree.txt) and its
# device.json, the iPad's <out>/<app>/ipad-portrait/ and ipad-landscape/;
# each walk's log to $RESULTS (a directory of its own; a temporary one when
# unset), and with SAMPLE set the app's main thread's samples beside it.
# An app whose build is the iPhone's alone is passed over on the iPad (ios-family.sh).
#
# Each device's own, so the iPhone and the iPad can be taken at once:
# PORT_OFFSET, added to each server's shots.json port (the device's own
# instance of it, app-server.sh's `port`); RESULTS and PARTIAL, a
# directory and a file of its own each. PARTIAL, when set, is a file each
# app any of whose shots were not taken is added to: the workflow keeps no
# hash beside its shots, so it is taken again.
set -uo pipefail
out=$1
apps=$2
device=$3
shift 3
here=$(cd "$(dirname "$0")" && pwd)
family=${FAMILY:-iphone}
partial=${PARTIAL:-/dev/null}
results=${RESULTS:-$(mktemp -d)}
phase=${PHASE:-walk}
installed=${INSTALLED:-${RUNNER_TEMP:-/tmp}/installed}
mkdir -p "$results" "$out" "$installed"
udid=$(xcrun simctl list devices booted -j | jq -r --arg d "$device" '[.devices[][] | select(.state == "Booted")] | (map(select(.name == $d)) + .)[0].udid')

taken=()
walks=()
for a in "$@"; do
  app=$(find "$apps/simulator-$a" -maxdepth 1 -name '*.app' 2>/dev/null | head -1)
  # Nothing published for it yet: said where it was fetched, and its shots kept.
  [ -n "$app" ] || { echo "$a: no iPhone build to take; skipped"; continue; }
  if [ "$family" = ipad ]; then
    [ -n "$(bash "$here/ios-family.sh" "$apps" "$a")" ] || continue
    kinds="ipad-portrait:portrait ipad-landscape:landscape"
  else
    kinds="ios:"
  fi
  bundle=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")
  walks+=("$a:$family:$bundle:$app")
  for k in $kinds; do taken+=("$a:$k"); done
done
[ ${#taken[@]} -gt 0 ] || { echo "no app to take on the $family"; exit 0; }

# Each app installed afresh (and the simulator's keychain emptied, so no
# login of a walk before is remembered), what it would ask a person for
# said yes to, its plan in its container. Each phase said with its time.
install() {
  local a=$1 bundle=$2 app=$3 list=$4
  local p server data shots
  p=$(date +%s)
  ph() { echo "  $a: $1 $(( $(date +%s) - p ))s"; p=$(date +%s); }
  server=$(jq -r --argjson off "${PORT_OFFSET:-0}" '.server.port // empty | "http://127.0.0.1:\(. + $off)"' "$a/demo/shots.json")
  if [ "$family" = ipad ]; then
    # The simulator's build alone, made one that takes the whole screen
    # (above), and signed again as the simulator's builds are, its
    # entitlements (the keychain's) kept.
    local whole
    whole=$(mktemp -d)/$(basename "$app")
    cp -R "$app" "$whole"
    chmod -R u+w "$whole"
    /usr/libexec/PlistBuddy -c 'Delete :UIRequiresFullScreen' "$whole/Info.plist" > /dev/null 2>&1 || true
    /usr/libexec/PlistBuddy -c 'Add :UIRequiresFullScreen bool true' "$whole/Info.plist"
    codesign --force --sign - --preserve-metadata=entitlements "$whole" > /dev/null 2>&1 ||
      codesign --force --sign - "$whole" > /dev/null
    app=$whole
    ph "made to take the whole screen"
  fi
  xcrun simctl terminate "$udid" "$bundle" > /dev/null 2>&1 || true
  xcrun simctl uninstall "$udid" "$bundle" > /dev/null 2>&1 || true
  xcrun simctl keychain "$udid" reset > /dev/null 2>&1 || true
  ph "uninstalled, keychain emptied"
  xcrun simctl install "$udid" "$app" || { echo "::error::$a did not install"; return 1; }
  ph installed
  xcrun simctl privacy "$udid" grant all "$bundle" > /dev/null 2>&1 || true
  data=$(xcrun simctl get_app_container "$udid" "$bundle" data)
  shots="$data/Library/Application Support/shots"
  mkdir -p "$shots"
  jq --arg s "$server" --arg l "$list" \
    '{orientations: (if $l == "large" then ["portrait", "landscape"] else null end), theme: .walk.theme, shots: .walk[$l]} | with_entries(select(.value != null))
     | walk(if type == "string" then gsub("\\{server\\}"; $s) else . end)' "$a/demo/shots.json" > "$shots/plan.json"
  ph "permissions, plan written"
  touch "$installed/$udid-$a"
}

status=0
if [ "$phase" = install ]; then
  for w in ${walks[@]+"${walks[@]}"}; do
    IFS=: read -r a kind bundle app <<< "$w"
    list=phone
    [ "$family" = ipad ] && list=large
    rm -f "$installed/$udid-$a"
    install "$a" "$bundle" "$app" "$list" || status=1
  done
  exit $status
fi

# The walks: each app as installed above (or now, when it was not), opened
# with `-shots YES`, and waited for until it says it is over (`done`, or
# `failed` and why) or its process ends. Its pictures, drawn upright by the
# app whichever way it is held, then taken from the container; its log
# (each shot's step, switch and picture times) said.
for w in ${walks[@]+"${walks[@]}"}; do
  IFS=: read -r a kind bundle app <<< "$w"
  list=phone
  [ "$family" = ipad ] && list=large
  t=$(date +%s)
  if [ -f "$installed/$udid-$a" ]; then
    echo "  $a: installed already, as the simulator booted"
  else
    install "$a" "$bundle" "$app" "$list" || { echo "$a" >> "$partial"; status=1; continue; }
  fi
  rm -f "$installed/$udid-$a"
  data=$(xcrun simctl get_app_container "$udid" "$bundle" data)
  shots="$data/Library/Application Support/shots"
  p=$(date +%s)
  ph() { echo "  $a: $1 $(( $(date +%s) - p ))s"; p=$(date +%s); }
  pid=$(xcrun simctl launch "$udid" "$bundle" -shots YES 2>&1 | awk -F': ' '/: [0-9]+$/ {print $NF}')
  ph "launched (pid ${pid:-unknown})"
  # SAMPLE (seconds): what the app's main thread did over its walk's first
  # SAMPLE seconds, by macOS's `sample` of it, said after its log
  # (sample.py): where a slow step's time went.
  sampler=
  if [ -n "${SAMPLE:-}" ] && [ -n "$pid" ]; then
    sample "$pid" "$SAMPLE" 1 -mayDie -file "$results/sample-$a-$kind.txt" > /dev/null 2>&1 &
    sampler=$!
  fi
  until=$(( $(date +%s) + 1200 ))
  while [ "$(date +%s)" -lt "$until" ]; do
    [ -f "$shots/out/done" ] || [ -f "$shots/out/failed" ] && break
    if [ -n "$pid" ] && ! kill -0 "$pid" 2> /dev/null; then
      sleep 1
      if ! [ -f "$shots/out/done" ] && ! [ -f "$shots/out/failed" ]; then
        echo "::error::$a $kind: the app ended with its walk unsaid"
        # Why, when the system wrote it down: the newest crash report of
        # the app's process, its exception and where it stopped.
        sleep 5
        ips=$(ls -t "$HOME/Library/Logs/DiagnosticReports/"*.ips 2> /dev/null | xargs grep -l "\"$bundle\"" 2> /dev/null | head -1)
        [ -n "$ips" ] && python3 "$here/crash.py" "$ips" | sed 's/^/  /' || echo "  no crash report found"
      fi
      break
    fi
    sleep 1
  done
  sed 's/^/  /' "$shots/out/walk.log" 2> /dev/null || true
  cp "$shots/out/walk.log" "$results/walk-$a-$kind.log" 2> /dev/null || true
  echo "$a $kind: the walk took $(( $(date +%s) - t ))s"
  if [ -n "$sampler" ]; then
    wait "$sampler" 2> /dev/null || true
    echo "  $a $kind: its main thread over the walk's first ${SAMPLE}s:"
    python3 "$here/sample.py" "$results/sample-$a-$kind.txt" 2>&1 || echo "    no sample"
  fi
  # The iPhone's pictures under ios/, the iPad's under ipad-portrait/ and
  # ipad-landscape/ (the walk's out/portrait/ and out/landscape/).
  dir=$out/$a/ios
  for from in "" portrait landscape; do
    [ -n "$from" ] && dir=$out/$a/ipad-$from
    for theme in light dark; do
      [ -d "$shots/out/${from:+$from/}$theme" ] || continue
      mkdir -p "$dir/$theme"
      cp "$shots/out/${from:+$from/}$theme"/* "$dir/$theme/"
    done
  done
  if [ ! -f "$shots/out/done" ]; then
    echo "::error::$a $kind: the walk failed: $(cat "$shots/out/failed" 2> /dev/null || echo 'it said nothing')"
    cp "$shots/out/failed" "$dir/stuck.why.txt" 2> /dev/null || true
    status=1
  fi
  xcrun simctl terminate "$udid" "$bundle" > /dev/null 2>&1 || true
  rm -f "$shots/plan.json"
done

# Each shot's size, as it was drawn.
dims() { sips -g pixelWidth -g pixelHeight "$1" | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w, h}'; }

failed=0
for t in "${taken[@]}"; do
  a=${t%%:*} rest=${t#*:} kind=${rest%%:*} orientation=${rest#*:}
  dir=$out/$a/$kind
  want=$(jq --arg l "$([ "$family" = ipad ] && echo large || echo phone)" '.walk[$l] | length' "$a/demo/shots.json")
  for theme in light dark; do
    have=$(find "$dir/$theme" -maxdepth 1 -name '*.png' ! -name 'stuck.png' 2> /dev/null | wc -l | tr -d ' ')
    if [ "$have" -lt "$want" ]; then
      echo "::error::$a's $kind ($theme): $have of $want shots taken"
      echo "$a" >> "$partial"
      failed=1
    fi
  done
  # On its side: each picture wider than tall, as the app drew its scene.
  if [ "$orientation" = landscape ]; then
    for png in "$dir"/*/*.png; do
      [ -f "$png" ] || continue
      read -r w h < <(dims "$png")
      [ "$w" -gt "$h" ] || { echo "::error::$a's $kind: $(basename "$png") is ${w}x${h}: not on its side"; echo "$a" >> "$partial"; failed=1; }
    done
  fi
  first=$(find "$dir" -mindepth 2 -maxdepth 2 -name '*.png' ! -name 'stuck.png' 2> /dev/null | sort | head -1)
  [ -n "$first" ] || continue
  read -r w h < <(dims "$first")
  if [ "$family" = ipad ]; then corner=110 cutout=none; else corner=${CORNER:-165} cutout=${CUTOUT:-island}; fi
  printf '{"name":"%s","width":%s,"height":%s,"corner":%s,"cutout":"%s"}\n' "$device" "$w" "$h" "$corner" "$cutout" > "$dir/device.json"
done
[ "$status" -eq 0 ] || failed=1
exit $failed
