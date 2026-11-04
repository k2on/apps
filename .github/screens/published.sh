#!/usr/bin/env bash
# Each app's last published build, for screenshots taken without building
# (app-screens.yaml, apps.md): what the phones were last given.
#
#   published.sh android <dir> <app>...   each APK as the F-Droid repository
#                                         offers it ($FDROID, its index's
#                                         newest for the app's package), at
#                                         <dir>/apk-<app>/<app>.apk
#   published.sh ios <dir> <app>...       main's newest simulator-<app>
#                                         artifact (ios.yaml makes one on
#                                         main only with a TestFlight send),
#                                         at <dir>/simulator-<app>/
#
# An app with nothing published is said and left out; the others are still
# fetched. The iOS half asks GitHub's API as $GH_TOKEN, of $GITHUB_REPOSITORY.
set -uo pipefail
phone=$1
dir=$2
shift 2
case $phone in
  android)
    index=$(curl -fsSL --retry 3 "$FDROID/index-v1.json") || { echo "::error::no F-Droid index at $FDROID"; exit 1; }
    for a in "$@"; do
      pkg=$(jq -r .android "$a/screens/shots.json")
      name=$(jq -r --arg p "$pkg" '.packages[$p] // [] | max_by(.versionCode) | .apkName // empty' <<< "$index")
      if [ -z "$name" ]; then echo "::warning::F-Droid offers no $pkg; $a's Android shots are kept"; continue; fi
      mkdir -p "$dir/apk-$a"
      curl -fsSL --retry 3 -o "$dir/apk-$a/$a.apk" "$FDROID/$name" && echo "$a: $name" ||
        { echo "::warning::$a: $name would not download"; rm -rf "${dir:?}/apk-$a"; }
    done
    ;;
  ios)
    for a in "$@"; do
      id=$(gh api "repos/$GITHUB_REPOSITORY/actions/artifacts?name=simulator-$a&per_page=100" \
        --jq '[.artifacts[] | select(.expired | not) | select(.workflow_run.head_branch == "main")][0].id // empty')
      if [ -z "$id" ]; then echo "::warning::main has no simulator-$a kept (90 days); $a's iOS shots are kept"; continue; fi
      mkdir -p "$dir/simulator-$a"
      gh api "repos/$GITHUB_REPOSITORY/actions/artifacts/$id/zip" > "$dir/simulator-$a.zip" &&
        unzip -q -o "$dir/simulator-$a.zip" -d "$dir/simulator-$a" && rm "$dir/simulator-$a.zip" && echo "$a: artifact $id" ||
        { echo "::warning::$a: artifact $id would not download"; rm -rf "${dir:?}/simulator-$a"; }
    done
    ;;
  *) echo "usage: published.sh android|ios <dir> <app>..."; exit 1 ;;
esac
