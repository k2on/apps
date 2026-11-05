#!/usr/bin/env bash
# money's server for its screenshots (.github/screens/apps.md): the demo's
# household written into a fresh data directory (money-seed, seed/money),
# then money-server on it with dev auth. Run from the repository's root;
# leaves the server running.
#
#   server.sh <dir> <listen address>
set -euo pipefail
dir=$1
listen=$2
# The seed and the server as nix builds them: the cache's, unless they changed.
seed=$(nix build --no-link --print-out-paths .#money-seed)
server=$(nix build --no-link --print-out-paths .#money-server)
"$seed/bin/money-seed" "$dir/data"
MONEY_DEV_AUTH=1 MONEY_DATA="$dir/data" nohup "$server/bin/money-server" "$listen" > "$dir/server.log" 2>&1 &
echo $! > "$dir/pid"
