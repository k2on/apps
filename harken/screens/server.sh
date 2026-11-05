#!/usr/bin/env bash
# harken's server for its screenshots (.github/screens/apps.md): the demo's
# catalogue written as a media root of tagged files in a fresh data
# directory (harken-seed, harken/seed), then harken-server on it with dev
# auth, its own scanner reading them. Run from the repository's root;
# leaves the server running.
#
#   server.sh <dir> <listen address>
set -euo pipefail
dir=$1
listen=$2
# The seed and the server as nix builds them: the cache's, unless they
# changed. No ffmpeg, no Python: the seed writes the files itself.
seed=$(nix build --no-link --print-out-paths .#harken-seed)
server=$(nix build --no-link --print-out-paths .#harken-server)
"$seed/bin/harken-seed" "$dir/data"
HARKEN_DEV_AUTH=1 HARKEN_DATA="$dir/data" HARKEN_MEDIA="$dir/data/media" \
  nohup "$server/bin/harken-server" "$listen" > "$dir/server.log" 2>&1 &
echo $! > "$dir/pid"
