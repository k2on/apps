# harken-server

harken's server, on `ark-server` and `ark-auth`. The log (hosted with
harken's procedures native), the sync socket at `/sync`, `/healthz`, the
sign-in routes under `/auth`, `/media` and the browser client's validator
are the shared crates'; what is here is what only harken has:

- **`library.rs`** — the media directory as a peer. `music/` under the root
  is walked at start and the whole root is watched after; each audio file
  whose path is not already a song is read with lofty and authored as
  `add_song` by an `ark_client::Peer` in this process, dialling the hub
  (no socket), signed in as the account `library`. A new *directory* is
  walked (its tracks land before a watch on it exists); audio outside
  `music/` is not a track; a removal is ignored (the log is permanent, and an
  unplugged disk looks the same). The replica catches up with the log
  before the first walk, so a restart authors nothing.
- **`listening.rs`** — the `Desk`: one audio session per account, as an
  `ark_server::Live` over live rooms. Exactly one output; the output
  survives its socket; a command goes to the output, unless it cannot be
  reached and the asker is audible and pressed something that means a
  sound; a hand-off is one `Start` and reads *connecting* until the new
  output reports; a room that empties is kept (paused, only its output,
  away) and woken from on the next join, across restarts.
- **`assistant.rs`** — Home Assistant's media players as devices. A pure
  `Bridge` (who holds a speaker, what a command becomes, `SETTLE` polls of
  grace after a hand-off, `LAPSE` polls before letting a playing speaker
  go, the second taker releasing the first) and `ha`, the thread that
  stands each speaker in every room somebody is listening in, drives the
  six service calls, polls only what it holds, and says every hand-off,
  release and pause out loud.
- **`lib.rs`** — `Config` from the environment and `start`.

## Running

```
HARKEN_DEV_AUTH=1 harken-server [127.0.0.1:8787]
```

| variable | |
|---|---|
| `HARKEN_DATA` | the log, kept rooms, sessions, the scanner's replica (default: `$TMPDIR/harken-server`) |
| `HARKEN_DEV_AUTH=1` | anyone is whoever they say — a laptop only, said loudly at start |
| `HARKEN_OIDC_ISSUER`, `HARKEN_OIDC_CLIENT_ID`, `HARKEN_OIDC_CLIENT_SECRET_FILE` | all three or none; the secret is a file (a systemd credential); `HARKEN_OIDC_SCOPES` |
| `HARKEN_PUBLIC_URL` | where a browser reaches this server (default `http://ADDR`) |
| `HARKEN_REDIRECTS` | comma-separated prefixes a login may return to, besides loopback, `harken://` and the public URL |
| `HARKEN_MEDIA` | the media root: `music/` is scanned, all of it served at `/media`, **unauthenticated** |
| `HARKEN_WEB`, `HARKEN_WEB_MODULE` | the browser client, with the build as its validator; the module names the file whose change is a rebuild off the store |
| `HARKEN_MODULE` | an `.ark` to host instead of harken's own |
| `HARKEN_HA_URL`, `HARKEN_HA_TOKEN_FILE`, `HARKEN_HA_PLAYERS` | all three or none; players are `media_player.x[=Name]`, comma-separated |
| `HARKEN_HA_MEDIA` | where a *speaker* fetches from (default: the public URL) |

A server with neither a provider nor `HARKEN_DEV_AUTH=1` refuses to start.
Nothing assumes a peer arrived signed in: a client used before anybody
signed in dials nothing, and pushes its re-stamped work once somebody does;
an entry authored under an older login of the same person is accepted
(`Auth::owns`, through `.auth(..)`).

## Nix

- `nix/module.nix` — the NixOS service, `services.harken`: dev auth or an
  OpenID Connect provider whose secret is a systemd credential, the media
  directory made by tmpfiles and bound read-only, the state in
  `StateDirectory` (`HARKEN_DATA=%S/harken`), the house with its token as a
  credential, the assertion that a speaker is never handed a URL on this
  machine, and the warning when it is handed a LAN URL this server is not
  bound to. `web` defaults to null: which build a server should serve is the
  flake's to say.
- `nix/module-test.nix` — the module evaluated under each of those
  configurations and held to its assertions, warning and unit, without
  building a system.
- `nix/serve.nix` — `harken-serve [ADDR]`, a dev-auth server.

## Tests

`cargo test -p harken-server`: the desk's rules against a hub with standing
devices (`tests/listening.rs`); the bridge the whole way round over real
sockets against a stand-in Home Assistant (`tests/bridge.rs`); the scanner
over real WAV files, tags written and read by lofty, and `/media` range
requests (`tests/library.rs`); sign-in, signed-out and older-login work,
restarts (`tests/server.rs`); the web validator through this server
(`tests/web.rs`). Each was falsified once by breaking what it holds.

Not verified: a real Home Assistant or speaker (the stand-in is written from
the REST API's documentation), a real OpenID Connect provider, the NixOS
module on a machine (it is evaluated, never booted), inotify on a real
library of thousands of files, a TLS proxy in front.
