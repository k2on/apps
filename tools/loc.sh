#!/usr/bin/env bash
# Non-blank, non-comment Rust lines per crate, split into shipped source,
# binaries and tests, so a size claim is one reproducible number.
# Usage: tools/loc.sh [crate dirs...]   (default: every crate under rust/)
set -euo pipefail
cd "$(dirname "$0")/.."
count() { cat "$@" 2>/dev/null | grep -vE '^\s*$|^\s*//' | wc -l; }
dirs=("$@"); [ ${#dirs[@]} -eq 0 ] && dirs=(rust/*/ harken/domain)
printf "%-16s %8s %8s %8s %8s\n" crate src bins tests total
for d in "${dirs[@]}"; do
  d=${d%/}; [ -f "$d/Cargo.toml" ] || continue
  all=$(find "$d" -name '*.rs' -not -path '*/target/*')
  tests=$(echo "$all" | grep -E '/tests/|_tests\.rs$|/tests\.rs$|/benches/' || true)
  bins=$(echo "$all" | grep -E '/src/bin/|/src/main\.rs$' || true)
  src=$(echo "$all" | grep -vE '/tests/|_tests\.rs$|/tests\.rs$|/benches/|/src/bin/|/src/main\.rs$' || true)
  s=$( [ -n "$src" ] && count $src || echo 0); b=$( [ -n "$bins" ] && count $bins || echo 0); t=$( [ -n "$tests" ] && count $tests || echo 0)
  printf "%-16s %8s %8s %8s %8s\n" "$(basename $d)" "$s" "$b" "$t" "$((s+b+t))"
done
