#!/usr/bin/env bash
# `xcode-pinned.sh <checkout>`: the Xcode the checkout's flake.nix pins
# (`xcode`, by name and hash) selected, and its path put in $XCODE for the
# job. It must be on the runner image: the iPhone builds are built from it
# (../nix-xcode.sh adds it to the store when one must be built), and its
# simulators are what the apps run on. Missing, the job stops and says which
# the image has, for the pin to be moved.
set -euo pipefail
name=$(grep -o 'name = "Xcode_[^"]*"' "$1/flake.nix" | head -1 | cut -d'"' -f2)
xcode=/Applications/$name
if [ ! -d "$xcode" ]; then
  have=""; for x in /Applications/Xcode_*.app; do have="$have ${x##*/}"; done
  echo "::error::$name, which flake.nix pins, is not on this runner image: pin one it has ($have)"
  exit 1
fi
sudo xcode-select -s "$xcode/Contents/Developer"
xcodebuild -version
echo "XCODE=$xcode" >> "$GITHUB_ENV"
