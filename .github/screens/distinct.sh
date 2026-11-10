#!/usr/bin/env bash
# Two shots of one app are two screens: in each directory of <out>
# (`<out>/<theme>/`, or <out> itself), no two PNGs the same bytes. Two that
# are mean a shot never reached its screen (screens-pr-100: money's
# desktop budget, transactions and insights, one picture three times), and
# the capture fails, naming them.
#
#   distinct.sh <out>
set -uo pipefail
failed=0
while IFS= read -r -d '' dir; do
  same=$(find "$dir" -maxdepth 1 -name '*.png' ! -name 'stuck.png' -print0 | xargs -0 -r md5sum | sort | awk '
    { if ($1 == last) { print prev; print $2 } last = $1; prev = $2 }' | sort -u)
  if [ -n "$same" ]; then
    echo "::error::$dir: shots with the same picture: $(xargs -n1 basename <<< "$same" | tr '\n' ' ')"
    failed=1
  fi
done < <(find "$1" -type d -print0)
exit $failed
