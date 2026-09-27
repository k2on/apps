# harken, on ArkDB

A deliberately small harken: enough of the domain to exercise every layer
of the new stack end to end — a Rust-authored domain compiled to three
languages, a Rust server that is the authority for two scopes, a desktop
peer in Rust, a phone in Swift and one in Kotlin, all exact replicas of the
same log — and nothing that would only prove harken. Covers, works and
recordings, the listening session, Home Assistant and sign-in stay in the
old harken until the stack has earned them.

```
domain/    the domain, authored in Rust through ark-builder; emits harken.ark and,
           through arkc, generated Rust, Swift and Kotlin
server/    axum: hosts the `library` and `playlists` scopes, dev auth, a scanner
           that authors tracks from a directory, /media
desktop/   a terminal peer in Rust over the generated Rust
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

Everything is `nix`:

    nix run .#arkc -- gen rust  harken/domain/harken.ark harken/domain/gen/rust  --name Harken
    nix build .#harken-domain         # emits harken.ark and the three generated files
    nix build .#harken-server
    nix build .#harken-desktop
    nix develop .#swift               # then swift build in ios/
    nix develop .#kotlin              # then gradle in android/
