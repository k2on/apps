#!/usr/bin/env bash
# `strict-ios.sh <step>`: one of flake.nix's `strictIos` builds, from the
# Xcode in the store at $STORE_XCODE (xcode-strict.yaml), with its log, and
# what it made listed.
set -euo pipefail
step=$1
flake="git+file://$PWD?rev=$(git rev-parse HEAD)"
out=$(nix build --impure --print-build-logs --no-link --print-out-paths --expr "
  ((builtins.getFlake \"$flake\").legacyPackages.aarch64-darwin.strictIos
    (builtins.storePath \"$STORE_XCODE\")).$step")
echo "$step: $out"
find "$out" -maxdepth 3 | head -30
for f in "$out"/version "$out"/sdk "$out"/swift "$out"/file; do [ -f "$f" ] && cat "$f"; done
true
