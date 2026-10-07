# harken, on ArkDB

A self-hosted music system: a library a server scans from a directory,
playlists, one listening session per account across every device, and the
speakers in the house as devices in it. All of it runs on ArkDB: one log,
one domain written once in Rust, applied natively by the server, the
desktop and the browser, and every query a plan a client can keep up to
date by the rows a change touched.

```
domain/    the domain, in the vocabulary of spec/AUTHORING.md (src/); harken.ark,
           what it emits; listening.rs, the listening session's frames (never
           the log)
server/    ark-server with harken's own parts: the scanner, the listening desk,
           the Home Assistant bridge, sign-in, /media, the web build; the NixOS
           module (server/README.md)
iced/      the desktop and browser client, on arkui and ark-client; the `demo`
           feature is the seeded library GitHub Pages publishes
```

Frozen at spec v3, with the Swift and Kotlin runtimes they are built on
(`swift/FROZEN.md`, `kotlin/FROZEN.md`), and not following the domain
since:

```
domain/gen/   {swift,kotlin}: arkc's print of what the phones called, from the
              v3 harken.ark; there is no printer now
ios/          SwiftUI over ArkDBClient and the printed Swift (ios/README.md)
android/      Compose over ark-client and the printed Kotlin (android/README.md);
              still assembled by nix build .#harken-apk
```

## The domain

Ten tables in one log: `media` and `song` (a track and what is true of it
as a song), `album` and `person`, `work`, `movement`, `recording` and
`credit` (a work is not a recording, and neither is an album), and
`playlist` and `playlist_item`. Every reference is checked. `library` is
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

## Building

Everything is `nix`, from the repository root:

    nix build .#harken-server      # the server
    nix run .#harken-serve         # …in dev auth: anyone is whoever they say
    nix build .#harken-iced        # the desktop window
    nix build .#harken-web         # the browser demo, as a static directory (Pages)
    nix build .#harken-web-server  # the browser client harken-server serves
    nix build .#harken-domain      # harken.ark, as the source emits it
    nix build .#harken-apk         # the frozen Android app, debug-signed
    nix flake check                # fmt, clippy and every Rust test; harken.ark
                                   # is what the domain emits and it verifies;
                                   # the vectors; the NixOS module

`nixosModules.default` is `services.harken` (`server/README.md` has its
options). The frozen iOS app is Xcode over `ios/project.yml`, since only a
Mac can build one.

Two workflows build from `main`: `pages.yml` publishes `harken-web` at
<https://k2on.github.io/apps/harken/>, and `apk.yml` builds `harken-apk`
— the frozen v3 app — and attaches it to a release on a `v*` tag.

## Running it

    mkdir -p media/music && cp -r ~/Music/SomeAlbum media/music/
    HARKEN_MEDIA=$PWD/media nix run .#harken-serve   # 127.0.0.1:8787, dev auth
    nix run .#harken-iced -- --server http://127.0.0.1:8787 --user alice
    nix run .#harken-iced -- --server http://127.0.0.1:8787 --user bob

The scanner adds whatever audio is under `music/` in `HARKEN_MEDIA`, and
anything dropped there later. Make a playlist in one window and add to it in the other; stop the
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
sessions, the rooms and the modules run. The scanner's `library/`
replica is not copied; it is a peer, and catches up from the log.
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
pumps; it reads commands as JSON lines on stdin and answers one line each,
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

    nix flake check                       # checks.versions: every pinned revision, every scenario
    nix build .#harken-server-v4-journal  # one pinned revision's two binaries
    HARKEN_OLD_1_SERVER=$(nix build --print-out-paths .#harken-server-v4-journal)/bin/harken-server \
    HARKEN_OLD_1_PEER=$(nix build --print-out-paths .#harken-server-v4-journal)/bin/harken-peer \
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

The pinned revisions are still frozen at spec version 4 like this one: a
pinned peer reads this server's frames, and this server reads theirs. Two
revisions' state hashes are not comparable (a schema, or the hash's own
definition, may differ between them), so the version scenarios compare
what each peer reads, not what it hashes.

## Not verified

No phone has run either frozen app, and the iOS screens have not met a
compiler. The phones speak spec v3 and the server now runs a v4 module;
they are not expected to sync with it, and nobody has tried.
No desktop window has been opened: the browser build has been drawn in
headless Chromium, and the desktop only driven through its tests. Nothing
here has met a real Home Assistant or a real OpenID Connect provider.
