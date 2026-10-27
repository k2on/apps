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
    rm -f "$dir/$name.status" "$dir/$name.code"
    # Each line stamped with the seconds since it started: printed only
    # when waited for, the runner's own times would all be that moment.
    # The status is written once the log is too (the stamping reads to
    # the end), so a wait never prints a log still being written.
    nohup bash -c '
      { "$@" 2>&1; echo $? > "$0.code"; } |
        while IFS= read -r line; do printf "%4ss  %s\n" "$SECONDS" "$line"; done > "$0.log"
      mv "$0.code" "$0.status"' "$dir/$name" "$@" < /dev/null > /dev/null 2>&1 &
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
