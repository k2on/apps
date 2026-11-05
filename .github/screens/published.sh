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
    ;;
  *) echo "usage: published.sh android <dir> <app>..."; exit 1 ;;
esac
