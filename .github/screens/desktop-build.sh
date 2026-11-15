#!/usr/bin/env bash
# Each desktop window as nix builds it at a commit, for its screenshots
# (apps.md): `<app>-desktop` or `<app>-iced` (flake.nix, `icedApp`), the cache's when its
# inputs have not moved, linked as <out>/desktop-<app>.
#
#   desktop-build.sh <checkout> <out> <app>...
#
# An app the commit has no such package for is said and left out; one that
# fails to build is said, and the others are still built.
set -uo pipefail
src=$1
out=$2
shift 2
mkdir -p "$out"
failed=0
for a in "$@"; do
  # An Ark app's window is generated (`<app>-desktop`); one written by
  # hand is `<app>-iced`.
  package=
  for p in "$a-desktop" "$a-iced"; do
    has=$(nix eval --json "$src#packages.x86_64-linux" --apply "p: p ? \"$p\"" 2>/dev/null)
    if [ "$has" = true ]; then package=$p; break; fi
  done
  if [ -z "$package" ]; then
    echo "$a: this commit has no $a-desktop or $a-iced; skipped"
    continue
  fi
  if nix build --print-build-logs -o "$out/desktop-$a" "$src#$package"; then
    echo "$a: $(readlink "$out/desktop-$a")"
  else
    echo "::error::$a's desktop window did not build"
    failed=1
  fi
done
exit $failed
