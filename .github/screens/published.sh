#!/usr/bin/env bash
# Each app's last published build, for screenshots taken without building
# (app-screens.yaml, apps.md): what the phones were last given.
#
#   published.sh android <dir> <app>...   each APK as the F-Droid repository
#                                         offers it ($FDROID, its index's
#                                         newest for the package whose id
#                                         ends in .<app>), at
#                                         <dir>/apk-<app>/<app>.apk
#
# The iPhone's needs no such thing: nix builds it, or has it cached
# (ios-build.sh).
# An app with nothing published is said and left out; the others are still
# fetched.
set -uo pipefail
phone=$1
dir=$2
shift 2
case $phone in
  android)
    index=$(curl -fsSL --retry 3 "$FDROID/index-v1.json") || { echo "::error::no F-Droid index at $FDROID"; exit 1; }
    for a in "$@"; do
      # Every app's id is the flake's bundlePrefix and its name: the
      # package whose id ends in .<app>.
      pkg=$(jq -r --arg a "$a" '.packages | keys[] | select(endswith("." + $a))' <<< "$index" | head -1)
      name=$(jq -r --arg p "$pkg" '.packages[$p] // [] | max_by(.versionCode) | .apkName // empty' <<< "$index")
      if [ -z "$name" ]; then echo "::warning::F-Droid offers no $pkg; $a's Android shots are kept"; continue; fi
      mkdir -p "$dir/apk-$a"
      curl -fsSL --retry 3 -o "$dir/apk-$a/$a.apk" "$FDROID/$name" && echo "$a: $name" ||
        { echo "::warning::$a: $name would not download"; rm -rf "${dir:?}/apk-$a"; }
    done
    # COMMIT (a dry run, app-screens.yaml): the APKs build built at that
    # commit (its pull request's run), in place of the published ones, as
    # the iPhone's and the desktop's are the commit's; waited for while that
    # run is making them (await-apk.sh). An app it built none of (its
    # inputs had not moved from main's) keeps what F-Droid offers.
    if [ -n "${COMMIT:-}" ]; then
      run=$(gh api "repos/${GITHUB_REPOSITORY:?}/actions/workflows/build.yaml/runs?head_sha=$COMMIT&per_page=1" --jq '.workflow_runs[0].id // empty')
      if [ -z "$run" ]; then
        echo "no build run at ${COMMIT:0:7}: the published APKs"
      else
        for a in "$@"; do
          got=$(mktemp -d)
          code=0
          bash "$(dirname "$0")/await-apk.sh" "$run" "$a" "$got" || code=$?
          if [ "$code" = 0 ]; then
            rm -rf "${dir:?}/apk-$a"
            mv "$got/apk-$a" "$dir/apk-$a"
            echo "$a: build's APK at ${COMMIT:0:7} (run $run)"
          elif [ "$code" != 3 ]; then
            echo "::error::$a: build's APK at ${COMMIT:0:7} (run $run) would not come"
            exit 1
          fi
        done
      fi
    fi
    ;;
  *) echo "usage: published.sh android <dir> <app>..."; exit 1 ;;
esac
