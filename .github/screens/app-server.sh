#!/usr/bin/env bash
# An app's own server on this runner, for its screenshots (apps.md): the
# app's <app>/demo/server.sh, which starts it with dev auth and
# something in it, on the port its shots.json names. Then waits until it
# answers /healthz, and until its log's head has stood still for five
# seconds (a scanner done adding). Nothing here is any one app's.
#
#   app-server.sh <app> <dir>
#
# The server's pid is left in <dir>/pid and its log in <dir>/server.log,
# by the app's script. Run from the repository's root.
set -euo pipefail
app=$1
dir=$2
port=$(jq -r '.server.port' "$app/demo/shots.json")
mkdir -p "$dir"
# Who its dev sign-in signs in when a login names nobody (ark-auth): the
# phone's sign-in sheet then opens and comes straight back, with no page
# to fill in.
ARK_DEV_USER=$(jq -r '.server.user // empty' "$app/demo/shots.json")
export ARK_DEV_USER
# …and the picture they wear (a file, from the repository's root): the
# screens show a face, not only initials (nix/demo/README.md).
picture=$(jq -r '.server.picture // empty' "$app/demo/shots.json")
if [ -n "$picture" ]; then ARK_DEV_PICTURE=$(realpath "$picture"); export ARK_DEV_PICTURE; fi
bash "$app/demo/server.sh" "$dir" "127.0.0.1:$port"

health() { curl -sf "http://127.0.0.1:$port/healthz" || true; }
for _ in $(seq 1 60); do
  [ -n "$(health)" ] && break
  sleep 1
done
[ -n "$(health)" ] || { cat "$dir/server.log" || true; echo "::error::$app's server did not answer"; exit 1; }

last=
still=0
for _ in $(seq 1 300); do
  head=$(health | awk '/^head/ {print $2}')
  if [ "$head" = "$last" ]; then still=$((still + 1)); else still=0; fi
  [ "$still" -ge 5 ] && break
  last=$head
  sleep 1
done
echo "$app's server on 127.0.0.1:$port, its log at $(health | awk '/^head/ {print $2}')"
