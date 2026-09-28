# Harken for iOS

SwiftUI over the Swift runtime (`ArkDB`), the client library around it
(`ArkDBClient`) and harken's domain written in Swift against the authoring
vocabulary (`ArkAuthoring`, `spec/AUTHORING.md`). One replica of the log,
exact, offline first; with no server it is the authority for its own log.

**Unverified on a device, and the views were never compiled.** This app was
written in a Linux container with no iOS SDK and no SwiftUI. What *has*
been compiled and run is everything below the views: `Harken/Rows.swift`
and the four domain files are built on Linux as the `HarkenPhone` target of
`../../swift` and driven by its test runner (`PhoneTests.swift`) through a
session alone, a session opened signed out and then signed in against an
in-process server running the whole of `harken.ark` (with a scanner peer
authoring `add_song`), a second and third device of the same person, and a
re-login that keeps pending work. `Model.swift` was type-checked on Linux
against a three-line stand-in for SwiftUI's `ObservableObject`,
`@Published` and `Color`; nothing more. The four SwiftUI view files were
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
domain as `arkc gen swift` prints it for the phone (`clientFunctions` in
`flake.nix`: `create_playlist, add_to_playlist, remove_from_playlist,
library, playlists, playlists_of, playlist`; what the scanner alone
authors is not in it) — never copied. `Harken.xcodeproj` and
`Harken/Info.plist` are outputs; regenerate rather than edit.

The domain's tables are a struct called `Harken` (`Schema.swift`), so the
app's own namespace for the bridge is `Phone`.

## What it does

- **It works with nobody signed in.** The first launch opens the session
  signed out (`Session.openSignedOut`): everything is authored as nobody,
  applied, written to disk, and nothing is sent anywhere. Signing in
  (Settings → Account; dev auth, the name is the login) makes all of it
  that person's (`Session.signIn`) and syncs it; the app then opens as them
  on every launch. Signing out reopens signed out over the same replica —
  one directory per server, not per person, because the log is the
  server's — and whatever that person left pending stays theirs and goes
  when they sign in again: the server accepts an entry authored under an
  older login of the same person.
- **Library** — everything in the library, in the order it was added
  (`library`, read against the selected playlist, so each row knows
  whether it is on it). The row's button puts a track on that playlist or
  takes it off; a long press lists every playlist, ticked by
  `playlists_of`, each a toggle. The toolbar picks the selected playlist.
- **Playlists** — the person's playlists in the order they were made
  (`playlists`); `+` opens a sheet to name a new one (`create_playlist`).
  Under the name field the sheet says what is wrong with it as it is typed
  — the procedure's own input checks, run by the form validator — and
  Create is off while there is something to say. A name the person already
  has is not a problem: the log keeps the new playlist and calls it
  "Name (1)".
- **Playlist** — its contents in playlist order (`playlist`, which answers
  with the library's own rows). Swipe to remove (`remove_from_playlist`).
- **Favorites.** The app makes a default "Favorites" only for somebody with
  no playlist at all, and only once it knows: alone at once; signed in
  after the link has been up two seconds, so the log has arrived; signed
  out, on the first add with no playlist to add to — not on opening, so
  that signing in later on a second device does not hand the person a
  "Favorites (1)" they never asked for.
- **Where every change stands.** Each change this phone makes is kept with
  its entry id, and `Session.standing` says where it is: under a playlist
  or an item, *not synced yet* while pending, nothing once confirmed, and
  *not saved:* followed by the server's own sentence when it was rejected.
  Settings lists every change with its standing, and the status bar shows
  the newest rejection's reason.
- **Settings** — the account, the server URL (`ws://127.0.0.1:8787/sync`
  is `harken-server`'s default) and *Work alone*; what the session says;
  the changes; the module's hash and the procedures the phone runs
  natively.

## How the pieces meet

`Model` owns one `Session` (`ArkDBClient`), opened in
`Application Support/harken/<server-…|alone>/` with

```swift
Session.openSignedOut(directory: dir, module: Phone.moduleBytes, procedures: Phone.procedures, server: url)
Session.open(directory: dir, module: Phone.moduleBytes, procedures: Phone.procedures, user: user, server: url)
```

where `Phone.moduleBytes` is `module().emit()` and `Phone.procedures` is
`module().procedures()` of the domain compiled into the app.

**Every procedure runs as the Swift it is written in.** A mutation is an
input of the domain's own type, `session.author(name: "add_to_playlist",
args: AddToPlaylist(playlistId: …, mediaId: …).args)`: the session looks the
function up by name for its hash and autos, draws the autos (a random
16-byte id per `NewId`, the clock in ms for `Now` — the only
non-determinism, at origin), and the replica applies it through the native
procedure — the closure in `Playlists.swift`, run under `Native` — as one
transaction over the optimistic view. Entries replayed from the log whose
function the phone holds run the same way; `add_song`, which the phone
does not have, arrives from the server's scanner and is applied by what
the server sends for it. Reads are `session.query(name:args:)`, natively
too, and the rows are read through the domain's own types
(`LibraryEntry(repr: .v(value))`, `Playlist(repr: .v(value))`).

Working alone, the same session holds an `Authority` and commits after
every mutation, sequencing through the interpreter — so every entry the
native code authors is also applied by the spec's evaluator before it is
confirmed, a conformance check on every tap.
