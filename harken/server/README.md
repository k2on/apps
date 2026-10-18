# harken-server

harken's server, on `ark-server` and `ark-auth`. The log (hosted with
harken's procedures native), the sync socket at `/sync`, `/healthz`, the
sign-in routes under `/auth`, `/media` and the browser client's validator
are the shared crates'; what is here is what only harken has:

- **`library.rs`** — the media directory as a peer. `music/` under the root
  is walked at start and the whole root is watched after; each audio file
  is read with lofty and authored as `add_song` by an `ark_client::Peer` in
  this process, dialling the hub (no socket), signed in as the account
  `library`. `add_song` is an upsert by `file`: a file the library already
  has is that song said again — retagged, or with a cover it lacked — and
  keeps its id, so a playlist holding it still does; the same file saying
  nothing new writes nothing. What a file was when it was last offered —
  its size, its modification time, and the same of the covers beside it —
  is kept in `DATA/library-files.json`, and a file whose print has not
  moved is not read at all; one that has moved is authored only if the
  entry would change what the replica holds. So a restart over an
  untouched library reads no tags and authors nothing, and a missing or
  unreadable `library-files.json` is one full read and nothing worse. A
  new *directory* is walked (its tracks land before a watch on it exists);
  a picture added, replaced or removed offers the tracks under its
  directory again; audio outside `music/` is not a track; a removed track
  is ignored (the log is permanent, and an unplugged disk looks the same).
- **`covers.rs`** — where a cover comes from, in this order, per track:
  1. a picture beside it: `cover`, `folder`, `front` or `album` (`.jpg`,
     `.jpeg`, `.png`, `.webp`, any case), else the only picture in the
     directory that is not `artist.*`; a disc directory (`CD1`, `Disc 2`)
     with none asks the record's directory above it. Spelt
     `.cover/<v>/music/…/cover.jpg`, which `/media/.cover/<v>/` serves from
     the media root with `<v>` ignored: `<v>` is the file's size and
     modification time, hashed, so a cover replaced under the same name is
     a new value, and no client goes on drawing the one it cached;
  2. else the picture inside the file — the front cover, else the first —
     written once into `DATA/art/<sha256>.<ext>` and spelt
     `.art/<sha256>.<ext>`, which `/media/.art/` serves. The media root may
     be read-only (it is, under the NixOS module), and a hundred tracks
     carrying one picture are one file.

  An artist's picture is only ever a file: `artist.*` beside the record or
  in the directory above it, or `folder.*` in that directory above when it
  is named for the artist (`Artist/Album/track`, Jellyfin's and Kodi's
  layout). Files directly in `music/` are nobody's record. Every value is a
  path every client already joins to `<server>/media/` as it does a track,
  so no client changed; `/media/.art` and `/media/.cover` are reserved, and
  `ServeDir` refuses a `..` in either, written or encoded.
- **The listening session is not here.** It is the domain's live state
  (`harken_domain::listening`, `docs/plan-live.md`): one instance per
  account on `/sync`, sequenced by the engine's own server
  (`ark::live::state::Desk` inside the hub), its transitions written once
  and run natively here and interpreted wherever the module is loaded,
  kept in `live-states.cbor` and restored paused with no output. This crate
  holds no rule of it.
- **`assistant.rs`** — Home Assistant's media players as devices of that
  session. A pure `Bridge`, handed the session and what the house said:
  who holds a speaker, what a change of the session becomes (a hand-off of
  the whole queue from where it was, a pause, a play, a seek), what the
  speaker did that the session does not know said as its own transition
  (`started`, `advance`, `jump`, `stalled`, `play`, `seek`) — where a
  playing speaker is being its `media_position` counted on from
  `media_position_updated_at`, which is when the house last refreshed it,
  read against the house's own clock as its `Date` says; an undated one is
  never a seek; which track it is on read from the URL it reports
  compared as an address, never as a string (`address`: decoded, entities
  and all, the scheme and host in one case, a default port dropped), or
  from its `queue_position` where the URL names its queue — `SETTLE` polls
  of grace after a hand-off, `LAPSE` polls before letting a playing speaker
  go (`release`, said as it), `QUIET` polls after telling it anything, and
  the second taker releasing the first. Its volume and its group are
  reconciled the same way (`tune`): what the house says it supports is said
  once as the speaker's `equip`; a volume or a group that moved in the
  session is told to the house (`volume_set`, `volume_mute`, `join`,
  `unjoin`), and one that moved in the house while the session stood still
  is said as the speaker (`report_volume`, `report_group`), outside `QUIET`;
  a member is never handed a queue. And `ha`, the thread: each speaker a
  participant (`HubHandle::participate`) in each session a client is in and
  in no other — a speaker never keeps a session's presence open — the
  service calls, polling every speaker standing anywhere (its volume and
  group; what it plays only where it is held), the integration it groups
  in read once through the template API, and every hand-off, release,
  pause, join and unjoin said out loud.
- **`lib.rs`** — `Config` from the environment and `start`.

## Running

```
HARKEN_DEV_AUTH=1 harken-server [127.0.0.1:8787]
```

