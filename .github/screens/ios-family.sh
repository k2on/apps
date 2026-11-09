#!/usr/bin/env bash
# Of the apps named, those whose simulator build takes the iPad: its
# Info.plist's UIDeviceFamily (TARGETED_DEVICE_FAMILY, as Xcode wrote it)
# has 2. One printed a line; an app with no build, or the iPhone's alone,
# not at all.
#
#   ios-family.sh <apps> <app>...
#
# <apps> as apps-ios.sh's: simulator-<app>/<App>.app.
set -uo pipefail
apps=$1
shift
for a in "$@"; do
  app=$(find "$apps/simulator-$a" -maxdepth 1 -name '*.app' 2>/dev/null | head -1)
  [ -n "$app" ] || continue
  if plutil -extract UIDeviceFamily json -o - "$app/Info.plist" 2> /dev/null | jq -e 'index(2) != null' > /dev/null; then
    echo "$a"
  fi
done
