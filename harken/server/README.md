# harken-server

The sync server, on the `ark` runtime and nothing generated. It loads one
`.ark` module, hosts every scope in it as an `ark::peer::Authority` inside
one `ark::protocol::Server`, and applies every pushed intent through the
module's own closures (`ark::hash::closures`). Two scopes for harken —
`library` and `playlists` — and it would host a module with twenty the same
way.

## Running

```
harken-server --module harken/domain/harken.ark --data ./harken-data \
              --listen 127.0.0.1:8787 --media /srv/media
```

Every flag is also an environment variable: `HARKEN_MODULE` (required),
`HARKEN_DATA` (default `./harken-data`), `HARKEN_LISTEN` (default
`127.0.0.1:8787`), `HARKEN_MEDIA` (optional). Until `harken.ark` is built,
the demo module works: `nix run .#arkc -- demo /tmp/demo.ark`.

From the repository root, through nix:

```
cd rust && nix develop ../#rust -c cargo run -p harken-server -- --module ../harken/domain/harken.ark
cd rust && nix develop ../#rust -c cargo test -p harken-server
```

## Endpoints

- `GET /sync` — the protocol, over a WebSocket. Frames are binary: a client
  frame is the canonical CBOR of `ClientMsg::to_value`, a server frame of
  `ServerMsg::to_value`. Each connection is one `ConnId` for the machine;
  the server pings every 20 s and closes a socket that leaves three
  unanswered, so no client has to keep anything alive. A frame that is not
  the protocol closes that socket and nothing else.
- `GET /healthz` — plain text: `ok`, the connection count, and the head
  sequence of every hosted scope.
- `GET /media/*` — the media directory, when `--media` is set. **No
  authentication**: anyone who can reach the port can fetch any file under
  it. Bind to loopback or a LAN, or put something in front.

## Dev auth, and it says so

Identity is `ark::protocol::trusting`: the `token` in `Hello` is taken as
the user's name and every login is the session `dev`; access is
`open_access`, so everyone receives every scope. The server prints a line
saying exactly that at every startup. There is no other mode yet.

## Persistence

Each scope's log is `DATA/<scope>.ark-log`: the canonical CBOR of

```
{ t: "log", scope,
  base:    { seq, hash, rows: { table: [row…] } },     -- the snapshot it stands on
  entries: [ { seq, entry, facts: [change…] } … ],     -- above the snapshot
  ids:     [ { id, seq } … ] }                          -- every id ever sequenced
```

`entry` and `change` are the protocol's own encodings (`entry_value`,
`change_value`), so a file carries exactly what a `Batch` frame does. The
whole file is rewritten after every batch of appends — to a temporary file
beside it, then renamed into place — and loaded at startup, where the
snapshot's hash is recomputed and held to what was written and the entries
must run without a gap; a damaged file is refused rather than served. A
scope whose log never moved has no file. Whole-file rewrites are fine at
harken's scale and are the thing to replace first if that changes.

## The scanner

With `--media`, the server walks `MEDIA/music` at startup for `mp3 flac ogg
m4a wav opus` and authors `add_track` for every file whose path (relative
to the media root, the same path `/media/` serves) is not yet in `track`:
title from the file stem, artist from the directory the file is in,
album from the directory above that when there are two under `music`,
`duration_ms` 0. It is an ordinary in-process peer — an
`ark::protocol::Client` holding `library` whole, exchanged with the server
machine directly, no socket — authoring as the user `library`. The fresh id
and the clock for the autos are drawn here and frozen in the entry, the one
place non-determinism enters. A module with no `add_track` gets a line
saying the scanner has nothing to do.

## Shape

`ark::protocol::Server` is sans-io and neither `Send` nor `Sync` (its
authenticator is a `Box<dyn Fn>`), so it is built and lives on one thread
(`hub.rs`); axum's handlers, the scanner and `/healthz` reach it through a
channel handle. That thread also writes the logs, so a disk write never
sits on the reactor.