| variable | |
|---|---|
| `HARKEN_DATA` | the log, the kept listening sessions, sign-ins, the scanner's replica, its prints and the covers it took out of files (default: `$TMPDIR/harken-server`; below) |
| `HARKEN_DEV_AUTH=1` | anyone is whoever they say — a laptop only, said loudly at start |
| `HARKEN_OIDC_ISSUER`, `HARKEN_OIDC_CLIENT_ID`, `HARKEN_OIDC_CLIENT_SECRET_FILE` | all three or none; the secret is a file (a systemd credential); `HARKEN_OIDC_SCOPES` |
| `HARKEN_PUBLIC_URL` | where a browser reaches this server (default `http://ADDR`) |
| `HARKEN_REDIRECTS` | comma-separated prefixes a login may return to, besides loopback, `harken://` and the public URL |
| `HARKEN_MEDIA` | the media root: `music/` is scanned, all of it served at `/media`, **unauthenticated** |
| `HARKEN_WEB`, `HARKEN_WEB_MODULE` | the browser client, with the build as its validator; the module names the file whose change is a rebuild off the store |
| `HARKEN_MODULE` | an `.ark` to host instead of harken's own |
| `HARKEN_HA_URL`, `HARKEN_HA_TOKEN_FILE`, `HARKEN_HA_PLAYERS` | all three or none; players are `media_player.x[=Name]`, comma-separated |
| `HARKEN_HA_MEDIA` | where a *speaker* fetches from (default: the public URL) |
| `HARKEN_ROLES` | who holds which role: `library=alice,bob;admin=alice` |
| `HARKEN_ADMIN_BIND` | where the admin page listens (none without it): loopback asks nothing, wider asks for a login holding `admin` (`docs/plan-guards.md` D4) |
| `HARKEN_ADMIN_WEB` | the admin page's build (`nix build .#ark-admin`); without it the admin API alone |

What is in `HARKEN_DATA` (`/var/lib/harken` under the NixOS module), and
whose it is:

| path | |
|---|---|
| `log.ark-log`, `log.ark-journal` | the log: the library, every playlist, every person's work. This *is* the database |
| `modules.cbor` | the modules this server has run, so an older client's entry still applies |
| `cursors.cbor` | how far each session has read, for what the log may forget |
| `live-states.cbor` | each account's listening session as the engine keeps it — its ordered part, under the hash of its type (a queue, a place in it, the clock), restored paused with no output |
| `sessions.json` | the sign-ins: every login this server issued |
| `library/` | the scanner's replica, a peer like any other |
| `library-files.json` | the scanner's prints; local, never the log's |
| `art/` | the pictures taken out of files, by hash |

A library scanned before covers were found needs nothing: the first start
of a server that keeps prints has none, so it reads every file once and
authors an entry for each record whose cover it finds. Dropping the
database is never needed for covers. If it is wanted anyway — to start the
library over — stop the service and delete `log.ark-log` and
`log.ark-journal`, which is every song *and every playlist*; `library/`,
`library-files.json`, `cursors.cbor` and `live-states.cbor` may go with them,
and keeping `sessions.json` keeps everybody signed in. Nothing on a device
needs clearing: a client's `Hello` names the log it was confirmed on, a
server whose log is another answers with its snapshot, and the device
re-bases what it had pending onto that (`fleet.rs`
`a_server_that_lost_its_log_re_bases_everyone_onto_what_it_has`) — so what
the old log held is gone from every device, and its pending work is
authored again on the new one, where an entry naming a song or a playlist
the new log does not have does nothing or is refused, saying why. The scanner's replica is re-based the same
way, and with no song for any file it reads and authors the whole library
again.

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
  flake's to say. `admin.bind` is the admin page's listener,
  `127.0.0.1:8788` by default and null for none, and `admin.page` its
  build, `ark-admin` by default; a bind wider than loopback that nobody in
  `roles.admin` can open is a warning.
- `nix/module-test.nix` — the module evaluated under each of those
  configurations and held to its assertions, warning and unit, without
  building a system.
- `nix/serve.nix` — `harken-serve [ADDR]`, a dev-auth server.

## Tests

`cargo test -p harken-server`: the bridge's rules against values
(`assistant.rs`); the session over real sockets — the bridge the whole way
round against a stand-in Home Assistant, two accounts taking one speaker,
paging under a library of two batches, and a browser, a desktop and a
speaker sharing one session through transfers, a dropped socket and both
ways of coming back (`tests/bridge.rs`); the session's own rules are the
domain's transition tests (`harken/domain/tests/listening.rs`); the scanner
over real WAV files, tags written and read by lofty, covers beside a
record and inside its tracks fetched where the log says, a retagged file
the same song, a rescan of nothing new reading nothing, and `/media` range
requests and `..` (`tests/library.rs`); sign-in, signed-out and older-login work,
restarts (`tests/server.rs`); the web validator through this server
(`tests/web.rs`). Each was falsified once by breaking what it holds.

Not verified: a real Home Assistant or speaker (the stand-in is written from
the REST API's documentation), a real OpenID Connect provider, the NixOS
module on a machine (it is evaluated, never booted), covers in a real
library's files (the tests write the tags, the pictures are a few bytes
nothing decodes, and an MP4's or an Ogg's picture has not been read here),
inotify on a real library of thousands of files, a TLS proxy in front.
