#!/usr/bin/env bash
# An app's browser screenshots (apps.md), the same for every app: what is
# the app's is its <app>/screens/shots.json, its `web`. The page nix built
# at the commit (`nix build .#<app>-web`: index.html beside pkg/), served
# here on the loopback, photographed by Playwright's Chromium
# (web-shots.js): each shot opened by its address — the launch arguments
# every shell reads, `?screen=budget`, and arkui-web's `?context=`,
# `?menu=`, `?popover=` — then its steps, at each size in light and dark.
#
#   app-web.sh <out> <site> <app>     (ARK_WEB_SHOTS: another shots.json, to take a few)
#
# ARK_WEB_MODE=ssg takes a static site (an app's whose `[web] modes` has it)
# by its routes' paths, and ARK_WEB_JS=0 with script off (web-shots.js).
#
# Writes <out>/device.json and <out>/<size>/<theme>/<id>.png, each with
# its <id>.tree.txt (the page's text, what a step can name). Run from the
# repository's root, where `node` can `require('playwright')` with its
# Chromium installed (`npx playwright install chromium` on a runner).
set -euo pipefail
out=$1
site=$2
app=$3
here=$(cd "$(dirname "$0")" && pwd)
[ -f "$site/index.html" ] || { echo "::error::no index.html in $site"; exit 1; }

# What it is drawn with, pinned so that every run draws the same: DejaVu,
# the desktop's shots' sans-serif (app-desktop.sh), as the only font
# fontconfig knows; the flake's nixpkgs' when nix is here, else the
# system's.
fonts=/usr/share/fonts/truetype/dejavu
if command -v nix > /dev/null; then
  pinned=$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#dejavu_fonts 2> /dev/null) && fonts=$pinned/share/fonts
fi
[ -d "$fonts" ] || { echo "::error::no DejaVu at $fonts"; exit 1; }
conf=$(mktemp -d)
cat > "$conf/fonts.conf" <<EOF
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <dir>$fonts</dir>
  <cachedir>$conf/cache</cachedir>
  <!-- Every family the page names is DejaVu Sans: fontconfig answers any
       name with its nearest font, and a name it has never heard of
       (-apple-system) would otherwise come back as DejaVu Serif. -->
  <match target="pattern"><edit name="family" mode="prepend_first"><string>DejaVu Sans</string></edit></match>
</fontconfig>
EOF

# The page on a free port of the loopback, stopped however this ends.
port=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$site" > /dev/null 2>&1 &
server=$!
trap 'kill $server 2> /dev/null; rm -rf "$conf"' EXIT
for _ in $(seq 50); do
  curl -sf "http://127.0.0.1:$port/" > /dev/null && break
  sleep 0.1
done

mkdir -p "$out"
echo "$app web"
FONTCONFIG_FILE=$conf/fonts.conf timeout 900 node "$here/web-shots.js" "${ARK_WEB_SHOTS:-$PWD/$app/screens/shots.json}" "http://127.0.0.1:$port" "$out"
