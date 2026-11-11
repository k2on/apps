#!/usr/bin/env bash
# On Linux, before any Mac is asked for: every part of the named Mac
# packages that is built on Linux (flake.nix, `appleKernel`: each app's
# Rust kernel, cross-compiled for the phone, the simulator and the Mac),
# built here, so that the Mac only fetches it and runs Xcode. The Mac
# cannot build an x86_64-linux derivation; it can only substitute one.
#
#   apple-ahead.sh <flake> <out> <attr>...
#
# <flake> as nix takes it: `.`, or a checkout's path (not `git+file:` of a
# shallow clone, which nix refuses).
# <attr> is an aarch64-darwin package (`money-ios-simulator`,
# `harken-ios-archive`, `money-macos`); one the flake does not have is said
# and passed over. Writes the built store paths to <out>, one a line, for a
# cache push or `nix copy`.
set -uo pipefail
flake=$1
out=$2
shift 2
: > "$out"
drvs=()
for attr in "$@"; do
  if ! drv=$(nix eval --raw "$flake#packages.aarch64-darwin.$attr.drvPath" 2> "${TMPDIR:-/tmp}/apple-ahead.err"); then
    if grep -q "does not provide attribute" "${TMPDIR:-/tmp}/apple-ahead.err"; then
      echo "$attr: no such Mac package here; passed over"
      continue
    fi
    cat "${TMPDIR:-/tmp}/apple-ahead.err" >&2
    echo "::error::$attr did not evaluate"
    exit 1
  fi
  # Of everything it is built from, what is Linux's and a Mac build's own
  # input (what those need on Linux is built with them). Either form of
  # `derivation show`: newer nix nests them under `derivations`, without
  # the store's prefix.
  mapfile -t linux < <(nix derivation show -r "$drv" 2> /dev/null | jq -r '
    (.derivations // .) as $d
    | [$d | to_entries[] | select(.value.system != "x86_64-linux") | (.value.inputDrvs // .value.inputs.drvs // {}) | keys[]]
    | map(ltrimstr("/nix/store/")) | unique[]
    | select(($d[.] // $d["/nix/store/" + .]).system == "x86_64-linux")
    | "/nix/store/" + .')
  # One that is itself Linux's (a kernel asked for by name).
  if [ "$(nix eval --raw "$flake#packages.aarch64-darwin.$attr.system" 2> /dev/null)" = x86_64-linux ]; then linux+=("$drv"); fi
  echo "$attr: ${#linux[@]} derivations of it are Linux's"
  drvs+=("${linux[@]}")
done
[ ${#drvs[@]} -gt 0 ] || { echo "nothing of them is built on Linux"; exit 0; }
refs=()
for d in $(printf '%s\n' "${drvs[@]}" | sort -u); do refs+=("$d^*"); done
start=$(date +%s)
nix build --no-link --print-out-paths --print-build-logs "${refs[@]}" > "$out" || exit 1
echo "built or fetched $(wc -l < "$out") paths in $(( $(date +%s) - start ))s"
