#!/usr/bin/env bash
# This build's clients against the servers the owner runs (ui/docs/app.md,
# Servers Already Deployed): money's and harken's servers built from an
# older revision of this flake — the one k2on/os pins, by default — and
# this build's clients dialling them, each walked the way a person goes:
#
#   money   the phone host (money/tests/deployed.rs): the first run,
#           the address typed, the dev sign-in, the demo's household synced
#           from the old server, a transaction entered and confirmed there,
#           and seen on a second phone signed in after it.
#   harken  the phone host (harken/tests/deployed.rs), as money's: the
#           first run, the address typed, the dev sign-in, the server's
#           Favorites synced, a playlist made and confirmed there, and seen
#           on a second device; then the listening session on that old
#           server: the phone and the second device (a computer) each
#           stand in it as their host says they are and see the other
#           there, the second plays and the phone's player shows it, the
#           phone pauses and the second sees it paused; then the headless client this build's
#           harken-server package carries (harken-peer, which drives
#           ark-ffi's Peer as a phone does) the same way.
#
# Neither is in CI: it builds the old servers (from Cachix when they are
# there) and both phone hosts. Run it from the repository's root, in the
# dev shell (cargo, curl, python3), before a release that the deployed
# servers will meet:
#
#   .github/compat-check.sh [REV]
#
# REV: the revision the servers are built from (77da9807…, the one k2on/os
# pins: `koon-apps` in its flake.lock). Exits non-zero on the first thing
# that does not round-trip.
set -euo pipefail
rev=${1:-77da98074c9792e96efc643abfe2943eabe4abd7}
cache=(--option extra-substituters https://k2on.cachix.org
  --option extra-trusted-public-keys k2on.cachix.org-1:oI4Db0uW5ER+s4Te/MrXSmjWUWkOc9rmKdzsgUDxQP8=)
work=$(mktemp -d)
pids=()
cleanup() {
  for p in "${pids[@]}"; do kill "$p" 2> /dev/null || true; done
  rm -rf "$work"
}
trap cleanup EXIT

built() { nix build --no-link --print-out-paths "${cache[@]}" "$@"; }
echo "the deployed servers, from k2on/apps $rev"
old=github:k2on/apps/$rev
money_server=$(built "$old#money-server")
money_seed=$(built "$old#money-seed")
harken_server=$(built "$old#harken-server")
echo "this build's harken-peer"
harken_peer=$(built .#harken-server)/bin/harken-peer

port() { python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'; }
# Up once its /healthz says ok: a minute at most.
up() {
  for _ in $(seq 1 300); do
    curl -sf "$1/healthz" | grep -q '^ok' && return 0
    sleep 0.2
  done
  cat "$2" >&2
  echo "::error::$1 did not answer" >&2
  return 1
}

# money: the old seed's household, the old server on it with dev auth.
mkdir -p "$work/money"
"$money_seed/bin/money-seed" "$work/money/data"
money=http://127.0.0.1:$(port)
MONEY_DEV_AUTH=1 ARK_DEV_USER=alex MONEY_DATA="$work/money/data" \
  "$money_server/bin/money-server" "${money#http://}" > "$work/money/server.log" 2>&1 &
pids+=($!)
up "$money" "$work/money/server.log"
echo "money's deployed server: $(curl -sf "$money/healthz" | grep '^domain')"
(cd rust && ARK_DEPLOYED_SERVER=$money cargo test -p money-app --test deployed -- --nocapture)

# harken: the old server, empty, with dev auth; two of this build's peers.
mkdir -p "$work/harken"
harken=http://127.0.0.1:$(port)
HARKEN_DEV_AUTH=1 ARK_DEV_USER=alex HARKEN_DATA="$work/harken/data" \
  "$harken_server/bin/harken-server" "${harken#http://}" > "$work/harken/server.log" 2>&1 &
pids+=($!)
up "$harken" "$work/harken/server.log"
echo "harken's deployed server: $(curl -sf "$harken/healthz" | grep '^domain')"
(cd rust && ARK_DEPLOYED_SERVER=$harken cargo test -p harken-app --test deployed -- --nocapture)
python3 - "$harken_peer" "$harken" "$work/harken" << 'EOF'
import json, subprocess, sys, time

peer, server, work = sys.argv[1:]

def start(name):
    p = subprocess.Popen([peer, "--dir", f"{work}/{name}", "--server", server, "--user", "alex"],
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    def say(cmd, **args):
        p.stdin.write(json.dumps({"cmd": cmd, **args}) + "\n")
        p.stdin.flush()
        return json.loads(p.stdout.readline())
    return p, say

def until(what, f, secs=60):
    end = time.time() + secs
    while True:
        got = f()
        if got:
            return got
        if time.time() > end:
            sys.exit(f"never: {what}")
        time.sleep(0.2)

a, say_a = start("a")
until("the first peer linked", lambda: say_a("status").get("linked"))
name = f"Compat {int(time.time())}"
made = say_a("mutate", name="playlists.create_playlist", args={"name": name})
assert made.get("ok"), made
until("the server confirms the playlist", lambda: say_a("standing", id=made["id"])["standing"] == "confirmed")
assert say_a("status")["held"] == 0, "an intent held: the server has no such function"

b, say_b = start("b")
until("the second peer linked", lambda: say_b("status").get("linked"))
def has():
    say_b("settle", timeout_ms=2000)
    rows = say_b("query", name="playlists.playlists", args={}).get("rows") or []
    return any(r.get("name") == name for r in rows)
until("the second peer has the playlist", has)
for say in (say_a, say_b):
    say("quit")
print(f"harken: {name!r} made on one peer, confirmed by the deployed server, seen on another")
EOF
echo "this build's clients round-trip with the servers k2on/apps $rev builds"
