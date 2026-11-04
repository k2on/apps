#!/usr/bin/env bash
# harken's server for its screenshots (.github/screens/apps.md): the demo's
# catalogue made a media root of tagged files (media.py), then harken-server
# on it with dev auth, its own scanner reading them. Run from the
# repository's root; leaves the server running.
#
#   server.sh <dir> <listen address>
set -euo pipefail
dir=$1
listen=$2
here=$(dirname "$0")
command -v ffmpeg >/dev/null || { if command -v brew >/dev/null; then brew install ffmpeg; else sudo apt-get update -qq && sudo apt-get install -y -qq ffmpeg; fi; } >/dev/null
(cd rust && cargo build --locked --release -p harken-server --bins)
python3 -I "$here/media.py" harken/iced/src/catalogue.json "$dir/media"
HARKEN_DEV_AUTH=1 HARKEN_DATA="$dir/data" HARKEN_MEDIA="$dir/media" \
  nohup rust/target/release/harken-server "$listen" > "$dir/server.log" 2>&1 &
echo $! > "$dir/pid"
