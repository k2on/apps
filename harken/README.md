# harken, on ArkDB

A self-hosted music system: a library a server scans from a directory,
playlists, one listening session per account across every device, and the
speakers in the house as devices in it. All of it runs on ArkDB: one log,
one domain written once in Rust, applied natively by the server, the
desktop and the browser, and every query a plan a client can keep up to
date by the rows a change touched.

```
domain/    the domain, in the vocabulary of spec/AUTHORING.md (src/); harken.ark,
           what it emits; listening.rs, the listening session as a live state
           (never the log) and the plays (the log)
server/    ark-server with harken's own parts: the scanner, the Home Assistant
           bridge, sign-in, /media, the web build; the NixOS module
           (server/README.md)
iced/      the desktop and browser client, on arkui and ark-client; the `demo`
           feature is the seeded library GitHub Pages publishes
android/   the phone: Compose over the Kotlin client, loading harken.ark
           from its assets and interpreting it (android/README.md,
           kotlin/README.md, docs/plan-phones.md)
ios/       the same app in SwiftUI over the Swift client, loading the same
           harken.ark (ios/README.md); never built for iOS
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

Every query is a plan (`spec/AUTHORING.md` §1.5): `library`, `albums`,
`composers` and the rest say what they read and how it joins, so a client
can hold any of them as an `ark_client::View` that a change moves by the
rows it touched; the desktop's library is one. `domain/tests/agreement.rs`
holds every mutator's native Rust to the interpreter over `harken.ark` and
every mutator's hash to its v3 value, and fails while the committed module
differs from what the source emits.
When `domain/src` changes:

    cd rust && cargo run -p harken-domain -- ../harken/domain/harken.ark

## The listening session

One per account, across every device of it: the queue, the track, whether
it is playing and from where, and which device makes the sound. It is not
the log — an afternoon of pauses and skips is worth nothing tomorrow — but
the domain's live state (`domain/src/listening.rs`, `docs/plan-live.md`):
written once beside the mutations, held by the server, sequenced per
account on `/sync`, kept across a restart paused and with no output, and
run natively by the server and the desktop and interpreted from
`harken.ark` by anything else.

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

    nix build .#harken-server      # the server
    nix run .#harken-serve         # …in dev auth: anyone is whoever they say
    nix build .#harken-iced        # the desktop window
    nix build .#harken-web         # the browser demo, as a static directory (Pages)
    nix build .#harken-web-server  # the browser client harken-server serves
    nix build .#harken-domain      # harken.ark, as the source emits it
    nix flake check                # fmt, clippy and every Rust test; harken.ark
                                   # is what the domain emits and it verifies;
                                   # the vectors; the NixOS module

`nixosModules.default` is `services.harken` (`server/README.md` has its
options). The iOS app is Xcode over `ios/project.yml`, since only a Mac
can build one.

One workflow, `.github/workflows/build.yaml`, builds from `main`: it
publishes `harken-web` at <https://k2on.github.io/apps/harken/>, builds
`harken-apk`, the Android app over the Kotlin client (`nix build
.#harken-apk`, `android/README.md`), and publishes that as the F-Droid
repository (`android/fdroid/README.md`). Each build is skipped when its
inputs have not moved.

The Kotlin client has a headless peer too, `harken-peer-kt`
(`nix build .#harken-peer-kt`): `harken-peer`'s lines on another runtime,
which `checks.fleet-kotlin` runs beside Rust peers — through partitions and
restarts, alone and joining, a lost log, the version matrix's two shapes
and a scoped module (`docs/plan-fleet.md`).

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
server in place (`docs/plan-alone.md` §4). Everything done alone is handed
to it — the replica goes back to where it last shared the server's log,
which for a device that never had one is nothing, and every local change is
pending again, in order — and that server's sign-in starts; once it
finishes, the changes become the signer's and are pushed, landing after
whatever the server already has. The lists on screen are patched through
the join, not rebuilt. The server is remembered for `local`, so the next
start opens there. Leaving a server is `ark_client::Peer::leave`, an API
and not yet a control.

`harken-peer` does the same from a shell: `--alone` and later `--server
URL` over one `--dir` is the join, and `join` and `leave` are commands.

### The explorer, on the desktop and on the server

`E` on the desktop opens the explorer (`ark-explorer`, `docs/plan-guards.md`
D4) over this device's own replica, beside `D`'s numbers: every table it
holds, a page of rows at a time with every column; the log as a client
knows it — its pending intents, where it is, how it is linked, its
`Verify` answers; and a read-only console that runs a domain query by name
or a plan in the IR's JSON form. `j`/`k`, `h`/`l`, `gg`/`G` and counts
move; `<Enter>` opens a table or edits a cell, `x` deletes a row, `<Tab>`
(or `1`, `2`, `3`) goes between the tables, the log and the console, and
`<Esc>` goes back, and out from the top. A client writes only through the
CRUD a domain exposes (`r.crud::<T>()`), as the signed-in person, and never
raw — alone too. harken exposes none, so here it is read-only, and says so.

