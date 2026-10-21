# harken, on ArkDB

A self-hosted music system: a library a server scans from a directory,
playlists, one listening session per account across every device, and the
speakers in the house as devices in it. All of it runs on ArkDB at spec v5
(`docs/plan-v5.md`): one log, one domain written once in plain Rust and
linked by the server, the desktop and the browser, and every query a view
a client keeps up to date by the rows a change touched.

```
domain/    the domain, in plain Rust (spec/AUTHORING.md): src/schema.rs,
           types.rs, keys.rs, library.rs, browse.rs, playlists.rs;
           listening.rs, queue.rs, devices.rs and groups.rs, the listening
           session as volatile tables (never the log); plays.rs (the log);
           host.rs, what an app knows of it beside its functions
server/    an ark-peer that listens, with harken's own parts: the scanner, the Home Assistant
           bridge, sign-in, /media, the web build, the admin page's listener;
           the NixOS module (server/README.md)
iced/      the desktop and browser client, on arkui and ark-peer; the `demo`
           feature is the seeded library GitHub Pages publishes
```

Frozen at spec v4 with the runtimes they are built on (`kotlin/FROZEN.md`,
`swift/FROZEN.md`), out of the flake, and not following the domain since:

```
android/   the phone: Compose over the Kotlin client, loading v4's harken.ark
           from its assets (android/README.md)
ios/       SwiftUI over ArkDBClient (ios/README.md); never built for iOS
```

## The domain

Eleven tables in one log: `media` and `song` (a track and what is true of
it as a song), `album` and `person`, `work`, `movement`, `recording` and
`credit` (a work is not a recording, and neither is an album),
`playlist` and `playlist_item`, and `play`, a track somebody heard. Every
reference is checked, and `play` has none: it carries the track as it was
heard, so a removed track keeps its plays. `library` is
the router the scanner and the describing verbs are on; `playlists` has the
`owned` middleware, which hands a body the playlist its input names if it
is the caller's and refuses `not your playlist` otherwise.

Four decisions that each show on screen:

- **Playlists are per person.** `playlists` lists the caller's only.
- **A taken name is numbered, not refused.** Creating "Favorites" when that
  person has one keeps the new playlist under its own id as "Favorites
  (1)", then "(2)", decided in `create_playlist` in log order, so work done
  offline or on another device is never dropped. A client makes its default
  "Favorites" only when that person has no playlist.
- **An app works before anyone signs in.** A peer with no account authors
  as nobody, keeps its work pending and on disk, and connects to nothing;
  signing in makes all of it the signer's and syncs it. An older login of
  the same person is accepted by the server as theirs.
- **Every refused change says why.** A rejection carries the domain's own
  sentence, and each client shows it against the change it refused.

Every query is a view, a plan of typed operators (`spec/AUTHORING.md`;
`domain/src/browse.rs` and `playlists.rs`): `library`, `albums`,
`composers` and the rest say what they read and how it joins, so a client
holds any of them as a view (`ark_peer::Peer::view`) that a change moves
by the rows it touched (`docs/plan-v5.md` §1.10); the desktop's lists are
views. The
domain's tests hold it to what v4's was: the read model, the listening
session transition for transition, every query maintained against a fresh
evaluation, and convergence over a simulated fleet (`domain/tests/`).
The crate is the source of truth: the server and the client link it and
call its types and functions as they are. It was ported from ArkLang in N1,
and `domain/tests/port.rs` holds it to what the ArkLang text did: the
schema's hash and, over seeded scripts of every function, the states and
transcripts the two agreed on, pinned.

## The listening session

One per account, across every device of it: the queue, the track, whether
it is playing and from where, and which device makes the sound. It is not
the log — an afternoon of pauses and skips is worth nothing tomorrow — but
the domain's live state (`domain/src/listening.rs`, `docs/plan-live.md`):
volatile tables (`docs/plan-v5.md` §1.9) written once beside the
mutations, sequenced by the hub's volatile authority on `/sync`, kept
across a restart in `log/live` and restored by `on_restore` (paused, with
no output), and run by the interpreter on the server, the desktop and the
browser.

