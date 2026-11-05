#!/usr/bin/env bash
# This run's captures laid over a tree of them: each <app>/<phone>/ taken
# whole (its device.json, light and dark) replaces what the tree had of it;
# one taken part way is left out, so the tree keeps what it had.
#
#   apps-lay.sh <captures> <tree>
set -euo pipefail
new=$1
tree=$2
shopt -s nullglob
for at in "$new"/*/*/; do
  at=${at%/}
  phone=$(basename "$at")
  a=$(basename "$(dirname "$at")")
  if ! [ -f "$at/device.json" ] || ! [ -d "$at/light" ] || ! [ -d "$at/dark" ]; then
    echo "::warning::$a $phone was taken only in part; kept as it was"
    continue
  fi
  rm -rf "${tree:?}/$a/$phone"
  mkdir -p "$tree/$a"
  cp -r "$at" "$tree/$a/$phone"
  rm -f "$tree/$a/$phone"/stuck.png "$tree/$a/$phone"/stuck.tree.txt
done