The server's admin page is the same component compiled to wasm, served on
a listener of its own (`HARKEN_ADMIN_BIND`; `services.harken.admin.bind`,
`127.0.0.1:8788` by default) over the authority's store, its log with who
pushed each entry, and every connection's cursor. It writes as the
authority: through a table's CRUD where the domain exposes one, unless `R`
flips the switch, and raw otherwise — `ark.put_row`, `ark.delete_row`,
judged by the constraints and served to every peer like any entry. harken
exposes no CRUD, so every edit there is raw. On loopback it asks
nothing (reach it from elsewhere through an SSH tunnel); bound wider it asks
for a login holding the `admin` role (`services.harken.roles.admin`), signed
in through this server's own `/auth`. `nix build .#ark-admin` is the page;
the NixOS module serves it by default.

### Backing up, restoring, and asking a running server

    nix run .#arkc -- backup /var/lib/harken /srv/backup/harken   # while it runs
    nix run .#arkc -- verify-log /srv/backup/harken harken/domain/harken.ark
    nix run .#arkc -- restore /srv/backup/harken /var/lib/harken-new
    curl -H 'Accept: application/json' http://127.0.0.1:8787/healthz

`backup` copies a running server's data directory without stopping it:
the log's snapshot, then its journal as long as it was once the snapshot
had been read, so what the server appends meanwhile is not in the copy
and the copy is the log at one moment. Then the cursors, the sign-in
sessions, the kept listening sessions and the modules run. The scanner's `library/`
replica is not copied; it is a peer, and catches up from the log. Nor are
its prints or the covers it took out of files: restored, it reads every
file once and writes them again.
`restore` refuses a directory with anything in it and writes the log
**unnamed**, so the server started on it names the log afresh. Every
device is then sent the snapshot once and rebases its pending work onto
it. That is deliberate: a device that saw more of the old log than the
backup holds would otherwise be handed the new history on top of the old
one. `--same-log` keeps the name, for a backup of a stopped server
moved elsewhere. `verify-log DIR` prints the head, the horizon, the
log's id and the modules the server has run. Given the module, it also
replays the log and prints the state hash at the head, which is what
every device's `Verify` is compared with. `/healthz` asked for JSON
answers one object: head, horizon, log id, the module and every module
run, connections, rooms, and every session with its cursor, when it was
last heard and how many connections it has open. The process fleet's
`a_backup_taken_mid_stream_restores_to_its_moment` does all of it
against a server being pushed to (`docs/plan-db.md` D6).

## harken-peer, and the fleet

`harken-peer` is a peer with no screen, built beside the server (the
`harken-server` package ships both). It opens a replica in a directory,
signs in the way the desktop does in dev auth, dials the sync socket and
pumps when the socket wakes it, after a command that asks something of it,
and at the redial deadline — never on a clock, as the desktop does not
(`docs/plan-reactive.md`), so a frame nobody was woken for is a fleet that
does not converge; it reads commands as JSON lines on stdin and answers one line each,
ids and rows in the vectors' dialect (`ark::json`):

    harken-peer --dir /tmp/alice --server http://127.0.0.1:8787 --user alice
    {"cmd":"mutate","name":"create_playlist","args":{"name":"Road trip"}}
    {"ok":true,"id":{"$id":"…"}}
    {"cmd":"settle"}
    {"ok":true,"cursor":1,"pending":0}

