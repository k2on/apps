# Harken for iOS

SwiftUI over the Swift runtime (`ArkDB`), the client library around it
(`ArkDBClient`) and the generated domain (`HarkenGen.swift`). A whole-scope
replica of `library` and `playlists`, exact, offline first; with no server
it is the authority for its own log (docs/arkdb.md §3.10).

**Unverified on a device.** This app was written in a Linux container with
no iOS SDK and no SwiftUI. What *has* been compiled and run is everything
below the views: `Harken/Rows.swift` (the typed rows, the three mutations
through their generated functions, the join) was built on Linux against
`ArkDBClient` and `../domain/gen/swift/HarkenGen.swift` and driven through a
session alone and through two sessions on an in-process server — the same
paths `Model.swift` calls. The five SwiftUI files were written against iOS 17
APIs and read, not compiled. Expect to fix small things the first time
Xcode sees them.

## Generating the project

```
brew install xcodegen           # once
cd harken/ios
xcodegen generate
open Harken.xcodeproj
```

`project.yml` is the project: one target, iOS 17, and one Swift package
dependency on `../../swift` for `ArkDB` and `ArkDBClient`. The generated
domain is referenced in place as the `Generated` group —
`../domain/gen/swift/HarkenGen.swift`, written by

```
nix run .#arkc -- gen swift harken/domain/harken.ark harken/domain/gen/swift \
    --name Harken --only create_playlist,add_to_playlist,remove_from_playlist,library,playlists,playlist_items
```

— never copied. `Harken.xcodeproj` and `Harken/Info.plist` are outputs;
regenerate rather than edit.

## What it does

- **Library** — every track, by artist, album and title (`library`). A
  leading swipe or the `+` puts a track on the selected playlist
  (`add_to_playlist`); the toolbar menu picks which playlist that is.
- **Playlists** — every playlist by name (`playlists`); `+` opens a sheet
  to name a new one (`create_playlist`; a blank name is refused, and the
  refusal appears in the status bar). Tapping one opens it.
- **Playlist** — its items by position (`playlist_items`), each joined to
  its track on the phone — the two are in different scopes, so a query
  reads one store and the join is a map lookup; an item whose track has not
  arrived reads *(unavailable)*. Swipe to remove (`remove_from_playlist`).
- **Settings** — the server URL (`ws://127.0.0.1:8787/sync` is
  `harken-server`'s default), the user name (dev auth: the name is the
  login, the server calls every login `dev`), and *Work alone*. Applying
  reopens the session. Below it, what the session says: the link, the
  cursor per scope, pending intents, rejections, a denial, and the last
  `Agree`; a button asks the authority to verify; another takes the link
  down and up, which is how to watch the rebase.
- The status bar under the tabs is the same in one line, with the last
  refusal or error beside it.

## How the pieces meet

`Model` owns one `Session` (`ArkDBClient`), opened in
`Application Support/harken/<server|alone>/<user>/` — one directory per mode
and user, because a directory opened alone is refused against a server. The
session pumps its link on a 50 ms timer; the model subscribes to its change
notification and re-reads on the main actor.

**Mutations run the generated code, not the interpreter.** Each goes through
`HarkenDomain` as

```swift
session.mutate(name: "add_to_playlist", args: args) { db, ctx, autos in
    try HarkenGen.addToPlaylist(db, ctx, autos, args)
}
```

The session looks the function up by name in the module for its scope, its
hash and its autos, draws the autos (a random 16-byte id per `NewId`, the
clock in ms for `Now` — the only non-determinism, at origin), runs the body
as one transaction over the optimistic view (`TransactionStore`), records
the entry, pushes it if linked, and persists. The session is also given
`Generated(functions: HarkenGen.functions, apply: HarkenGen.apply, query:
HarkenGen.query)`, so entries *replayed* from the log whose function the
phone was generated with go through `HarkenGen.apply` too. `add_track` is
not among them (`--only`): the scanner's entries replay through the
interpreter over the module bytes `HarkenGen.moduleBytes` carries, and end
in the same state. Reads are `session.run { db in try HarkenGen.query("library", db, [:]) }`.

Working alone, the same session holds an `Authority` per scope and
`localCommit`s after every mutation, sequencing through the interpreter —
which means every entry the generated code authors is also applied by the
spec's evaluator before it is confirmed, a conformance check on every tap.
