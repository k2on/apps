#!/usr/bin/env bash
# Each iPhone app as nix builds it at a commit, for its screenshots
# (apps.md): `nix build .#<app>-ios-simulator` in that commit's checkout
# (flake.nix, `iosSimulator`), copied out as <out>/simulator-<app>/<App>.app.
#
#   ios-build.sh <checkout> <Xcode.app> <out> <app>...
#
# An app whose inputs have not moved is the cache's, and nothing is built.
# Only when one must be built is Xcode wanted: the flake pins it by its
# hash, and <Xcode.app> — the runner's own — is put in the store as it is
# (nix-store --add-fixed), once, and only if it is that very Xcode; a
# different one is said, and nothing is built from it. An app the commit
# has no such package for is said and left out; one that fails to build
# is said, and the others are still built.
set -uo pipefail
src=$(cd "$1" && pwd)
xcode=$2
out=$3
shift 3
flake="git+file://$src"
added=
failed=0

for a in "$@"; do
  attr="$flake#packages.aarch64-darwin.$a-ios-simulator"
  if ! nix eval --raw "$attr.drvPath" > /dev/null 2>&1; then
    echo "$a: this commit has no $a-ios-simulator; skipped"
    continue
  fi
  # Must anything be built, Xcode with it? A dry run says what would be.
  if [ -z "$added" ] && nix build --dry-run "$attr" 2>&1 | grep -q -- '-Xcode_[^/]*\.app\.drv'; then
    echo "$a: to be built, so $xcode into the store"
    start=$(date +%s)
    nix-store --add-fixed --recursive sha256 "$xcode"
    echo "  added in $(( $(date +%s) - start ))s"
    added=1
    if nix build --dry-run "$attr" 2>&1 | grep -q -- '-Xcode_[^/]*\.app\.drv'; then
      echo "::error::$xcode is not the Xcode flake.nix pins (its hash differs): pin this runner's, or select the one pinned"
      exit 1
    fi
  fi
  echo "$a: building (or fetching) $a-ios-simulator"
  if ! path=$(nix build --no-link --print-out-paths --print-build-logs "$attr"); then
    echo "::error::$a-ios-simulator did not build"
    failed=1
    continue
  fi
  mkdir -p "$out/simulator-$a"
  cp -R "$path"/*.app "$out/simulator-$a/"
  chmod -R u+w "$out/simulator-$a"
  echo "$a: $path"
done
exit $failed