`mutate`, `status`, `hash`, `wait`, `settle`, `query`, `rejections`,
`standing`, `disconnect`, `reconnect`, `sign_in`, `sign_out`, `join`,
`leave`, `persist`, `quit`; the source's first page says what each answers. Without `--user` it
opens signed out, authoring as nobody until `sign_in`. A sign-in waits on
the server ten seconds to connect and twenty to read (ark-auth's bounds),
or `--auth-patience-ms` for both; one that fails at start ends the
process. Its session revoked (`/auth/logout` with its token), a peer is
told at once on the socket it has — `denied`, "signed out: this login was
revoked" — keeps its store and pending, and stops dialling until
`sign_in`. A server restarted with less log than a peer has confirmed
hands it its state as a snapshot, and the peer's pending intents land on
that.

`server/tests/fleet.rs` is the process fleet (`docs/plan-fleet.md`): the
real server and real peers as child processes on loopback, each peer behind
a TCP proxy the test black-holes, cuts, pauses and replays through, and
every scenario ending on one invariant — every replica's confirmed state
hashes as the server's log does, and every accepted intent is in it once.

    cd rust && cargo test -p harken-server --test fleet -- --nocapture   # the timings
    FLEET_LONG=1 cargo test -p harken-server --test fleet the_seeded_fuzz -- --nocapture
    FLEET_SEED=12345 cargo test -p harken-server --test fleet the_seeded_fuzz
    cargo test -p harken-server --test fleet -- --ignored                # witnesses, if any (none today)

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
(`docs/plan-db.md` D1). Until one is, `checks.versions` runs the two
scenarios that need no older binary and the rest say they are skipped.

    nix flake check                       # checks.versions: every pinned revision, every scenario
    nix build .#harken-server-<name>      # one pinned revision's two binaries
    HARKEN_OLD_1_SERVER=$(nix build --print-out-paths .#harken-server-<name>)/bin/harken-server \
    HARKEN_OLD_1_PEER=$(nix build --print-out-paths .#harken-server-<name>)/bin/harken-peer \
      cargo test -p harken-server --test versions -- --nocapture

`HARKEN_OLD_<n>_SERVER` and `HARKEN_OLD_<n>_PEER` (and `HARKEN_OLD_<n>_NAME`
for what is printed), `n` counting from 1 in `nix/versions.nix`'s order, are
what the scenarios read; without them, the five that need an older binary
say they are skipped and pass, so `cargo test` on a laptop is unchanged.
The two that need none run the grown domain — `harken_server::grown`, a
nullable `playlist_item.note`, a table `tag`, and `create_playlist`'s body
moved — as "the next release": `HARKEN_MODULE=FILE` for the server and
`harken-peer --module FILE` for a peer, `FILE` what `grown::write` makes.

**To add a release to the matrix**: append `{ name; rev; }` to
`nix/versions.nix`, add the input `harken-<name>` to `flake.nix` with the
same revision (`git+https://github.com/k2on/apps?ref=main&rev=…`, following
this flake's `nixpkgs` and `rust-overlay`), run `nix flake lock`. The flake
refuses to evaluate if the locked revision is not the one `versions.nix`
names. `packages.harken-server-<name>` appears, `checks.versions` runs every
scenario once more against it, and `fleet-vm` runs its mixed machines on
the last entry.

What a peer says about versions, in `harken-peer`'s `status`:

- **`held`**: intents the server answered `held` — it has run no module
  that ships their function, so this peer is newer than it. They stay
  pending, are pushed again on every connection, and land once the server
  is upgraded; nothing is refused. A server from before holding says
  `reject` with `unknown function <hash>` for the same thing, and this
  build reads that as a hold too.
- **`behind`**: the server's module (said on every page) is not this
  peer's. Its facts are applied projected to this peer's schema — a column
  or table this peer lacks dropped, a nullable column it has and they lack
  `Null` — and no `Verify` is said, since two schemas' hashes cannot agree.
  A peer behind stays usable on everything its schema can see.

A server keeps the closures of every module it has started with
(`DATA/modules.cbor`; `/healthz` lists them, the current one marked), so an
older client's intent at a hash this server once ran is applied rather than
held; one from a module it never ran is held, which is the honest answer to
a client older than the server's first deploy. Its first start with a new
module re-hashes the log's snapshot under the new schema
(`ark_server::persist::rehome`).

None is pinned now (`nix/versions.nix` is empty, so the scenarios that
need an older binary are skipped); one is added when a release is shipped
that later builds must stay compatible with. A pinned revision must speak
this one's protocol: its peer reads this server's frames, and this server
reads theirs. Two
revisions' state hashes are not comparable (a schema, or the hash's own
definition, may differ between them), so the version scenarios compare
what each peer reads, not what it hashes.

## Not verified

Nothing in the container this was written in can run a phone, a window or
a speaker, so what is known of each beyond its tests is what was reported
back. The Android app is installed from the F-Droid repository and used on
a phone, and the browser build against a real server and a real Home
Assistant; that is where the crashes, the scrolling and the speaker's
position were reported, and each change since is tested on the JVM, in
headless Chromium and against a stand-in house before it is seen there.
The latest cast fixes (a speaker's position after a hand-off, kept within
600 ms) have not yet met a real speaker; `server/README.md` says how to
turn on the bridge's trace for that. The iOS app has never been built for
iOS: only its non-SwiftUI half type-checks, on Linux. The desktop window has
been driven only through its tests, and nothing has met a real OpenID
Connect provider from here.
The listening session has run between real sockets, peers and the bridge
against a stand-in house, and the reconcilers against a stand-in player;
no browser has played a track through it and no speaker has been handed
one, nor had its volume set or been joined to another: the volume and the
groups are held against a stand-in house's `volume_set`, `volume_mute`,
`join`, `unjoin` and template API, written from Home Assistant's
documentation. The Android app has not moved onto the live state yet
(`docs/plan-live.md` phase 3), so a phone has no listening session against
this server; and a speaker's plays are not recorded, since the bridge is
not a peer of the account.
