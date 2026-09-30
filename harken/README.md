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
`standing`, `disconnect`, `reconnect`, `sign_in`, `sign_out`, `persist`,
`quit`; the source's first page says what each answers. Without `--user` it
opens signed out, authoring as nobody until `sign_in`.

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
crash).

## Not verified

No phone has run either frozen app, and the iOS screens have not met a
compiler. The phones speak spec v3 and the server now runs a v4 module;
they are not expected to sync with it, and nobody has tried.
No desktop window has been opened: the browser build has been drawn in
headless Chromium, and the desktop only driven through its tests. Nothing
here has met a real Home Assistant or a real OpenID Connect provider.
