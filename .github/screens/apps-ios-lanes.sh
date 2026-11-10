#!/usr/bin/env bash
# The iPhone's and the iPad's shots of every app on one Mac, several at
# once: each app on each device is a lane, its own simulator, booted and
# then taken (apps-ios.sh, one app), up to LANES at a time (2 unless set).
# One job and one runner for them all, rather than one after the other on
# one simulator, or a Mac job each (GitHub gives an account five Macs, and
# a Mac's queue is long).
#
#   apps-ios-lanes.sh boot <kind> <app>...     a simulator per app (kind:
#                                              iphone, ipad), booting in the
#                                              background while fewer than
#                                              LANES are; the rest boot as
#                                              their turn comes
#   apps-ios-lanes.sh warm                     start booting the lanes not
#                                              booting yet, while fewer than
#                                              LANES are
#   apps-ios-lanes.sh take <out> <apps> <app>...  take every lane booted
#                                              or asked for; fail, once all
#                                              are tried, if any did
#
# The iPhone is the newest iOS's newest iPhone Pro; the iPad its iPad Pro
# 13-inch, else iPad Air 13-inch, else newest iPad. The first lane of a
# kind boots that simulator itself, every other one of its kind created
# beside it (`simctl create`, the same device type and runtime): device.json
# names the kind, each lane's UDID which (app-ios.sh's UDID). An iPad lane
# is only for an app whose build takes the iPad (ios-family.sh); a lane's
# simulator is shut down once it is taken. PARTIAL and RESULTS as
# apps-ios.sh's; each lane's log is printed whole once it ends.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
dir=${RUNNER_TEMP:-/tmp}/lanes
mkdir -p "$dir"
what=$1
shift

# "<udid> <name> <device type> <runtime>" of the kind's simulator.
base() {
  local kinds
  case $1 in
    ipad) kinds='["^iPad Pro 13-inch", "^iPad Air 13-inch", "^iPad"]' ;;
    *) kinds='["^iPhone [0-9]+ Pro$"]' ;;
  esac
  xcrun simctl list devices available -j | jq -r --argjson k "$kinds" '
    .devices | to_entries | map(select(.key | test("iOS"))) | sort_by(.key) | last as $r
    | $r.value as $d
    | $k | map(. as $p | $d | map(select(.name | test($p))) | sort_by(.name) | last) | map(select(. != null)) | first
    | "\(.udid)\t\(.name)\t\(.deviceTypeIdentifier)\t\($r.key)"'
}

lanes=${LANES:-2}
# A lane's simulator booting, in the background (meanwhile.sh).
start() {
  touch "$dir/$1.started"
  bash "$here/meanwhile.sh" start "boot-$1" bash -c '
    start=$(date +%s)
    xcrun simctl boot "$0" && xcrun simctl bootstatus "$0" -b > /dev/null
    echo "booted in $(( $(date +%s) - start ))s"' "$2"
}

case $what in
  boot)
    kind=$1
    shift
    IFS=$'\t' read -r udid name type runtime < <(base "$kind")
    [ -n "${udid:-}" ] || { echo "::error::no $kind simulator on this runner"; exit 1; }
    first=1
    for a in "$@"; do
      if [ -z "$first" ]; then udid=$(xcrun simctl create "$name" "$type" "$runtime") || { echo "::error::$a: no $kind simulator created"; continue; }; fi
      first=
      printf '%s\t%s\n' "$udid" "$name" > "$dir/$a-$kind"
      echo "$a $kind: $name ($udid)"
      shopt -s nullglob
      booting=("$dir"/*.started)
      if [ "${#booting[@]}" -lt "$lanes" ]; then start "$a-$kind" "$udid"; fi
    done
    ;;
  warm)
    shopt -s nullglob
    for f in "$dir"/*-iphone "$dir"/*-ipad; do
      l=${f##*/}
      [ -f "$dir/$l.started" ] && continue
      booting=("$dir"/*.started)
      [ "${#booting[@]}" -lt "$lanes" ] || break
      IFS=$'\t' read -r udid name < "$f"
      start "$l" "$udid"
    done
    ;;
  take)
    out=$1
    apps=$2
    shift 2
    # One lane: its boot waited for, its app taken, its simulator shut down.
    lane() {
      local a=$1 kind=$2 udid name status=0
      IFS=$'\t' read -r udid name < "$dir/$a-$kind"
      [ -f "$dir/$a-$kind.started" ] || start "$a-$kind" "$udid"
      bash "$here/meanwhile.sh" wait "boot-$a-$kind" || return 1
      if [ "$kind" = ipad ]; then
        UDID=$udid FAMILY=ipad bash "$here/apps-ios.sh" "$out" "$apps" "$name" "$a" || status=$?
      else
        UDID=$udid bash "$here/apps-ios.sh" "$out" "$apps" "$name" "$a" || status=$?
      fi
      xcrun simctl shutdown "$udid" > /dev/null 2>&1 || true
      return "$status"
    }
    todo=()
    for a in "$@"; do
      for kind in iphone ipad; do
        if [ -f "$dir/$a-$kind" ]; then todo+=("$a-$kind"); fi
      done
    done
    # macOS's own bash is 3.2 (no associative arrays): each lane says it
    # has ended in <lane>.status, and its log is printed whole then.
    failed=0
    running=()
    reap() {
      local l still=()
      for l in ${running[@]+"${running[@]}"}; do
        if [ -f "$dir/$l.status" ]; then
          echo "::group::$l ($(cat "$dir/$l.status"))"; cat "$dir/$l.log"; echo "::endgroup::"
          if [ "$(cat "$dir/$l.status")" != 0 ]; then failed=1; echo "${l%-*}" >> "${PARTIAL:-/dev/null}"; fi
        else
          still+=("$l")
        fi
      done
      running=(${still[@]+"${still[@]}"})
    }
    for l in "${todo[@]}"; do
      while [ "${#running[@]}" -ge "$lanes" ]; do sleep 2; reap; done
      echo "${l%-*} ${l##*-}: taken"
      { lane "${l%-*}" "${l##*-}" > "$dir/$l.log" 2>&1; echo $? > "$dir/$l.status"; } &
      running+=("$l")
    done
    while [ "${#running[@]}" -gt 0 ]; do sleep 2; reap; done
    exit $failed
    ;;
esac
