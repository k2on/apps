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
  (`harken_domain::listening`, `docs/plan-v5.md` §1.9): volatile tables
  on `/sync`, sequenced by the hub's volatile authority, a connection's
  device said in its volatile `Hello` (`device`), `on_join`, `on_leave`
  and `on_restore` the domain's; kept in `log/live` when the hub closes
  and restored by `on_restore`. This crate holds no rule of it.
- **`hosted.rs`** — the module the hub runs: harken's own, or a module file
  (`HARKEN_MODULE`: the binding map and every body, `hosted::write`), and
  the module files held as run before (`HARKEN_OLD_MODULES`), each one this
  module must follow — §1.2's prefix check on the schema plus one verify
  per new hash (`hosted::compat`).
- **`assistant.rs`** — Home Assistant's media players as devices of that
  session. A pure `Bridge`, handed the session and what the house said:
  who holds a speaker, what a change of the session becomes (a hand-off of
  the whole queue from where it was, a pause, a play, a seek), what the
  speaker did that the session does not know said as its own transition
  (`started`, `advance`, `jump`, `stalled`, `play`, `seek`,
  `report_position`) — where a playing speaker is being its
  `media_position` counted on from `media_position_updated_at`, which is
  when the house last refreshed it, read against the house's own clock as
  its answers' `Date`s narrow it down (`when::Skew`); an undated one is no
  position at all, and one dated before the bridge last told the speaker
  something is not believed for `STALE` polls; a speaker more than
  `SYNC_MS` (600 ms, every output's bound) from where the session counts it
  is said where it is (`report_position`, the clock re-anchored and no
  player told anything), more than `DRIFT_MS` its own `seek`, and a
  session that moved by more than the bound is told to it; a stall is two
  silent polls in a row (`STALL`), at a point the house dated since it last
  played; which track it is on read from the URL it reports
  compared as an address, never as a string (`address`: decoded, entities
  and all, the scheme and host in one case, a default port dropped), or
  from its `queue_position` where the URL names its queue; a hand-off from
  half way through a track that the speaker starts from the beginning (the
  seek after the enqueue dropped before it loaded) is sought again once it
  has the track, `SEEKS` times at most, and `started` is said only once it
  is there and the house has dated where — at that measured point, counted
  on to when it is said (`as_of`), on the track handed or any after it
  (`advance` or `jump` first), never at the point handed; `WAIT` polls of
  fetching before it stops waiting — `SETTLE` polls
  of grace after a hand-off, `LAPSE` polls before letting a playing speaker
  go (`release`, said as it), `QUIET` polls after telling it anything, and
  the second taker releasing the first. Its volume and its group are
  reconciled the same way (`tune`): what the house says it supports is said
  once as the speaker's `equip`; a volume or a group that moved in the
  session is told to the house (`volume_set`, `volume_mute`, `join`,
  `unjoin`), and one that moved in the house while the session stood still
  is said as the speaker (`report_volume`, `report_group`), outside `QUIET`;
  a member is never handed a queue. Tracks put into the session's queue
  after the one playing or at its end reach the speaker's own queue in
  place (`play_media` with `enqueue: next`, last first, and `enqueue:
  add`), so nothing stops; a track taken out or moved is a fresh hand-off
  from where the track playing is. And `ha`, the thread: each speaker a
  device — a connection to the hub in this process — in each session a client
  is in;
  where no client is, the speakers making the sound — the output and
  whoever follows it — go on standing, reconciled and reporting, while it
  sounds and for `IDLE` (ten minutes) after it stops, and then go, pausing
  what they held, the session kept paused as any is; a speaker that is not
  making the sound never keeps a session's presence open — the
  service calls, polling every speaker standing anywhere (its volume and
  group; what it plays only where it is held), the integration it groups
  in read once through the template API, and every hand-off, release,
  pause, join and unjoin said out loud. Everything it tells the house goes
  before the next poll, so a reading is always of what was told; a handed
  queue's first track and its seek go at once and the rest `ha::PUSH` at a
  time between polls; and with `HARKEN_BRIDGE_TRACE=1` each poll of each
  speaker is a line (below).
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
| `HARKEN_DATA` | the log, the kept listening sessions, sign-ins, the scanner's replica, its prints and the covers it took out of files (default: `$TMPDIR/harken-server`; below) |
| `HARKEN_DEV_AUTH=1` | anyone is whoever they say — a laptop only, said loudly at start |
| `HARKEN_OIDC_ISSUER`, `HARKEN_OIDC_CLIENT_ID`, `HARKEN_OIDC_CLIENT_SECRET_FILE` | all three or none; the secret is a file (a systemd credential); `HARKEN_OIDC_SCOPES` |
| `HARKEN_PUBLIC_URL` | where a browser reaches this server (default `http://ADDR`) |
| `HARKEN_REDIRECTS` | comma-separated prefixes a login may return to, besides loopback, `harken://` and the public URL |
| `HARKEN_MEDIA` | the media root: `music/` is scanned, all of it served at `/media`, **unauthenticated** |
| `HARKEN_WEB`, `HARKEN_WEB_MODULE` | the browser client, with the build as its validator; the module names the file whose change is a rebuild off the store |
| `HARKEN_MODULE` | a module file to host instead of harken's own |
| `HARKEN_OLD_MODULES` | module files, comma-separated, whose bodies are held as run before; each must be one this module follows |
| `HARKEN_RETAIN_ENTRIES` | entries the log keeps above its snapshot (10000); `HARKEN_RETAIN_DAYS` is read and keeps nothing on v5 |
| `HARKEN_KEEPALIVE_MS` | how often the sync socket pings (20000); it closes after three unanswered |
| `HARKEN_HA_URL`, `HARKEN_HA_TOKEN_FILE`, `HARKEN_HA_PLAYERS` | all three or none; players are `media_player.x[=Name]`, comma-separated |
| `HARKEN_HA_MEDIA` | where a *speaker* fetches from (default: the public URL) |
| `HARKEN_BRIDGE_TRACE=1` | a line per poll per speaker: what the house said of it and what the bridge did (below) |
| `HARKEN_ROLES` | who holds which role: `library=alice,bob;admin=alice` |
| `HARKEN_ADMIN_BIND` | where the admin page listens (none without it): loopback asks nothing, wider asks for a login holding `admin` (`docs/plan-guards.md` D4); `ark_peer::admin`, its raw writes and mutations authored by the authority as itself |
| `HARKEN_ADMIN_WEB` | the admin page's build (`nix build .#ark-admin`); without it the admin API alone |

What is in `HARKEN_DATA` (`/var/lib/harken` under the NixOS module), and
whose it is:

| path | |
|---|---|
| `log/snapshot`, `log/chain` | the log: the library, every playlist, every person's work — the snapshot it stands on and the chain above it. This *is* the database |
| `log/journal` | each session's last intent and the intents refused, so a push after a restart is judged as before |
| `log/live` | the listening sessions' volatile space as the hub last closed it, restored by `on_restore` |
| `sessions.json` | the sign-ins: every login this server issued |
| `library/` | the scanner's replica, a peer like any other |
| `library-files.json` | the scanner's prints; local, never the log's |
| `art/` | the pictures taken out of files, by hash |

A library scanned before covers were found needs nothing: the first start
of a server that keeps prints has none, so it reads every file once and
authors an entry for each record whose cover it finds. Dropping the
database is never needed for covers. If it is wanted anyway — to start the
library over — stop the service and delete `log/`, which is every song *and every
playlist*; `library/` and `library-files.json` may go with it,
and keeping `sessions.json` keeps everybody signed in. On v5 a device
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

### When a speaker is not where the bars say

The bridge is tested against a stand-in Home Assistant, and a real house
with a real Sonos has differed from it each time something went wrong. The
trace is what shows the difference: one line per poll of each speaker
standing, saying exactly what the house said and what the bridge made of
it. Turn it on with `services.harken.homeAssistant.trace = true;` (which
sets `HARKEN_BRIDGE_TRACE=1`) and rebuild, or set that variable by hand
for a server run any other way. Then:

1. Play something on a browser or a phone, half a minute into a track.
2. Note the time, and pick the speaker.
3. Wait half a minute with the speaker playing; if the bars are behind or
   jumped back, say by how much and when.
4. Send back the journal from a few seconds before the time noted to the
   end — `journalctl -u harken --since "HH:MM:SS" | grep 'house:'` — and
   what you saw and when.

A line reads:

```
harken-server: house: trace: media_player.kitchen: state "playing", media_content_id "http://10.0.0.2:8787/media/music/a.mp3", media_position 41.0, media_position_updated_at "2026-10-18T21:07:23.112000+00:00", Date Sun, 18 Oct 2026 21:07:24 GMT; speaker 41934 ms; session 41890 ms; track 0 of 12 (handed 0); started; quiet 0, waited 3, sought 1, silent 0; nothing done
```

— the house's answer as it gave it, then where the bridge counts the
speaker (`(dated before the last command)` when the house has not yet
dated anything since it was told something; `undated` when it gave no
position), where it has the session (`standing` while the hand-off is in
flight, at the point handed), where in the queue, and what it did:
`started`, `report_position`, `seek`, `stalled` with the point each was
said at, `media_seek` and the rest told to the house. Turn it off again
afterwards: it is a line a second for every speaker standing.

## Nix

- `nix/module.nix` — the NixOS service, `services.harken`: dev auth or an
  OpenID Connect provider whose secret is a systemd credential, the media
  directory made by tmpfiles and bound read-only, the state in
  `StateDirectory` (`HARKEN_DATA=%S/harken`), the house with its token as a
  credential, the assertion that a speaker is never handed a URL on this
  machine, the warning when it is handed a LAN URL this server is not
  bound to, and `homeAssistant.trace` for the bridge's trace. `web` defaults to null: which build a server should serve is the
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
ways of coming back, and a cast to a stand-in that takes a queue the way a
Sonos does through Home Assistant — idle, buffering, playing with no
position, then a position, an early seek dropped — never drawn at the
beginning on any device and within the bound of the speaker from the
moment it starts, a drift followed without a command, and the trace
(`tests/bridge.rs`); the session's own rules are the
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
