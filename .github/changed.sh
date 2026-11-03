#!/usr/bin/env bash
# `changed.sh BEFORE PATH...`: `true` when a PATH changed between BEFORE and
# HEAD, else `false`; `true` too when BEFORE is no commit this clone has (a
# new branch, a forced push, a run by hand), so nothing is skipped on doubt.
# What decides whether an app is sent to TestFlight (workflows/ios.yaml): an
# app whose own inputs did not move is built, and not published again.
set -euo pipefail
before=${1:-}
shift
if [ -z "$before" ] || [ "$before" = 0000000000000000000000000000000000000000 ] \
  || ! git cat-file -e "$before^{commit}" 2>/dev/null; then
  echo true
elif git diff --quiet "$before" HEAD -- "$@"; then
  echo false
else
  echo true
fi
