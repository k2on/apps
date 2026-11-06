#!/usr/bin/env bash
# `nix-xcode.sh <checkout> <Xcode.app> <attr>...`: each of the checkout's
# aarch64-darwin packages <attr> built, or fetched from the cache, its store
# path printed on stdout as `<attr> <path>` (everything else on stderr).
#
# The iPhone builds use the runner's installed Xcode, <Xcode.app>, where it
# is (flake.nix, `xcode`: pinned by version and build number, never put in
# the store), selected and checked by ./xcode-pinned.sh. What ios.yaml (the
# TestFlight archive) and app-screens.yaml (the simulator app) both build
# with. Should any plan still fetch or build an Xcode store path, it stops:
# Xcode is kept out of the store, and so out of every cache.
set -uo pipefail
src=$(cd "$1" && pwd)
xcode=$2
shift 2
flake="git+file://$src"
status=0

if [ ! -d "$xcode/Contents/Developer" ]; then
  echo "::error::$xcode is no Xcode (run xcode-pinned.sh first)" >&2
  exit 1
fi
for attr in "$@"; do
  ref="$flake#packages.aarch64-darwin.$attr"
  # Taken whole, then searched: `nix … | grep -q` under pipefail reads a
  # match as a failure when grep leaves before nix has said everything.
  if ! plan=$(nix build --dry-run "$ref" 2>&1); then
    if grep -q "does not provide attribute" <<< "$plan"; then
      echo "$attr: the checkout has no such package" >&2
      status=2
    else
      echo "$plan" >&2
      echo "::error::$attr did not evaluate" >&2
      status=1
    fi
    continue
  fi
  echo "$plan" >&2
  if grep -q -- '-Xcode[^/]*\.app' <<< "$plan"; then
    echo "::error::$attr's plan has an Xcode store path in it: Xcode must stay out of the store (flake.nix, xcode)" >&2
    exit 1
  fi
  echo "$attr: building (or fetching)" >&2
  if path=$(nix build --no-link --print-out-paths --print-build-logs "$ref" 2> >(cat >&2)); then
    echo "$attr $path"
  else
    echo "::error::$attr did not build" >&2
    status=1
  fi
done
exit $status
