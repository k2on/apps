#!/usr/bin/env bash
# `nix-xcode.sh <checkout> <Xcode.app> <attr>...`: each of the checkout's
# aarch64-darwin packages <attr> built, or fetched from the cache, its store
# path printed on stdout as `<attr> <path>` (everything else on stderr).
# They are built by one `nix build`, side by side where nothing orders them.
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
# Each attr whose plan passed the guard below, and its derivation.
attrs=()
drvs=()
for attr in "$@"; do
  ref="$flake#packages.aarch64-darwin.$attr"
  # Evaluated once: its derivation, which the plan and the build then take
  # as it is, with nothing evaluated again (each had evaluated the flake,
  # which under a busy runner was minutes). Its time said, so that it shows.
  start=$(date +%s)
  if ! drv=$(nix eval --raw "$ref.drvPath" 2> "${TMPDIR:-/tmp}/nix-xcode.err"); then
    if grep -q "does not provide attribute" "${TMPDIR:-/tmp}/nix-xcode.err"; then
      echo "$attr: the checkout has no such package" >&2
      status=2
    else
      cat "${TMPDIR:-/tmp}/nix-xcode.err" >&2
      echo "::error::$attr did not evaluate" >&2
      status=1
    fi
    continue
  fi
  echo "$attr: evaluated in $(( $(date +%s) - start ))s, $drv" >&2
  # Taken whole, then searched: `nix … | grep -q` under pipefail reads a
  # match as a failure when grep leaves before nix has said everything.
  plan=$(nix build --dry-run "$drv^*" 2>&1)
  echo "$plan" >&2
  if grep -q -- '-Xcode[^/]*\.app' <<< "$plan"; then
    echo "::error::$attr's plan has an Xcode store path in it: Xcode must stay out of the store (flake.nix, xcode)" >&2
    exit 1
  fi
  attrs+=("$attr")
  drvs+=("$drv")
done
[ ${#drvs[@]} -gt 0 ] || exit $status
# The machine, and how many builds nix runs at once on it: every attr
# is built by one `nix build`, so what does not depend on another (two
# apps; the simulator's ArkUI and the Mac's) is built at the same time,
# and what they share (`arkuiSwift`) once.
echo "this Mac: $(sysctl -n hw.ncpu 2> /dev/null || nproc) cores, $(( $(sysctl -n hw.memsize 2> /dev/null || echo 0) / 1073741824 )) GB; nix: max-jobs $(nix config show max-jobs 2> /dev/null), cores $(nix config show cores 2> /dev/null)" >&2
refs=()
for d in "${drvs[@]}"; do refs+=("$d^out"); done
echo "building (or fetching) ${attrs[*]} at once" >&2
start=$(date +%s)
# One that fails is said, and the others still built (--keep-going).
nix build --no-link --keep-going --print-build-logs "${refs[@]}" 2> >(cat >&2) || true
echo "built or fetched in $(( $(date +%s) - start ))s" >&2
# Each one's path, in the order asked: built above, so only looked up
# (one that failed is not built again).
for i in "${!attrs[@]}"; do
  attr=${attrs[$i]}
  path=$(nix-store -q --outputs "${drvs[$i]}" | head -1)
  if [ -n "$path" ] && nix path-info "$path" > /dev/null 2>&1; then
    echo "$attr $path"
  else
    echo "::error::$attr did not build" >&2
    status=1
  fi
done
exit $status
