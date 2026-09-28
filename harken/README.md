# harken, on ArkDB

A deliberately small harken: enough of the domain to exercise every layer
of the new stack end to end — a Rust-authored domain compiled to three
languages, a Rust server that is the authority for two scopes, a desktop
peer in Rust, a phone in Swift and one in Kotlin, all exact replicas of the
same log — and nothing that would only prove harken. Covers, works and
recordings, the listening session, Home Assistant and sign-in stay in the
old harken until the stack has earned them.

```
domain/    the domain, authored in Rust through ark-builder, one file per concern
           (schema, library, playlists, queries); emits harken.ark and, through
           arkc, generated Rust, Swift and Kotlin, each client's with only
           what it calls
server/    axum: hosts the `library` and `playlists` scopes from harken.ark
           itself, through the interpreter; dev auth; a scanner that authors
           tracks from a directory; /media
desktop/   a terminal peer in Rust over the generated Rust
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

Mutators, each one entry in one scope:

- `add_track(id: NewId(track), added_ms: Now, title, artist, album?,
  duration_ms, file)` in `library` — refuses a blank title; a no-op if the
  id exists or a non-empty `file` is already in the library, which is what
  makes a rescan idempotent inside `apply` rather than in the scanner.
- `create_playlist(id: NewId(playlist), created_ms: Now, name)` in
  `playlists` — trims; refuses an empty name; a no-op if this person already
  has a playlist of that name, so a second device's default playlist is not
  a duplicate.
- `add_to_playlist(added_ms: Now, playlist_id, track_id)` — a no-op if the
  playlist is missing or the item is there; `pos = MAX(pos) + 1` over the
  playlist, which is what makes the rebase visible: add while offline and
  it lands after what arrived while you were away.
- `remove_from_playlist(playlist_id, track_id)`.

Queries: `library()` (tracks by artist, album, title, id), `playlists()`
(by name), `playlist_items(playlist_id)` (by pos). A screen joins items to
tracks itself, because the two are in different scopes and a query reads
one store; the join is a map lookup on an id.

No live section yet: the listening session is the next thing to port and
the first real use of `live`.

**Clients are generated with `--only`.** `add_track` is authored by the
scanner and by nothing on a phone or the desktop, so their generated code
does not contain it: `arkc gen swift harken.ark … --only
create_playlist,add_to_playlist,remove_from_playlist,library,playlists,playlist_items`
(the list is `CLIENT_FUNCTIONS` in `domain/src/main.rs`, and the flake's
`clientFunctions`). Tracks still arrive, because a replica that does not
hold an entry's function applies the facts the server kept beside it — or,
holding the whole module's bytes as every generated file does, replays the
entry through the interpreter — and ends in the same state (docs/arkdb.md
§3.8). The server is generated with nothing: it loads `harken.ark` and
applies every intent through the module's own closures, so a domain change
reaches it by rebuilding the module and not the binary.

## What each program does

- **server** hosts both scopes for everyone signed in under dev auth (a
  name is a login), scans `HARKEN_MEDIA/music` for audio files and authors
  `add_track` for each as the `library` account, serves `/media`, and speaks
  the protocol on `/sync` over a WebSocket.
- **desktop** opens or creates its database, subscribes to both scopes
  whole, shows the library and the playlists, and adds to and removes from
  a playlist with the keyboard. Offline works: what it does alone is
  pending until the server is back, then rebases.
- **ios** and **android** do the same on a phone, with the same generated
  domain code their platform's runtime executes, and nothing crossing any
  bridge.

## Building

Everything is `nix`, from the repository root:

    nix build .#harken-domain    # harken.ark and the three generated files, as
                                 # the domain program and arkc write them today
    nix build .#harken-server
    nix build .#harken-desktop
    nix build .#harken-web       # the browser peer, as a static directory
    nix build .#arkdb-swift      # the Swift runtime and client, with their tests
    nix build .#arkdb-kotlin     # the Kotlin runtime and client, with their tests
    nix build .#harken-apk       # the Android app, debug-signed, from a recorded Maven graph
    nix flake check              # all of it, plus: fmt, clippy and every Rust test;
                                 # the vectors; harken.ark and gen/ against the tree

`harken/domain/harken.ark` and `harken/domain/gen/` are committed and
checked rather than regenerated on every build, because three programs
reference the generated files in place (`desktop/src/domain.rs`,
`ios/project.yml`, `android/app/build.gradle.kts`). When `domain/src`
changes, copy the check's answer into the tree:

    nix build .#harken-domain && cp -r result/harken.ark result/gen harken/domain/

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
    ./result/bin/harken-server --module harken/domain/harken.ark --media ./media
    ./result-1/bin/harken-desktop --server ws://127.0.0.1:8787/sync --user alice
    ./result-1/bin/harken-desktop --server ws://127.0.0.1:8787/sync --user bob

The scanner authors a track per file as the `library` account; both
desktops receive them as facts, since neither holds `add_track`. Make a
playlist on one and add to it on the other. Then stop the server, add on
both sides, and start it again: each peer's offline additions land after
what the other's did, in the order the authority sequenced them, and both
show one list. A desktop started with no `--server` is its own authority
and never needs one.
