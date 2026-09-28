# harken, on ArkDB

A deliberately small harken: enough of the domain to exercise every layer
of the new stack end to end — a domain written once in the vocabulary of
`spec/AUTHORING.md`, a Rust server that is the authority for two scopes, a
desktop peer in Rust, a phone in Swift and one in Kotlin, all exact
replicas of the same log — and nothing that would only prove harken. Covers, works and
recordings, the listening session, Home Assistant and sign-in stay in the
old harken until the stack has earned them.

```
domain/    the domain, in the vocabulary of spec/AUTHORING.md: schema.rs,
           library.rs, playlists.rs, module.rs. `module().emit()` is harken.ark;
           `module().procedures()` apply entries natively in any Rust peer;
           `arkc gen swift|kotlin` prints the same four files into gen/
server/    axum: hosts the `library` and `playlists` scopes of `module()`,
           applying every entry through harken's own procedures, natively;
           dev auth; a scanner that authors tracks from a directory; /media
desktop/   a terminal peer in Rust, authoring and replaying through the same
           procedures
web/       a browser peer: the Rust runtime as wasm, applying every entry through
           the interpreter over harken.ark, and a page over it; published to
           GitHub Pages
ios/       SwiftUI over the Swift runtime and the generated Swift
android/   Compose over the Kotlin runtime and the generated Kotlin
```

## The domain

Two scopes, because that is the decision the design says to make early
(docs/arkdb.md §3.2) and the one thing a single-scope demo could not show.

```
scope library {
  table track(id: Id(track), title: Text, artist: Text, album: Text?,
              duration_ms: Int, file: Text, added_ms: Int, user_id: Text)
        key (id)
}
scope playlists {
  table playlist(id: Id(playlist), name: Text, user_id: Text, created_ms: Int)
        key (id)
  table playlist_item(playlist_id: Id(playlist), track_id: Id(track),
                      pos: Int, added_ms: Int, user_id: Text)
        key (playlist_id, track_id) ref playlist_id -> playlist
}
```

`track_id` names a table in the other scope: an unchecked reference, which
is what a cross-scope id is under intents. A playlist item whose track has
not arrived is drawn as unavailable.

The text of it is `domain/src/schema.rs`: a scope is a struct of tables with
an `open()` naming them in order, a row a struct with its columns said once
in `Row::columns` (key, `.unique(..)`, `.refs::<Parent>()`), and a unique
index is what an insert's `.on(..)` may match on.

Two routers, `library` and `playlists`; on the second a guard and a
provide: `signed_in` refuses `sign in first` to an empty user, and `owned`
(built on it) hands the body the playlist the input names if it is the
caller's, or refuses `not your playlist`. Every input is a struct with its
checks — `trim`, `min(n).why("…")`, `max`, `at_least`, `exists` — run before
anything else, with the default messages of `spec/AUTHORING.md` §1.3.

Mutators, each one entry in one scope:

- `add_track` in `library` — trims the title and refuses a blank one,
  `duration_ms: at least 0`, a non-empty `file`; an insert `.on((file,))`,
  so a file already in the library is a no-op, which is what makes a rescan
  idempotent inside `apply` rather than in the scanner.
- `create_playlist` (`signed_in`) — trims; refuses an empty name or one over
  120 characters; an insert `.on((user_id, name))`, so a second device's
  default playlist is a no-op and not a duplicate.
- `add_to_playlist` (`owned`) — `pos = MAX(pos) + 1` over the playlist, which
  is what makes the rebase visible: add while offline and it lands after
  what arrived while you were away; an item already there is a no-op.
- `remove_from_playlist` (`owned`).

Queries: `library()` (tracks by artist, album, title, id), `playlists()`
(the caller's, by name; `signed_in`), `playlist_items(playlist_id)` (by pos;
`owned`). A screen joins items to tracks itself, because the two are in
different scopes and a query reads one scope; the join is a map lookup on
an id.

No live section yet: the listening session is the next thing to port and
the first real use of `live`.

**One text, run two ways.** Nothing is generated for Rust any more. The
server and the desktop depend on `harken-domain` and hold
`module().procedures()`: an entry whose hash is one of them is applied by
the domain's own Rust, natively, when it is authored, sequenced, replayed
on a rebase or received; any other entry replays through the interpreter
over the closure the module carries, or arrives as facts. The two ways are
held to each other on every procedure by `domain/tests/agreement.rs` (and
by the desktop on every authoring call in a debug build). The server can
still host an `.ark` file (`--module`): its functions run natively where
their hashes are harken's and interpreted otherwise.

