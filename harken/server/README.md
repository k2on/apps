# harken-server

harken's server, an `ark-peer` that listens, with `ark-auth`, hosting
`harken-domain`. The log and the listening session's volatile space on
the hub, the sync socket at `/sync`, the sign-in routes under `/auth`, the
browser client's validator and the admin page's listener are the shared
crates'; what is here is what only harken has:

- **`library.rs`** — the media directory as a peer. `music/` under the root
  is walked at start and the whole root is watched after; each audio file
  is read with lofty and authored as `library.add_song` by an
  `ark_peer::Peer` in this process, dialling the hub (no socket),
  signed in as the account
  `library`. `add_song` is an upsert by `file`: a file the library already
  has is that song said again — retagged, or with a cover it lacked — and
  keeps its id, so a playlist holding it still does; the same file saying
  nothing new writes nothing. What a file was when it was last offered —
  its size, its modification time, and the same of the covers beside it —
  is kept as rows of the scanner's own local schema (`HarkenLocal`'s
  `print`, in `DATA/library-files/`), and a file whose print has not
  moved is not read at all; one that has moved is authored only if the
  entry would change what the replica holds. So a restart over an
  untouched library reads no tags and authors nothing, and a missing or
  unreadable `library-files/` is one full read and nothing worse. A
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
  (`harken_domain::listening`, `docs/plan-v5.md` §1.9): volatile tables
  on `/sync`, sequenced by the hub's volatile authority, a connection's
  device said in its volatile `Hello` (`device`), `on_join`, `on_leave`
  and `on_restore` the domain's; kept in `log/live` when the hub closes
  and restored by `on_restore`. This crate holds no rule of it.
- **`hosted.rs`** — the domain the hub runs: harken's, as this build
  compiled it, its functions by name (§1.16).
- **`house.rs`** — the house's door, `POST /house/sign-in`: a peer holding
  the key the server keeps in `HARKEN_HOUSE_KEY`'s file is handed the
  house's own login (`the house@house`, which watches) or a speaker's in
  an account's session (`user@entity`, written as any sign-in is, and
  renewed with a day left). The house itself — Home Assistant's players
  as devices — is `harken-peer --house` (`harken/peer`), a peer of this
  server's socket like any other.
- **`lib.rs`** — `Config` from the environment, `start`, and `/healthz`
  in front of the hub's: `ok`, the connections, the log's `head` and
  `horizon`, `module <hash> current` and each module held, and a `room`
  line per account connected.

## Running

```
HARKEN_DEV_AUTH=1 harken-server [127.0.0.1:8787]
```

| variable | |
|---|---|
| `HARKEN_DATA` | the log (sign-ins among its rows), the kept listening sessions, the session key, the scanner's replica, its prints and the covers it took out of files (default: `$TMPDIR/harken-server`; below) |
| `HARKEN_DEV_AUTH=1` | anyone is whoever they say — a laptop only, said loudly at start |
| `HARKEN_OIDC_ISSUER`, `HARKEN_OIDC_CLIENT_ID`, `HARKEN_OIDC_CLIENT_SECRET_FILE` | all three or none; the secret is a file (a systemd credential); `HARKEN_OIDC_SCOPES` |
| `HARKEN_PUBLIC_URL` | where a browser reaches this server (default `http://ADDR`) |
| `HARKEN_REDIRECTS` | comma-separated prefixes a login may return to, besides loopback, `harken://` and the public URL |
| `HARKEN_MEDIA` | the media root: `music/` is scanned, all of it served at `/media`, **unauthenticated** |
| `HARKEN_WEB`, `HARKEN_WEB_MODULE` | the browser client, with the build as its validator; the module names the file whose change is a rebuild off the store |
| `HARKEN_RETAIN_ENTRIES` | entries the log keeps above its snapshot (10000); `HARKEN_RETAIN_DAYS` is read and keeps nothing on v5 |
| `HARKEN_KEEPALIVE_MS` | how often the sync socket pings (20000); it closes after three unanswered |
| `HARKEN_HOUSE_KEY` | the file the house's key is kept in, made at first start: set, `/house/sign-in` hands `harken-peer --house` its logins. The house's own variables (`HARKEN_HA_*`, `HARKEN_BRIDGE_TRACE`) are `harken-peer --house`'s now (`harken/peer/README.md`); a server given one refuses to start |
| `HARKEN_ROLES` | who holds which role: `library=alice,bob;admin=alice`; an `admin` grants more as rows (`role.grant`) |
| `HARKEN_SCOPED=1` | every connection fed `session.feed`: the library whole, its person's sign-ins and roles only (off: the log whole, as the fleet holds it) |
| `HARKEN_ADMIN_BIND` | where the admin page listens (none without it): loopback asks nothing, wider asks for a login holding `admin` (`docs/plan-guards.md` D4); `ark_peer::admin`, its raw writes and mutations authored by the authority as itself |
| `HARKEN_ADMIN_WEB` | the admin page's build (`nix build .#ark-admin`); without it the admin API alone |

What is in `HARKEN_DATA` (`/var/lib/harken` under the NixOS module), and
whose it is:

