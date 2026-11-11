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
    # commit, in place of the published ones, as the iPhone's and the
    # desktop's are the commit's; waited for while that run is making them
    # (await-apk.sh). A dry run never falls back to F-Droid's silently:
    # with no build run at the commit, or one that was cancelled or failed
    # before an app's APK, it fails, saying so (app-screens 43069095661 took
    # F-Droid's APKs, which have no ready signal, when another dispatch had
    # cancelled its build, and every Android job then failed waiting). Only
    # an app whose `apk` job succeeded and built nothing — its APK's inputs
    # seen built before (build.yaml's `apk-seen`), unmoved — keeps
    # F-Droid's, with a warning. An app that fails so is left out (no APK of
    # it at all), the others fetched, and the script fails at its end.
    if [ -n "${COMMIT:-}" ]; then
      # Asked until it answers (a refusal, "API rate limit exceeded", is
      # no answer: in app-screens 42965705977 its text was taken for the
      # run's id, and that run waited for for an hour).
      run=
      for try in 1 2 3 4 5 6 7 8 9 10; do
        if said=$(gh api "repos/${GITHUB_REPOSITORY:?}/actions/workflows/build.yaml/runs?head_sha=$COMMIT&per_page=1" --jq '.workflow_runs[0].id // empty' 2> /dev/null); then
          run=$said
          break
        fi
        echo "the API did not answer for build's run at ${COMMIT:0:7} (try $try)"
        sleep $((try * 30))
      done
      if [ -n "$run" ] && ! [[ $run =~ ^[0-9]+$ ]]; then
        echo "::error::build's run at ${COMMIT:0:7}: the API said '$run'"
        rm -rf "${dir:?}"/apk-*
        exit 1
      fi
      if [ -z "$run" ]; then
        echo "::error::no build run at ${COMMIT:0:7}: a dry run takes that commit's APKs, so dispatch build.yaml on the branch at that commit first"
        rm -rf "${dir:?}"/apk-*
        exit 1
      fi
      for a in "$@"; do
        got=$(mktemp -d)
        code=0
        bash "$(dirname "$0")/await-apk.sh" "$run" "$a" "$got" || code=$?
        if [ "$code" = 0 ]; then
          rm -rf "${dir:?}/apk-$a"
          mv "$got/apk-$a" "$dir/apk-$a"
          echo "$a: build's APK at ${COMMIT:0:7} (run $run)"
        elif [ "$code" = 3 ]; then
          # Why none: the run cancelled or failed, or the app's job skipped
          # its build as seen before.
          conclusion=$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run" --jq .conclusion)
          job=$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run/jobs" --paginate --jq ".jobs[] | select(.name == \"apk ($a)\") | .conclusion" | head -1)
          if [ "$job" = success ]; then
            echo "::warning::$a: build's run $run at ${COMMIT:0:7} built no APK (its inputs seen built before); F-Droid's is taken"
          else
            echo "::error::$a: build's run $run at ${COMMIT:0:7} ended ${conclusion:-unfinished} with no APK (its apk job: ${job:-none}); not taking F-Droid's in its place, so its Android shots are not taken. Dispatch build.yaml again at that commit, then this run."
            rm -rf "${dir:?}/apk-$a"
            bad=1
          fi
        else
          echo "::error::$a: build's APK at ${COMMIT:0:7} (run $run) would not come; its Android shots are not taken"
          rm -rf "${dir:?}/apk-$a"
          bad=1
        fi
      done
    fi
    # The other apps' shots are still taken (the emulators' jobs run on
    # what was fetched), and this fails once they are listed.
    exit "${bad:-0}"
    ;;
  *) echo "usage: published.sh android <dir> <app>..."; exit 1 ;;
esac
