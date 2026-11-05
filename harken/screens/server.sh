#!/usr/bin/env bash
# harken's server for its screenshots (.github/screens/apps.md): the demo's
# catalogue made a media root of tagged files (media.py), then harken-server
# (nix's) on it with dev auth, its own scanner reading them. Run from the
# repository's root; leaves the server running.
#
#   server.sh <dir> <listen address>
set -euo pipefail
dir=$1
listen=$2
here=$(dirname "$0")
# The server as nix builds it (the cache's, unless it changed), and ffmpeg,
# which media.py makes the files with, from the flake's own nixpkgs: no
# Homebrew, no apt, no cargo.
server=$(nix build --no-link --print-out-paths .#harken-server)
ffmpeg=$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#ffmpeg-headless.bin)
PATH="$ffmpeg/bin:$PATH" python3 -I "$here/media.py" harken/iced/src/catalogue.json "$dir/media"
HARKEN_DEV_AUTH=1 HARKEN_DATA="$dir/data" HARKEN_MEDIA="$dir/media" \
  nohup "$server/bin/harken-server" "$listen" > "$dir/server.log" 2>&1 &
echo $! > "$dir/pid"
