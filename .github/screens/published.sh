#!/usr/bin/env bash
# Each app's APK at a commit, for the Android shots of a run with no build
# run of its own to take them from (app-screens.yaml's `apks`: a push to
# main that moved the screenshots' scripts, a run by hand, a dry run):
# the APKs build.yaml built at that commit, so the shots are of the commit
# that was pushed, as the iPhone's and the desktop's are (nix builds them
# at it).
#
#   published.sh android <dir> <app>...   each at <dir>/apk-<app>/<app>.apk
#
#   COMMIT  the commit (required)
#   REF     its branch, whose build run is taken (build.yaml's run at
#           COMMIT on REF: main's push, or a branch's run by hand)
#   IDS     each app's applicationId in each channel, as JSON
#           ({"<app>": {"stable": id, "unstable": id}}, the flake's
#           `publish.apks`, which `which` reads)
#   SITE    the site the F-Droid repositories are under
#
# Each is waited for while that run is making it (await-apk.sh). Its
# channel is that run's (.github/channels.sh): main's push builds each
# app's `.unstable` twin, anything else (a branch's run by hand, as
# dispatched by default) the app under its own id. Nothing here assumes the
# id: the emulator's driver reads it from the APK (app-android.sh).
#
# An app whose `apk` job succeeded and built nothing (its inputs seen built
# before, build.yaml's `apk-seen`: only main's pushes mark them, so only
# main's skip) is taken as the run's channel's F-Droid repository offers it:
# that is the APK of those same inputs, built and deployed by the push that
# first had them. Before this took the stable repository's whatever the
# commit, which since the channels split is the last release's APK, not
# main's: app-screens 43622627318 photographed the pre-port money APK
# there, which has no in-app walk and never said ready.
#
# Anything else that leaves an app with no APK is no reason to take
# another build's in its place: a run that failed before an app's APK, or
# none at the commit, fails this (app-screens 43069095661 took F-Droid's
# APKs when another dispatch had cancelled its build, and every Android job
# then failed waiting), after the other apps are fetched. One exception:
# main's build cancelled by a newer push is superseded, not failed; the
# newer push's build, which builds what this one did not finish, is taken
# when it ends (app-screens' `workflow_run`), so the app is left out with a
# notice and keeps the shots it had.
set -uo pipefail
phone=$1
dir=$2
shift 2
here=$(dirname "$0")
: "${COMMIT:?}" "${REF:?}" "${IDS:?}" "${SITE:?}" "${GITHUB_REPOSITORY:?}"
case $phone in
  android) ;;
  *) echo "usage: published.sh android <dir> <app>..."; exit 1 ;;
esac
mkdir -p "$dir"

# build's run at the commit, asked until it answers (a refusal, "API rate
# limit exceeded", is no answer: in app-screens 42965705977 its text was
# taken for the run's id, and that run waited for for an hour), and until
# there is one: a push starts this and build at once.
run=
for try in 1 2 3 4 5 6 7 8 9 10; do
  if said=$(gh api "repos/$GITHUB_REPOSITORY/actions/workflows/build.yaml/runs?head_sha=$COMMIT&branch=$REF&per_page=1" --jq '.workflow_runs[0] // empty | "\(.id) \(.event) \(.head_branch)"' 2> /dev/null); then
    if [ -n "$said" ]; then
      read -r run event branch <<< "$said"
      break
    fi
    echo "no build run at ${COMMIT:0:7} on $REF yet (try $try)"
  else
    echo "the API did not answer for build's run at ${COMMIT:0:7} (try $try)"
  fi
  sleep $((try * 20))
done
if [ -n "$run" ] && ! [[ $run =~ ^[0-9]+$ ]]; then
  echo "::error::build's run at ${COMMIT:0:7}: the API said '$run'"
  exit 1
fi
if [ -z "$run" ]; then
  echo "::error::no build run at ${COMMIT:0:7} on $REF: the Android shots take that commit's APKs, so dispatch build.yaml on the branch at that commit first"
  exit 1
fi
# The run's channel, as channels.sh says it for that run.
channel=$(GITHUB_REF="refs/heads/$branch" GITHUB_EVENT_NAME=$event bash "$here/../channels.sh" channel)
repo="$SITE/$(bash "$here/../channels.sh" repo-path "$channel")"
echo "build's run $run at ${COMMIT:0:7} ($event on $branch): the $channel channel's APKs"

index=
# <app>'s APK as the channel's F-Droid repository offers it: its id's newest.
offered() {
  local a=$1 pkg name
  pkg=$(jq -r --arg a "$a" --arg c "$channel" '.[$a][$c] // empty' <<< "$IDS")
  if [ -z "$pkg" ]; then echo "::error::$a has no id in the $channel channel"; return 1; fi
  if [ -z "$index" ]; then
    index=$(curl -fsSL --retry 3 "$repo/index-v1.json") || { echo "::error::no F-Droid index at $repo"; index=; return 1; }
  fi
  name=$(jq -r --arg p "$pkg" '.packages[$p] // [] | max_by(.versionCode) | .apkName // empty' <<< "$index")
  if [ -z "$name" ]; then echo "::error::$repo offers no $pkg"; return 1; fi
  mkdir -p "$dir/apk-$a"
  curl -fsSL --retry 3 -o "$dir/apk-$a/$a.apk" "$repo/$name" || { echo "::error::$a: $repo/$name would not download"; rm -rf "${dir:?}/apk-$a"; return 1; }
  echo "$a: $name, as $repo offers it (build's run $run built none: its inputs seen built before)"
}

bad=
for a in "$@"; do
  got=$(mktemp -d)
  code=0
  bash "$here/await-apk.sh" "$run" "$a" "$got" || code=$?
  if [ "$code" = 0 ]; then
    rm -rf "${dir:?}/apk-$a"
    mv "$got/apk-$a" "$dir/apk-$a"
    echo "$a: build's APK at ${COMMIT:0:7} (run $run)"
  elif [ "$code" = 3 ]; then
    # Why none: the run cancelled or failed, or the app's job skipped its
    # build as seen before.
    conclusion=$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run" --jq .conclusion)
    job=$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run/jobs" --paginate --jq ".jobs[] | select(.name == \"apk ($a)\") | .conclusion" | head -1)
    if [ "$job" = success ]; then
      offered "$a" || bad=1
    elif [ "$conclusion" = cancelled ] && [ "$channel" = unstable ] && [ "$event" = push ]; then
      echo "::notice::$a: main's build run $run at ${COMMIT:0:7} was cancelled (a newer push superseded it) before its APK; its Android shots are left to the newer push's"
    else
      echo "::error::$a: build's run $run at ${COMMIT:0:7} ended ${conclusion:-unfinished} with no APK (its apk job: ${job:-none}); not taking another build's in its place, so its Android shots are not taken. Dispatch build.yaml again at that commit, then this run."
      rm -rf "${dir:?}/apk-$a"
      bad=1
    fi
  else
    echo "::error::$a: build's APK at ${COMMIT:0:7} (run $run) would not come; its Android shots are not taken"
    rm -rf "${dir:?}/apk-$a"
    bad=1
  fi
  rm -rf "$got"
done
# The other apps' shots are still taken (the emulators' jobs run on what
# was fetched), and this fails once they are listed.
exit "${bad:-0}"
