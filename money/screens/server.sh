#!/usr/bin/env bash
# money's server for its screenshots (.github/screens/apps.md): the demo's
# household written into a fresh data directory (money-seed), then
# money-server on it with dev auth. Run from the repository's root; leaves
# the server running.
#
#   server.sh <dir> <listen address>
set -euo pipefail
dir=$1
listen=$2
# The server and its seed as nix builds them: the cache's, unless they changed.
server=$(nix build --no-link --print-out-paths .#money-server)
"$server/bin/money-seed" "$dir/data"
MONEY_DEV_AUTH=1 MONEY_DATA="$dir/data" nohup "$server/bin/money-server" "$listen" > "$dir/server.log" 2>&1 &
echo $! > "$dir/pid"
