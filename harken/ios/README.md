# Harken for iOS

SwiftUI over the Swift runtime (`ArkDB`), the client library around it
(`ArkDBClient`) and harken's domain written in Swift against the authoring
vocabulary (`ArkAuthoring`, `spec/AUTHORING.md`). A whole-scope replica of
`library` and `playlists`, exact, offline first; with no server it is the
authority for its own log (docs/arkdb.md §3.10).

**Unverified on a device.** This app was written in a Linux container with
no iOS SDK and no SwiftUI. What *has* been compiled and run is everything
below the views: `Harken/Rows.swift` and the four domain files are built on
Linux as the `HarkenPhone` target of `../../swift` and driven by its test
runner through a session alone and through two sessions on an in-process
server — the same paths `Model.swift` calls. The five SwiftUI files were
written against iOS 17 APIs and read, not compiled. Expect to fix small
things the first time Xcode sees them.

## Generating the project

```
brew install xcodegen           # once
cd harken/ios
xcodegen generate
open Harken.xcodeproj
```

`project.yml` is the project: one target, iOS 17, and one Swift package
dependency on `../../swift` for `ArkDB`, `ArkDBClient` and `ArkAuthoring`.
The domain is referenced in place as the `Domain` group —
`../domain/gen/swift/{Schema,Library,Playlists,Module}.swift`, harken's
domain as `arkc gen swift` prints it for the phone
(`--only create_playlist,add_to_playlist,remove_from_playlist,library,playlists,playlist_items`,
so `add_track` and its input are not in it) — never copied.
`Harken.xcodeproj` and `Harken/Info.plist` are outputs; regenerate rather
than edit.

## What it does

- **Library** — every track, by artist, album and title (`library`). A
  leading swipe or the `+` puts a track on the selected playlist
  (`add_to_playlist`); the toolbar menu picks which playlist that is.
- **Playlists** — every playlist by name (`playlists`); `+` opens a sheet
  to name a new one (`create_playlist`). Under the name field the sheet
  says what is wrong with it as it is typed — the procedure's own input
  checks (trim, at least one character, at most 120) run by the form
  validator, the same words the mutation would refuse with — and Create is
  off while there is something to say. Tapping a playlist opens it.
- **Playlist** — its items by position (`playlist_items`), each joined to
  its track on the phone — the two are in different scopes, so a query
  reads one store and the join is a map lookup; an item whose track has not
  arrived reads *(unavailable)*. Swipe to remove (`remove_from_playlist`).
- **Settings** — the server URL (`ws://127.0.0.1:8787/sync` is
  `harken-server`'s default), the user name (dev auth: the name is the
  login, the server calls every login `dev`), and *Work alone*. Applying
  reopens the session. Below it, what the session says, the module's hash
  and the procedures the phone runs natively.

## How the pieces meet

`Model` owns one `Session` (`ArkDBClient`), opened in
`Application Support/harken/<server|alone>/<user>/` with

```swift
Session.open(directory: dir, module: Harken.moduleBytes, procedures: Harken.procedures, user: user, server: url)
```

where `Harken.moduleBytes` is `module().emit()` and `Harken.procedures` is
`module().procedures()` of the domain compiled into the app.

**Every procedure runs as the Swift it is written in.** A mutation is an
input of the domain's own type, `session.mutate(name: "add_to_playlist",
args: AddToPlaylist(playlistId: …, trackId: …).args)`: the session looks the
function up by name for its scope, hash and autos, draws the autos (a
random 16-byte id per `NewId`, the clock in ms for `Now` — the only
non-determinism, at origin), and the replica applies it through the native
procedure — the closure in `Playlists.swift`, run under `Native` — as one
transaction over the optimistic view. Entries replayed from the log whose
function the phone holds run the same way; `add_track`, which the phone
does not have, arrives from the server's scanner and is applied by its
facts. Reads are `session.query(name:args:)`, natively too, and the rows
are read through the domain's own row types (`Track(repr: .v(value))`).

Working alone, the same session holds an `Authority` per scope and
`localCommit`s after every mutation, sequencing through the interpreter —
so every entry the native code authors is also applied by the spec's
evaluator before it is confirmed, a conformance check on every tap.