**The state is the instruction.** Every button on every device is a
transition — `start`, `play`, `pause`, `seek`, `skip`, `jump`,
`transfer`, `stop`, and the queue's own edits, `enqueue` (after the
track playing, or at the end), `remove`, `move` and `clear_upcoming`,
none of which interrupts the track playing — and the device the session names as its output makes
its player agree with the session; every other device is silent and draws
it. The output says only what happens to it: `started` when a hand-off
lands, `advance` when a track ends, `stalled` when the sound stops by
itself, `release` when it lets the sound go, and `report_position` when its
player has come apart from the clock by more than 600 ms. A playing clock
carries the server's time it started from, so every device counts the
position itself and nothing heartbeats.

- **Every bar is within 600 ms of what is heard** (`read::SYNC_MS`), on
  every kind of output. The output is right about where it is: a player
  that drifted from a clock nobody touched is followed (`report_position`,
  no player told anything), and a player is sent where the session is only
  when the session moved — a seek, a skip, a hand-off.

- **The output survives its socket**: a device that goes keeps the sound,
  away and paused, and has it back when it returns. Arriving takes
  nothing.
- **A hand-off is a pause where the track was.** At `transfer` the clock
  stops where the track had got to and stands there — on every device's bar
  — until the device it went to says it has `started`; the one that had the
  sound goes quiet at once, and the one taking it starts at that point (a
  speaker that dropped the seek is told again once it has the track). The
  hand-off ends where the device measurably is — for a speaker, a position
  its house has dated — and never at the point handed or the beginning.
- **A press that means a sound** — play, a skip, a track — takes it from an
  output that is not there, for a device that can play. A track pressed
  while nothing is sounding plays on the device it was pressed on.
- **Coming back** (decided): a device that was offline is its own session.
  If the session is sounding elsewhere when it reconnects, its own sound
  stops and it is a controller; if nothing is sounding and it was playing,
  what it was doing becomes the session.
- **The desktop cannot make a sound** (no audio device is wired up), so it
  is a remote control and never the output; the browser can be.
- **A speaker in the house is a device of the session**: the server's Home
  Assistant bridge stands each configured `media_player` in every session
  a client is in, hands it the queue, and says what it does — and a
  speaker that is playing goes on playing the queue with every browser
  closed, standing in the session alone until it has been silent ten
  minutes
  (`server/README.md`). Where it is in the queue is read from the URL it
  says it is playing, compared as an address: a speaker spells back what
  it was handed its own way.
- **Volume is the session's, per device.** A device that has a volume
  says so once it stands in the session (`equip`, with its level) — the
  browser's `<audio>` element, a speaker whose house says it takes
  `volume_set` — and from then on anybody can move it (`set_volume`,
  `nudge_volume`, `set_mute`), the device obeys it as the output obeys the
  clock, and a change made on the device itself is reported
  (`report_volume`). The desktop sounds nothing and has no volume.
- **Speakers of one family join** (a Sonos is a `sonos`): a member follows a
  leader and plays what it plays, so the output is a group when anything
  follows it (`join`, `unjoin`, and the leader's `report_group` for a group
  made in the Sonos app). Handing the sound to a member makes it the
  leader, the rest following it, as Sonos does. A group's volume moves every
  member in proportion (`set_group_volume`, `nudge_group_volume`,
  `set_group_mute`). A phone, a browser and a desktop have no family —
  nothing keeps their audio in step with anybody's — so nothing joins them.
- **`q`** is the queue — the track playing, then up next, in the order it
  will play, the session's or, alone, this device's own: `<Enter>` or a
  click plays a row, `x` takes it out, `J` and `K` move it down and up
  among what plays after the track playing; the button beside the
  device picker's on the bar opens it too, and a row's menu has **Play
  next** and **Add to queue**.
