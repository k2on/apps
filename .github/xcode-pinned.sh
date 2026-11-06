#!/usr/bin/env bash
# `xcode-pinned.sh <checkout>`: the Xcode the checkout's flake.nix pins
# (`xcode`, by version and build number) selected, and its path put in
# $XCODE for the job. It must be on the runner image: the iPhone builds are
# built from it where it is installed (it is never put in the store, so
# never fetched from or pushed to a cache), and its simulators are what the
# apps run on. Missing, or another build of that version, the job stops and
# says which the image has, for the pin to be moved.
#
# Also put in $GITHUB_ENV: XCODE_NIX_CONF, the nix.conf line that lets the
# kernel's sandboxed build read that one directory (flake.nix, `iosPhone`'s
# `__impureHostDeps`), for the install-nix step's extra_nix_config.
set -euo pipefail
pin() {
  awk -v k="$1" '/xcode = let/ { on = 1 } on && $1 == k { gsub(/[";]/, "", $3); print $3; exit }' "$2/flake.nix"
}
version=$(pin version "$1")
build=$(pin build "$1")
if [ -z "$version" ] || [ -z "$build" ]; then
  echo "::error::no Xcode pin (xcode = let … version, build) found in flake.nix"
  exit 1
fi
xcode=/Applications/Xcode_$version.app
if [ ! -d "$xcode" ]; then
  have=""; for x in /Applications/Xcode_*.app; do have="$have ${x##*/}"; done
  echo "::error::Xcode $version ($build), which flake.nix pins, is not on this runner image: pin one it has ($have)"
  exit 1
fi
# The sandbox lets in this very path: a symbolic link would let in only the link.
real=$(cd "$xcode" && pwd -P)
if [ "$real" != "$xcode" ]; then
  echo "::error::$xcode is a link to $real: pin the directory itself"
  exit 1
fi
sudo xcode-select -s "$xcode/Contents/Developer"
xcodebuild -version
have=$(xcodebuild -version | awk '/^Build version/ { print $3 }')
if [ "$have" != "$build" ]; then
  echo "::error::$xcode is Xcode build $have, not $build, which flake.nix pins: move the pin (xcode's version and build)"
  exit 1
fi
echo "XCODE=$xcode" >> "$GITHUB_ENV"
# Not `extra-`: nix cannot append to this setting, so its macOS defaults
# are named again before the Xcode.
echo "XCODE_NIX_CONF=allowed-impure-host-deps = /System/Library /usr/lib /dev /bin/sh $xcode" >> "$GITHUB_ENV"
