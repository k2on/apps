#!/usr/bin/env bash
# `ios-archive.sh <App> <store path> <build> [<version>]`: nix's unsigned
# archive of the app (flake.nix, `<app>-ios-archive`) made
# $RUNNER_TEMP/<App>.xcarchive, writable, for ../ios-upload.sh to sign and
# send, given its build number: the commit's time (ios.yaml says why). The
# archive is built with project.yml's, so that nix's hash moves only with
# what the app is built from; an archive is not signed until it is
# exported, so its build number is set here, in the app's Info.plist and the
# archive's own. Its bundle id is project.yml's. A release (a `v*` tag,
# docs/releasing.md) gives it its version too, the tag's without the `v`:
# App Store Connect takes one to three numbers, so `v1.2` or `v1.2.3`.
# With CHANNEL=unstable (the unstable TestFlight app, a hook no app turns
# on yet: workflows/ios.yaml) it is made the unstable channel's app
# (nix/channels.nix): its bundle id, and each extension's under it, with
# `.unstable` after the app's, and its name with " Unstable" after it.
set -euo pipefail
App=$1
from=$2
build=$3
version=${4:-}
if [ -n "$version" ] && ! [[ "$version" =~ ^[0-9]+(\.[0-9]+){0,2}$ ]]; then
  echo "ios-archive.sh: version $version is not one to three numbers (tag vX.Y or vX.Y.Z)" >&2
  exit 1
fi
to="$RUNNER_TEMP/$App.xcarchive"
rm -rf "$to"
cp -R "$from/$App.xcarchive" "$to"
chmod -R u+w "$to"
plist="$to/Products/Applications/$App.app/Info.plist"
have=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist")
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $build" "$plist"
/usr/libexec/PlistBuddy -c "Set :ApplicationProperties:CFBundleVersion $build" "$to/Info.plist"
if [ -n "$version" ]; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$plist"
  /usr/libexec/PlistBuddy -c "Set :ApplicationProperties:CFBundleShortVersionString $version" "$to/Info.plist" 2> /dev/null \
    || /usr/libexec/PlistBuddy -c "Add :ApplicationProperties:CFBundleShortVersionString string $version" "$to/Info.plist"
fi
if [ "${CHANNEL:-stable}" = unstable ]; then
  base=$have
  while IFS= read -r -d '' p; do
    id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$p")
    case $id in
      "$base" | "$base".*) /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $base.unstable${id#"$base"}" "$p" ;;
    esac
  done < <(find "$to/Products/Applications/$App.app" -name Info.plist -print0)
  /usr/libexec/PlistBuddy -c "Set :ApplicationProperties:CFBundleIdentifier $base.unstable" "$to/Info.plist"
  shown=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleDisplayName' "$plist" 2> /dev/null || /usr/libexec/PlistBuddy -c 'Print :CFBundleName' "$plist")
  /usr/libexec/PlistBuddy -c "Set :CFBundleDisplayName $shown Unstable" "$plist" 2> /dev/null \
    || /usr/libexec/PlistBuddy -c "Add :CFBundleDisplayName string $shown Unstable" "$plist"
  have=$base.unstable
fi
echo "$App: $have, version $(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$plist") ($build)"
