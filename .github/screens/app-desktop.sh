#!/usr/bin/env bash
# An app's desktop screenshots (apps.md), the same for every app: what is
# the app's is its <app>/demo/shots.json, its `desktop`. The window nix
# built at the commit, run with no window by arkui-shots (ui/rust/shots,
# asked for by ARK_SHOTS): it signs in to the server CI started
# (app-server.sh) as the window would, the server's dev sign-in coming
# straight back to its loopback with no browser, then does each shot's
# steps and draws it in light and in dark.
#
#   app-desktop.sh <out> <window> <app> [server URL]
#
# <window> is the build (`<app>-iced`, with bin/<app>-iced). Writes
# <out>/device.json, <out>/<theme>/<id>.png and beside each <id>.tree.txt,
# the texts a step can name. Run from the repository's root.
set -euo pipefail
out=$1
window=$2
app=$3
server=${4:-}
user=$(jq -r '.server.user // empty' "$app/demo/shots.json")
bin=$(find -L "$window/bin" -maxdepth 1 -name "$app-iced" | head -1)
[ -n "$bin" ] || { echo "::error::no $app-iced in $window"; exit 1; }

# What it is drawn with, pinned by the flake's nixpkgs so that every run
# draws the same: wgpu on Mesa's lavapipe, Vulkan on the CPU (the runner
# has no GPU), DejaVu for the window's default sans-serif, a Linux
# desktop's usual, and Noto Color Emoji, in place of whatever fonts the
# runner has.
fonts=$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#dejavu_fonts)
# And an emoji font, Noto's colour one: with DejaVu alone every emoji (a
# category's, a reaction) was drawn as a box. Both in one directory of
# links, the only fonts the window may use.
emoji=$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#noto-fonts-color-emoji)
pinned=$(mktemp -d)
ln -s "$fonts/share/fonts" "$pinned/dejavu"
ln -s "$emoji/share/fonts" "$pinned/emoji"
mesa=$(nix build --no-link --print-out-paths --inputs-from . 'nixpkgs#mesa^drivers')
icd=$(find "$mesa/share/vulkan/icd.d" -name 'lvp_icd.*.json' | head -1)
[ -n "$icd" ] || { echo "::error::no lavapipe in $mesa"; exit 1; }

# A home of its own: nothing remembered, so the first shot signs in and
# the rest open signed in, as the phones' do.
home=$(mktemp -d)
args=()
[ -z "$server" ] || args+=(--server "$server")
[ -z "$user" ] || args+=(--user "$user")
mkdir -p "$out"
echo "$app desktop"
HOME=$home XDG_CONFIG_HOME=$home/.config XDG_DATA_HOME=$home/.local/share XDG_CACHE_HOME=$home/.cache \
  ARK_SHOTS="$PWD/$app/demo/shots.json" ARK_SHOTS_OUT="$out" \
  ARK_SHOTS_FONTS="$pinned" VK_ICD_FILENAMES="$icd" \
  timeout 900 "$bin" "${args[@]}"
rm -rf "$home" "$pinned"
# Each shot its own screen: none the same picture as another.
bash "$(dirname "$0")/distinct.sh" "$out"
