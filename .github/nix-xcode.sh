#!/usr/bin/env bash
# `nix-xcode.sh <checkout> <Xcode.app> <attr>...`: each of the checkout's
# aarch64-darwin packages <attr> built, or fetched from the cache, its store
# path printed on stdout as `<attr> <path>` (everything else on stderr).
#
# The iPhone builds take Xcode as an input pinned by its hash (flake.nix,
# `xcode`). Only when something must be built is it wanted: then
# <Xcode.app>, the runner's own, is put in the store as it is (nix-store
# --add-fixed), once, and only if it is that very Xcode — a different one
# is said, and nothing is built. What ios.yaml (the TestFlight archive) and
# app-screens.yaml (the simulator app) both build with.
set -uo pipefail
src=$(cd "$1" && pwd)
xcode=$2
shift 2
flake="git+file://$src"
added=
status=0

# Whether building <ref> would build Xcode's placeholder: Xcode not in the store.
wants_xcode() {
  local plan
  plan=$(nix build --dry-run "$1" 2>&1)
  grep -q -- '-Xcode_[^/]*\.app\.drv' <<< "$plan"
}
for attr in "$@"; do
  ref="$flake#packages.aarch64-darwin.$attr"
  if ! nix eval --raw "$ref.drvPath" > /dev/null 2>&1; then
    echo "$attr: the checkout has no such package" >&2
    status=2
    continue
  fi
  # Taken whole, then searched: `nix … | grep -q` under pipefail reads a
  # match as a failure when grep leaves before nix has said everything.
  if [ -z "$added" ] && wants_xcode "$ref"; then
    echo "$attr: to be built, so $xcode into the store" >&2
    start=$(date +%s)
    nix-store --add-fixed --recursive sha256 "$xcode" >&2
    echo "  added in $(( $(date +%s) - start ))s" >&2
    added=1
    if wants_xcode "$ref"; then
      echo "::error::$xcode is not the Xcode flake.nix pins (its hash differs): pin this runner's (nix hash path), or select the one pinned" >&2
      exit 1
    fi
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