| path | |
|---|---|
| `log/kv`, `log/log` | the log: the library, every playlist, every person's work, every sign-in (a `sign_in` row) and granted role — the rows it stands on and the entries above them. This *is* the database |
| `log/local.kv`, `log/local.log` | the hub's own rows (`ark_peer::Local`), in the log's format: each origin's last intent and its actor, the intents refused and those waiting behind a held one, so a push after a restart is judged as before; and the listening sessions' volatile space as the hub last closed it, restored by `on_restore` |
| `session.key` | the key every token is signed with (`ark-auth`); made at first start, never leaves the machine |
| `house.key` | the house's key (`HARKEN_HOUSE_KEY` under the NixOS module): what `harken-house` signs its speakers in with |
| `library/` | the scanner's replica, a peer like any other |
| `library-files/` | the scanner's prints and the index of the pictures it took out of files (`HarkenLocal`, a local instance); never the log's |
| `art/` | the pictures taken out of files, by hash |

A library scanned before covers were found needs nothing: the first start
of a server that keeps prints has none, so it reads every file once and
authors an entry for each record whose cover it finds. Dropping the
database is never needed for covers. If it is wanted anyway — to start the
library over — stop the service and delete `log/`, which is every song *and every
playlist*; `library/` and `library-files/` may go with it,
and with the log every sign-in goes: everybody signs in again. On v5 a device
that held the old log does **not** re-base onto the new one: a server
whose log is empty serves it at sequence 0, which every replica takes for
its own base, and nothing converges (`fleet.rs`
`a_server_that_lost_its_log_re_bases_everyone_onto_what_it_has`, a
witness). Clear the devices' replicas with it.

A server with neither a provider nor `HARKEN_DEV_AUTH=1` refuses to start.
Nothing assumes a peer arrived signed in: a client used before anybody
signed in dials nothing, and pushes its re-stamped work once somebody does;
an entry authored under an older login of the same person is accepted
(`Auth::owns`, through `.auth(..)`).

## Nix

- `nix/module.nix` — the NixOS service, `services.harken`: dev auth or an
  OpenID Connect provider whose secret is a systemd credential, the media
  directory made by tmpfiles and bound read-only, the state in
  `StateDirectory` (`HARKEN_DATA=%S/harken`); with `homeAssistant` a
  second unit, `harken-house` (`harken-peer --house --server` this
  server on this machine, bound to `harken.service`), with Home
  Assistant's token as its credential and the server's dynamic user, so
  it reads the `house.key` the server writes; the assertion that a
  speaker is never handed a URL on this machine, the warning when it is
  handed a LAN URL this server is not bound to, and `homeAssistant.trace`
  for the bridge's trace. The option names are as they were. `web` defaults to null: which build a server should serve is the
  flake's to say. `admin.bind` is the admin page's listener,
  `127.0.0.1:8788` by default and null for none, and `admin.page` its
  build, `ark-admin` by default; a bind wider than loopback that nobody in
  `roles.admin` can open is a warning.
- `nix/module-test.nix` — the module evaluated under each of those
  configurations and held to its assertions, warning and unit, without
  building a system.
- `nix/serve.nix` — `harken-serve [ADDR]`, a dev-auth server.

## Tests

`cargo test -p harken-server`: the session over real sockets — the house
(`harken_peer::house`, against this server's socket) the whole way
round against a stand-in Home Assistant, two accounts taking one speaker,
paging under a library of two batches, and a browser, a desktop and a
speaker sharing one session through transfers, a dropped socket and both
ways of coming back, and a cast to a stand-in that takes a queue the way a
Sonos does through Home Assistant — idle, buffering, playing with no
position, then a position, an early seek dropped — never drawn at the
beginning on any device and within the bound of the speaker from the
moment it starts, a drift followed without a command, the trace, the
house as a process of its own, the door's key, and a press timed to the
house (`tests/bridge.rs`; the bridge's rules against values are
`harken/peer`'s); the session's own rules are the
domain's transition tests (`harken/domain/tests/listening.rs`); the scanner
over real WAV files, tags written and read by lofty, covers beside a
record and inside its tracks fetched where the log says, a retagged file
the same song, a rescan of nothing new reading nothing, and `/media` range
requests and `..` (`tests/library.rs`); sign-in, signed-out and older-login work,
restarts, `/healthz` and the admin page over its wire (`tests/server.rs`);
the web validator through this server (`tests/web.rs`); the process fleet
— the real binaries on loopback, the network taken away and given back,
the admin page's raw write (`tests/fleet.rs`); and versions: which module
may follow which, and the fleet across a moved function
(`tests/versions.rs`). Each was falsified once by breaking what it holds;
what v5 does not yet do is a test ignored as a witness, saying where.

Not verified: a real network run of the fleet off loopback, a pinned
previous v5 revision (`nix/versions.nix` pins none), a real Home Assistant or speaker (the stand-in is written from
the REST API's documentation), a real OpenID Connect provider, the NixOS
module on a machine (it is evaluated, never booted), covers in a real
library's files (the tests write the tags, the pictures are a few bytes
nothing decodes, and an MP4's or an Ogg's picture has not been read here),
inotify on a real library of thousands of files, a TLS proxy in front.