## What each program does

- **server** hosts both scopes for everyone signed in under dev auth (a
  name is a login), scans `HARKEN_MEDIA/music` for audio files and authors
  `add_track` for each as the `library` account, serves `/media`, and speaks
  the protocol on `/sync` over a WebSocket.
- **desktop** opens or creates its database, subscribes to both scopes
  whole, shows the library and the playlists, and adds to and removes from
  a playlist with the keyboard, every mutation through harken's procedures. Offline works: what it does alone is
  pending until the server is back, then rebases.
- **ios** and **android** do the same on a phone, with the same generated
  domain code their platform's runtime executes, and nothing crossing any
  bridge.

## Building

Everything is `nix`, from the repository root:

    nix build .#harken-domain    # harken.ark, as `cargo run -p harken-domain` emits it,
                                 # and the Swift and Kotlin prints of it
    nix build .#harken-server
    nix build .#harken-desktop
    nix build .#harken-web       # the browser peer, as a static directory
    nix build .#arkdb-swift      # the Swift runtime and client, with their tests
    nix build .#arkdb-kotlin     # the Kotlin runtime and client, with their tests
    nix build .#harken-apk       # the Android app, debug-signed, from a recorded Maven graph
    nix flake check              # all of it, plus: fmt, clippy and every Rust test;
                                 # the vectors; harken.ark and gen/ against the tree

`harken/domain/harken.ark` and `harken/domain/gen/` are committed and
checked rather than regenerated on every build, because programs reference
them in place (`web` embeds `harken.ark`; `ios/project.yml` and
`android/app/build.gradle.kts` the printed Swift and Kotlin).
`domain/tests/agreement.rs` fails while the committed `harken.ark` differs
from what `module().emit()` writes; when `domain/src` changes:

    cd rust && cargo run -p harken-domain -- ../harken/domain/harken.ark

The Android app is a nix build like the rest (`android/README.md` says what
the derivation pins and how its Maven graph is re-recorded); the iOS app is
Xcode over `ios/project.yml`, since nothing but a Mac can build one. Neither
has been run on a device from here.

The browser peer is `harken/web`: the same `ark` crate compiled to
wasm32, bound by wasm-bindgen at the exact version `rust/Cargo.lock` names
(the flake reads it out of the lockfile), and a page bundled by esbuild —
`nix build .#harken-web` is `index.html`, `app.js`, `harken_web.js`,
`harken_web_bg.wasm` and `style.css`. It embeds `harken.ark` and applies
every entry through the interpreter, as the server does, and authors by
name through the module's closures; alone it is its own authority and
offers a few demo tracks, since nothing scans a library in a tab.
`web/README.md` has its JS API, how to run it locally, and the follow-up:
authoring through native procedures once the domain's new authoring API
lands.

Two workflows under `.github/workflows/` build from `main`:
`pages.yml` runs `nix build .#harken-web` and publishes it under
<https://k2on.github.io/apps/harken/> with a landing page above it;
`apk.yml` runs `nix build .#harken-apk` on an x86_64 runner, uploads
`harken-debug.apk` as the artifact `harken-debug-apk`, and on a `v*` tag
attaches it to a GitHub release. Both install nix with
`DeterminateSystems/nix-installer-action` and keep the store between runs
in GitHub's actions cache with `nix-community/cache-nix-action` (free for
a public repository; magic-nix-cache relied on a cache API GitHub has
retired). Pages must be set to deploy from GitHub Actions in the
repository's settings.

## Running it

    nix build .#harken-server .#harken-desktop -o result
    mkdir -p media/music/Bach/Goldberg && cp *.mp3 media/music/Bach/Goldberg/
    ./result/bin/harken-server --media ./media
    ./result-1/bin/harken-desktop --server ws://127.0.0.1:8787/sync --user alice
    ./result-1/bin/harken-desktop --server ws://127.0.0.1:8787/sync --user bob

The scanner authors a track per file as the `library` account; both
desktops replay them through the same `add_track` procedure. Make a
playlist on one and add to it on the other. Then stop the server, add on
both sides, and start it again: each peer's offline additions land after
what the other's did, in the order the authority sequenced them, and both
show one list. A desktop started with no `--server` is its own authority
and never needs one.
