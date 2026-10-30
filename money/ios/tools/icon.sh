#!/usr/bin/env bash
# The app's icon from tools/icon.svg: 1024 x 1024, opaque RGB (App Store
# Connect refuses an icon with alpha). Run from money/ios with Inkscape 1.x:
#
#   bash tools/icon.sh Money/Assets.xcassets/AppIcon.appiconset/icon-1024.png
set -eu
inkscape "$(dirname "$0")/icon.svg" --export-type=png --export-filename="$1" \
  --export-png-color-mode=RGB_8 -w 1024 -h 1024 >/dev/null 2>&1
