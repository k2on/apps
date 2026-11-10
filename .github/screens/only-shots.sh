#!/usr/bin/env bash
# The design loop's: of each app's shots, only those a dry run asked for
# (app-screens' `shots` input), the phones' and the desktop's alike, by id.
# Rewrites <app>/screens/shots.json in the job's checkout, which every
# driver reads; the commit's own is untouched. Nothing asked: nothing done.
#
#   only-shots.sh "<id> <id>…" <app>...
#
# An app none of whose shots is asked for keeps none, and its drivers
# take nothing but its sign-in.
set -euo pipefail
ids=$1
shift
[ -n "${ids// /}" ] || exit 0
want=$(jq -Rc '[splits("[ ,]+")] | map(select(. != ""))' <<< "$ids")
for a in "$@"; do
  f=$a/screens/shots.json
  [ -f "$f" ] || continue
  jq --argjson w "$want" '
    .shots |= map(select(.id as $i | $w | index($i)))
    | if .desktop then .desktop.shots |= map(select(.id as $i | $w | index($i))) else . end' "$f" > "$f.only"
  mv "$f.only" "$f"
  echo "$a: $(jq -c '[.shots[].id]' "$f") (desktop $(jq -c '[.desktop.shots[]?.id]' "$f"))"
done
