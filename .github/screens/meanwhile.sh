#!/usr/bin/env bash
# Work a job's later steps need, run while the steps between go on: a
# background process outlives the step that started it (the runner ends it
# only with the job), so a build or a boot can run under the nix steps.
#
#   meanwhile.sh start <name> <command…>   start it; its output to a log
#   meanwhile.sh wait <name>               wait for it, print the log, and
#                                           fail as it failed
set -euo pipefail
dir=${RUNNER_TEMP:-/tmp}/meanwhile
mkdir -p "$dir"
what=$1
name=$2
shift 2
case $what in
  start)
    rm -f "$dir/$name.status"
    nohup bash -c '"$@" > "$0.log" 2>&1; echo $? > "$0.status"' "$dir/$name" "$@" < /dev/null > /dev/null 2>&1 &
    echo "started $name in the background"
    ;;
  wait)
    until [ -f "$dir/$name.status" ]; do sleep 1; done
    cat "$dir/$name.log"
    status=$(cat "$dir/$name.status")
    [ "$status" = 0 ] || echo "::error::$name failed ($status)"
    exit "$status"
    ;;
esac
