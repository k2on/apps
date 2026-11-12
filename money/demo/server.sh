#!/usr/bin/env bash
# money's server for its screenshots (.github/screens/apps.md): the demo's
# household written into a fresh data directory (money-seed, money/core's `demo`),
# then money-server on it with dev auth. Run from the repository's root;
# leaves the server running. Seeded up to DEMO_DAY (`2026-11-12`) when it
# is set, today otherwise: app-screens.yaml gives every job the run's one
# day, so the phones' servers (and the Mac's demo, app-mac.sh) are one
# household on one day.
#
#   [DEMO_DAY=<day>] server.sh <dir> <listen address>
set -euo pipefail
dir=$1
listen=$2
# The seed and the server as nix builds them: the cache's, unless they changed.
# One build of both: each `nix build` evaluates the flake again, which on
# the Mac was half a minute apiece (app-screens 43318181211: the seed's
# ~60 s were two evaluations, not seeding).
for p in $(nix build --no-link --print-out-paths .#money-seed .#money-server); do
  case $p in *money-seed*) seed=$p ;; *money-server*) server=$p ;; esac
done
"$seed/bin/money-seed" "$dir/data" ${DEMO_DAY:+"$DEMO_DAY"}
MONEY_DEV_AUTH=1 MONEY_DATA="$dir/data" nohup "$server/bin/money-server" "$listen" > "$dir/server.log" 2>&1 &
echo $! > "$dir/pid"