- **`d`** is the picker — every device, the one away drawn and not a target,
  no row that stops it everywhere, this device ticked while nothing plays,
  every member of a playing group ticked, each row with its volume, a `+`
  on a speaker that can join and a `−` on one that can leave, and the group
  first as itself — and **`D`** every number the session holds: the
  instance's sequence and pending calls, the server's clock offset, the
  devices with their volumes, families and leaders, the group, every
  scrubber being dragged (the one ephemeral field), the calls refused. On
  the bar, a mute and a slider for what is playing; **`+`** and **`-`**
  nudge it and **`M`** mutes it — in the picker, the row's; **`J`** there
  joins or unjoins the row.

**A play is the log's**: `record_play`, authored by the output once a
track has played thirty seconds or half its length, once per play — so
`last_played()` and `recently_played()` are there offline and the same on
every device.

It used to be none of this: a hand-written protocol over opaque frames, a
`Desk` in the server deciding where each command went, a second socket at
`/listen` before that, and a copy of the client half in each client.

## Building

Everything is `nix`, from the repository root:

    nix build .#harken-server      # the server, and harken-peer beside it
    nix run .#harken-serve         # …in dev auth: anyone is whoever they say
    nix build .#harken-iced        # the desktop window
    nix build .#harken-web         # the browser demo, as a static directory (Pages)
    nix build .#harken-web-server  # the browser client harken-server serves
    nix build .#ark-admin          # the admin page the server's admin listener serves
    nix build .#harken-domain      # the domain crate, its schema hash held to its pin
    nix flake check                # fmt, clippy and every Rust test; the domain
                                   # builds, its schema hash pinned;
                                   # the vectors and traces; the NixOS module

`nixosModules.default` is `services.harken` (`server/README.md` has its
options). The frozen iOS app is Xcode over `ios/project.yml`, since only a
Mac can build one.

One workflow builds from `main` (`.github/workflows/build.yaml`): it
publishes `harken-web` at <https://k2on.github.io/apps/harken/>. The
Android app is frozen at spec v4: no APK is built, and the F-Droid
repository keeps offering the last one (`kotlin/FROZEN.md` says how to
build it at its revision). That is the workflow once
`docs/build-workflow-v5.patch` is applied; until then its APK job fails,
which does not hold the page back.

## Running it

    mkdir -p media/music && cp -r ~/Music/SomeAlbum media/music/
    HARKEN_MEDIA=$PWD/media nix run .#harken-serve   # 127.0.0.1:8787, dev auth
    nix run .#harken-iced -- --server http://127.0.0.1:8787 --user alice
    nix run .#harken-iced -- --server http://127.0.0.1:8787 --user bob

The scanner adds whatever audio is under `music/` in `HARKEN_MEDIA`, and
anything dropped there later; a file retagged is the same song with its new
tags. A record's cover is a `cover`, `folder`, `front` or `album` picture
beside its tracks (`.jpg`, `.jpeg`, `.png`, `.webp`), else the only picture
there, else the picture inside a track; an artist's is an `artist.*` beside
the record or above it, or the `folder.*` of a directory named for them
(`server/README.md` has the rest, and what `HARKEN_DATA` holds).

Make a playlist in one window and add to it in the other; stop the
server, add on both sides, start it again, and each window's offline
additions land after the other's, in the order the server sequenced them.
A window with no remembered login opens signed out and offers a sign-in
button.

### Starting without a server, connecting later

    nix run .#harken-iced            # no --server: alone

