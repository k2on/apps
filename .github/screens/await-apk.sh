#!/usr/bin/env bash
# An app's APK from a build run still going (app-screens.yaml, a pull
# request's Android shots, started when build starts): waited for until the
# run's `apk (<app>)` job has uploaded `apk-<app>`, then downloaded into
# <dir>/apk-<app>/. An artifact is there as soon as it is uploaded, so the
# shots need not wait for the rest of the run (its tests, the other apps).
# When that job ends without one (its inputs had not moved, or it failed),
# there is nothing to take, and it says so.
#
#   await-apk.sh <run id> <app> <dir>
#
# Exits 0 with the APK in place, 3 when the run built none for <app>.
set -euo pipefail
run=$1
app=$2
dir=$3
repo=${GITHUB_REPOSITORY:?}
# Each answer taken whole, then searched: `gh … | grep -q` under pipefail
# reads a match as a failure when grep leaves before gh has said everything.
has() {
  local names
  names=$(gh api "repos/$repo/actions/runs/$run/artifacts" --paginate --jq '.artifacts[].name')
  grep -qx "apk-$app" <<< "$names"
}
# The app's APK job has ended, or the whole run has (cancelled, or failed
# before it had one).
ended() {
  local jobs
  [ "$(gh api "repos/$repo/actions/runs/$run" --jq .status)" = completed ] && return 0
  jobs=$(gh api "repos/$repo/actions/runs/$run/jobs" --paginate --jq ".jobs[] | select(.name == \"apk ($app)\") | .status")
  grep -qx completed <<< "$jobs"
}
for _ in $(seq 1 240); do
  if has; then
    gh run download "$run" --repo "$repo" --name "apk-$app" --dir "$dir/apk-$app"
    echo "$app: the run's APK, taken as soon as it was up"
    exit 0
  fi
  # Ended: looked for once more, as the upload is the job's last step.
  if ended; then
    has && { gh run download "$run" --repo "$repo" --name "apk-$app" --dir "$dir/apk-$app"; exit 0; }
    echo "$app: the run built no APK for it; nothing to take"
    exit 3
  fi
  sleep 15
done
echo "::error::$app: no APK from run $run within an hour"
exit 1
