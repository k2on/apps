#!/usr/bin/env bash
# `ios-archive.sh <App> <store path> <build>`: nix's unsigned
# archive of the app (flake.nix, `<app>-ios-archive`) made
# $RUNNER_TEMP/<App>.xcarchive, writable, for ../ios-upload.sh to sign and
# send, given its build number: the commit's time (ios.yaml says why). The
# archive is built with project.yml's, so that nix's hash moves only with
# what the app is built from; an archive is not signed until it is
# exported, so its build number is set here, in the app's Info.plist and the
# archive's own. Its bundle id is project.yml's.
set -euo pipefail
App=$1
from=$2
build=$3
to="$RUNNER_TEMP/$App.xcarchive"
rm -rf "$to"
cp -R "$from/$App.xcarchive" "$to"
chmod -R u+w "$to"
plist="$to/Products/Applications/$App.app/Info.plist"
have=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist")
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $build" "$plist"
/usr/libexec/PlistBuddy -c "Set :ApplicationProperties:CFBundleVersion $build" "$to/Info.plist"
echo "$App: $have, version $(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$plist") ($build)"