With no `--server` the desktop opens **alone**. A page with no `?server=`
first asks whoever served it: when its `/healthz` answers — a harken
server serving its own page — it opens on that server, as it always did;
from anywhere else (GitHub Pages, a file) it opens alone. Alone is this
device's own replica in a fixed place — `local` under
`$XDG_DATA_HOME/harken`, or `harken:local` in the page's `localStorage` —
and it is not a demo: the peer is its own authority, every change is
sequenced as it is made and kept, and the status line says `alone`. The
line above the table has a server address and **connect**: that joins the
server in place (`docs/plan-alone.md` §4). The chain sequenced alone is
handed to it whole — the server adopts it by its facts and answers with the
sequences it assigned (`docs/plan-v5.md` §1.7) — and that server's sign-in
starts; once it finishes, the changes become the signer's, keeping their
ids, and land after whatever the server already has. The lists on screen are patched through
the join, not rebuilt. The server is remembered for `local`, so the next
start opens there. Leaving a server is not yet a control.

`harken-peer` does the same from a shell: `--alone` and later `--server
URL` over one `--dir` is the join, and `join` and `leave` are commands.

### The explorer, on the desktop and on the server

`E` on the desktop opens the explorer (`ark-explorer`, `docs/plan-guards.md`
D4) over this device's own replica, beside `D`'s numbers: every table it
holds, a page of rows at a time with every column, read under the module's
types (an id labelled by its table, a field by its refinement's name); the
log as this client knows it — its confirmed chain, its pending intents,
where it is and how it is linked; a read-only console; and every query and
mutation of the module as a form, whose checks are the module's own
refinements run by `parse` with the module's words. `j`/`k`, `h`/`l`,
`gg`/`G` and counts move; `<Enter>` opens a table or edits a cell, `x`
deletes a row, `<Tab>` goes between the tables, the log, the console and
the functions, and `<Esc>` goes back, and out from the top. A client
writes only through the CRUD a domain exposes (`insert_<t>`,
`update_<t>`, …), as the signed-in person, and never raw — alone too.
harken exposes none, so here its tables are read-only, and it says so; its
mutations are callable from their forms as a button would author them.

The server's admin page is the same component compiled to wasm, served by
`ark_peer::admin` on a listener of its own (`HARKEN_ADMIN_BIND`;
`services.harken.admin.bind`, `127.0.0.1:8788` by default) over the
authority's store and its log, with who is connected. It writes as the
authority: through a table's CRUD where the domain exposes one, unless `R`
flips the switch, and raw otherwise — a raw write is a function of no input
the authority authors through the same door as any mutation, which checks
the row it replaces is the one there, judged by the constraints and served
to every peer like any entry. harken exposes no
CRUD, so every edit there is raw. On loopback it asks nothing (reach it
from elsewhere through an SSH tunnel); bound wider it asks for a login
holding the `admin` role (`services.harken.roles.admin`), signed in
through this server's own `/auth`. `nix build .#ark-admin` is the page;
the NixOS module serves it by default.

### Asking a running server, and backing it up

    curl http://127.0.0.1:8787/healthz

answers in lines: `ok`; `connections N`; the log's `head` and `horizon`;
`domain <schema hash> <version> current`; and a `room <account> peers N`
line per account with a connection open. What the data directory holds,
and whose each file is, is `server/README.md`'s table: the log is
`log/snapshot` and `log/chain` (the chain of entries, each with its facts,
the state after it and the hash of the chain to it), `log/journal`,
`log/live`, `sessions.json`, and the scanner's `library/`,
`library-files.json` and `art/`.

v4's `arkc backup`, `restore` and `verify-log` are not ported. A stopped
server's data directory is the backup: copy it whole. (On open a chain
is truncated at the first entry whose hash does not check, so one cut
short mid-append opens at its last whole entry; a copy of a running
server's is not otherwise promised to be one moment.)

## harken-peer, and the fleet

`harken-peer` is a peer with no screen, built beside the server (the
`harken-server` package ships both). It opens a replica in a directory,
signs in the way the desktop does in dev auth, dials the sync socket and
pumps when the socket wakes it, after a command that asks something of it,
and at the redial deadline — never on a clock, as the desktop does not
(`docs/plan-reactive.md`), so a frame nobody was woken for is a fleet that
does not converge; it reads commands as JSON lines on stdin and answers one line each
(an id `{"$id":"8-4-4-4-12"}`, an integer a JSON number, a sum
`{"variant":"Name", …fields}`):

    harken-peer --dir /tmp/alice --server http://127.0.0.1:8787 --user alice
    {"cmd":"mutate","name":"create_playlist","args":{"name":"Road trip"}}
    {"ok":true,"id":{"$id":"…"}}
    {"cmd":"settle"}
    {"ok":true,"cursor":1,"pending":0}

`mutate`, `status`, `hash`, `wait`, `settle`, `query`, `rejections`,
`standing`, `disconnect`, `reconnect`, `sign_in`, `sign_out`, `join`,
`leave`, `call`, `live`, `live_refusals`, `persist`, `quit`; the source's
first page says what each answers. `--device KIND,NAME,CAN_PLAY` makes it
a device of its account's listening session, `--module FILE` runs a module
file instead of harken's own, and `--alone` makes it its own authority. Without `--user` it
opens signed out, authoring as nobody until `sign_in`. A sign-in waits on
the server ten seconds to connect and twenty to read (ark-auth's bounds),
or `--auth-patience-ms` for both; one that fails at start ends the
process. Its session revoked (`/auth/logout` with its token), a peer is
told at once on the socket it has — `denied`, "signed out: this login was
revoked" (the hub closes it with 4401) — keeps its store and pending, and
stops dialling until `sign_in`. A server restarted with less log than a
peer has confirmed does not re-base it on v5: that is the fleet's one
witness (`a_server_that_lost_its_log_re_bases_everyone_onto_what_it_has`,
ignored, saying where).

`server/tests/fleet.rs` is the process fleet (`docs/plan-fleet.md`): the
real server and real peers as child processes on loopback, each peer behind
a TCP proxy the test black-holes, cuts, pauses and replays through, and
every scenario ending on one invariant — every replica's confirmed state
hashes as the server's log does, and every accepted intent is in it once.

    cd rust && cargo test -p harken-server --test fleet -- --nocapture   # the timings
    FLEET_LONG=1 cargo test -p harken-server --test fleet the_seeded_fuzz -- --nocapture
    FLEET_SEED=12345 cargo test -p harken-server --test fleet the_seeded_fuzz
    cargo test -p harken-server --test fleet -- --ignored                # the witness

The fuzz runs 20 steps on a fixed seed by default, and 2,000 on a seed from
the clock under `FLEET_LONG=1`; `FLEET_SEED` names one either way, and a
failure prints the seed and the schedule. A scenario marked `#[ignore = "witness: …"]` is a bug
found in the engine and left failing on purpose until it is fixed there. A
failed fleet keeps its directory — the server's log, every replica and every
stderr — and says where; `FLEET_KEEP=1` keeps it always.

`nix build .#fleet-vm` is the same idea on three NixOS machines under the
real `services.harken`: an interface taken down, the service restarted, the
server crashed and booted, files copied into the media directory. The test
asks for the `kvm` system feature, which is why it is a package and not one
of the checks. A machine without `/dev/kvm` can still run it, with qemu
falling back to emulation, by saying it has the feature:

    nix build .#fleet-vm --option system-features "nixos-test benchmark big-parallel kvm"

which took eleven minutes on a four-core container (the test script
itself five and a half, most of it booting the server again after the
crash). It also runs mixed: a fourth machine on the last pinned revision's
server, with alice on that revision's peer and bob on this one's, upgraded
in place under both by `switch-to-configuration` into a specialisation
whose only change is the package; and bob on the old peer against this
revision's server.

## Versions: the fleet run mixed

A deployment lives through releases: phones that are never updated, a
server upgraded in place under the peers using it, a domain that grows a
column. `server/tests/versions.rs` runs the process fleet with older
binaries beside this build's (`docs/plan-db.md` D1); the older ones are
**pinned previous revisions of this repository**, listed in
`nix/versions.nix` and built by nix from each revision's own flake.
**The matrix is empty until a shipped revision is pinned**: the two it held
were development revisions nobody runs, and were dropped
(`docs/plan-db.md` D1). Until one is, `checks.versions` runs the
scenarios that need no older binary and the rest say they are skipped.

    nix flake check                       # checks.versions: every pinned revision, every scenario
    nix build .#harken-server-<name>      # one pinned revision's two binaries
    HARKEN_OLD_1_SERVER=$(nix build --print-out-paths .#harken-server-<name>)/bin/harken-server \
    HARKEN_OLD_1_PEER=$(nix build --print-out-paths .#harken-server-<name>)/bin/harken-peer \
      cargo test -p harken-server --test versions -- --nocapture

`HARKEN_OLD_<n>_SERVER` and `HARKEN_OLD_<n>_PEER` (and `HARKEN_OLD_<n>_NAME`
for what is printed), `n` counting from 1 in `nix/versions.nix`'s order, are
what the scenarios read; without them, the three that need an older binary
(1, 4 and 5) say they are skipped and pass, so `cargo test` on a laptop is
unchanged. The rest need none: they hold §1.2's prefix check on the
schema over next releases written in the test (a table appended, a column
appended).

**To add a release to the matrix**: append `{ name; rev; }` to
`nix/versions.nix`, add the input `harken-<name>` to `flake.nix` with the
same revision (`git+https://github.com/k2on/apps?ref=main&rev=…`, following
this flake's `nixpkgs` and `rust-overlay`), run `nix flake lock`. The flake
refuses to evaluate if the locked revision is not the one `versions.nix`
names. `packages.harken-server-<name>` appears, `checks.versions` runs every
scenario once more against it, and `fleet-vm` runs its mixed machines on
the last entry.

What versions mean on v5 (`docs/plan-v5.md` §1.5, and K2's rules):

- **A server runs exactly the functions its build registered** (§1.16),
  by name; `/healthz` says which domain: its schema's hash and version,
  which every `Hello` carries too.
- **No code crosses the wire.** An intent naming a function the server
  does not run is answered `Held`: its device keeps it pending, and what
  it pushed behind it waits on the server in order, until a server that
  runs it; then all of it is judged. Nothing is refused for being newer.
- **A grown schema keeps the log.** A server started on a module whose
  schema appends to the old one opens its chain under the new schema,
  checking each entry by its hash, never deriving a state again. A replica
  on another schema takes the authority's states on faith and reads every
  row by prefix; it stays usable on everything its schema can see.
  `harken-peer`'s `status` still says `held` and `behind`, always 0 and
  false on v5.

No v5 revision is pinned yet; a revision from before R1 runs spec v4 and
speaks nothing this server reads, so the matrix starts at the first
shipped v5 revision.

## Not verified

The phones are frozen at spec v4 (`kotlin/FROZEN.md`, `swift/FROZEN.md`)
and speak nothing this server reads: no APK is built, and the iOS app has
not met a compiler.
No desktop window has been opened: the browser build was drawn in
headless Chromium under v4 and has not been drawn on v5, and the desktop
is only driven through its tests. Nothing
here has met a real Home Assistant or a real OpenID Connect provider.
The listening session has run between real sockets, peers and the bridge
against a stand-in house, and the reconcilers against a stand-in player;
no browser has played a track through it and no speaker has been handed
one, nor had its volume set or been joined to another: the volume and the
groups are held against a stand-in house's `volume_set`, `volume_mute`,
`join`, `unjoin` and template API, written from Home Assistant's
documentation. A speaker's plays are not recorded, since the bridge is not
a peer of the account.
