#!/usr/bin/env bash
# `limit.sh <seconds> <command…>`: the command, ended if it is still going
# after <seconds>, and what the machine was doing then said first (on
# stderr): its memory, the largest processes, every build, compiler and
# simulator process, a sample of each asset compiler, and the booted
# simulators. So a build that hangs fails in minutes with why, not at the
# job's limit with nothing (app-screens 43771365729 and 43787509355 each sat
# over an hour on one `actool`, the simulator's asset catalog, and said
# nothing). Exits as the command did, or 124 when it was ended.
set -uo pipefail
limit=$1
shift
# Its own process group, so that ending it ends everything it started.
set -m
"$@" &
pid=$!
set +m
start=$(date +%s)
while kill -0 "$pid" 2> /dev/null; do
  if [ $(( $(date +%s) - start )) -ge "$limit" ]; then
    {
      echo "::error::still going after ${limit}s, so ended: $*"
      echo "== memory"
      vm_stat 2> /dev/null || free -m
      sysctl vm.swapusage 2> /dev/null || true
      memory_pressure 2> /dev/null | tail -n 1 || true
      echo "== the largest processes"
      ps -axo pid,ppid,etime,rss,%cpu,state,command | sort -k4 -nr | head -25 | cut -c1-250
      echo "== the builds, compilers and simulators"
      ps -axo pid,ppid,etime,rss,%cpu,state,command | grep -E 'nix|xcodebuild|actool|ibtool|swift|clang|ld |AssetCatalog|CoreSimulator|simctl|launchd_sim|SimRenderServer' | grep -v grep | cut -c1-300
      for p in $(pgrep -x actool 2> /dev/null; pgrep -f AssetCatalogSimulatorAgent 2> /dev/null); do
        echo "== a sample of $p: $(ps -o command= -p "$p" | cut -c1-120)"
        sample "$p" 2 -mayDie 2> /dev/null | grep -E '^ +[0-9]+ ' | head -40
      done
      echo "== the simulators booted"
      xcrun simctl list devices booted 2> /dev/null || true
    } >&2
    kill -TERM -- "-$pid" 2> /dev/null || kill -TERM "$pid"
    for _ in 1 2 3 4 5 6 7 8 9 10; do kill -0 "$pid" 2> /dev/null || break; sleep 1; done
    kill -KILL -- "-$pid" 2> /dev/null || true
    exit 124
  fi
  sleep 5
done
wait "$pid"
