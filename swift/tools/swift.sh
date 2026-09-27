#!/usr/bin/env bash
# Run a `swift` subcommand (build, run ArkDBTests, …) inside the repository's
# nix devshell with the two things SwiftPM needs on this platform: the
# compiler wrapper beside this script as SWIFT_EXEC, and the Swift runtime
# libraries on LD_LIBRARY_PATH — the manifest SwiftPM compiles and runs has
# no rpath for libdispatch, and a built test binary may not either. Both are
# derived from the devshell's own environment, not written down.
#
#   cd swift && nix develop ..#swift -c tools/swift.sh build
#   cd swift && nix develop ..#swift -c tools/swift.sh run ArkDBTests
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
libs=""
for flag in ${NIX_LDFLAGS:-}; do
  case "$flag" in
    /nix/store/*) libs="$libs:$flag:${flag%%/lib/swift*}/lib" ;;
  esac
done
REAL="$(command -v swiftc)"
ROOT="$(sed -n 's/^prog=\(.*\)\/bin\/swiftc$/\1/p' "$REAL" 2>/dev/null | head -1)"
for d in "$(dirname "$REAL")/../lib/swift/linux" "${ROOT:+$ROOT/lib/swift/linux}"; do
  [ -n "$d" ] && [ -d "$d" ] && libs="$libs:$d"
done
export LD_LIBRARY_PATH="${libs#:}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export SWIFT_EXEC="$HERE/nix-swiftc"
cd "$HERE/.."
exec swift "$@"
