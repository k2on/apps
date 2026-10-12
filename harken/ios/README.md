# harken for iOS

SwiftUI over the Swift client at spec v4 (`swift/`: `ArkDB` and
`ArkDBClient`, `swift/README.md`). The phone is a **client**: it loads
`harken.ark` — the module the domain crate writes, the one the server, the
desktop, the browser and the Android app run — from its own bundle,
verifies it, and interprets every function of it. Nothing of the domain is
written in Swift, and nothing is generated into the app: what the screens
call is a mutation or a query of the module, by name
(`docs/plan-phones.md`, round 7). It is the Android app (`../android`)
again, screen for screen.

**Never built for iOS, never run.** This app was written in a Linux
container with no iOS SDK, no Xcode and no SwiftUI. What *has* been
compiled is the part of it that is not SwiftUI: `Harken/Rows.swift` (the
rows and writes), `Harken/Bridge.swift` (the peer, its queue and its
maintained views) and `Harken/Model.swift` (the model the screens observe)
type-check on Linux against the `ArkDB` and `ArkDBClient` modules and a
three-line stand-in for SwiftUI's `ObservableObject`, `@Published` and
`Color` (`tools/typecheck.sh`, which `checks.swift` runs). The four view
files and `HarkenApp.swift` were written against iOS 17's SwiftUI and read,
not compiled. Under them, on Linux: the runtime and client pass every
client vector and every trace of the corpus, directly and through their
storage records, replay a Rust peer's records byte for byte, and
`harken-peer-swift` — a headless peer on the same client — runs in the
process fleet beside Rust peers (`checks.swift`, `checks.fleet-swift`). On
iOS the socket is `URLSessionWebSocketTask` (`SessionSocket`), which has
been type-checked and never opened a connection. Expect the first build in
Xcode to want small fixes.

## Generating the project

```
brew install xcodegen           # once
cd harken/ios
xcodegen generate
open Harken.xcodeproj
```

`project.yml` is the project: one target, iOS 17, one Swift package
dependency on `../../swift` for `ArkDB` and `ArkDBClient`, and
`../domain/harken.ark` copied into the bundle as a resource — referenced in
place, never copied into this tree. `Harken.xcodeproj` and
`Harken/Info.plist` are outputs; regenerate rather than edit.

## How the pieces meet

- **`Bridge`** owns the one `Peer` (`ArkDBClient`) on a serial queue of its
  own, `ark-peer`: the pump every fifty milliseconds, every mutation, every
  read a screen asks for. `library`, `playlists` and the open `playlist`
  are maintained views (`ClientView`): each is handed every `Changes` the
  peer reports, and the patches it answers with are spliced into the list
  the screen was given — read again only on a `reset` (a rebase that
  replaced the store, a sign-in, another selection). What the screens see
  goes to the main queue as a `Snapshot` when something in it moved.
- **`Model`** is SwiftUI's half: the prefs in `UserDefaults` (the server,
  work alone, the login whole so a restart reuses it), the snapshot, the
  last notice, and the sign-in.
- **`Phone`** (Rows.swift) reads rows of harken's queries into `Track` and
  `Playlist` and makes each write one of the module's mutations by name,
  returning the entry's id so that its standing can be asked later.

The replica is `Application Support/harken/ark/<server-…|alone>/replica`,
in the records `rust/ark-client` writes — one directory per server (or
alone), not per person, because the log is the server's.

## What it does

- **It works with nobody signed in.** The first launch opens the peer
  signed out: everything is authored as nobody, applied, written down, and
  nothing is dialled. Signing in (Settings; dev auth, a name) makes all of
  it that person's and syncs it; the login is kept and the app opens as
  them on every launch. Signing out keeps what that person left pending for
  when they sign in again. A server that signs people in through a
  provider's page is not supported: the sign-in says so.
- **Work alone.** No server: the peer is its own authority over a local
  history in a replica of its own. Songs only arrive from a server's
  scanner, so the library stays empty; playlists work.
- **Library** — everything in the library, read against the selected
  playlist so each row knows whether it is on it; the row's button puts a
  track on that playlist or takes it off, a long press lists every
  playlist ticked by `playlists_of`. With no playlist at all, adding makes
  "Favorites" first.
- **Playlists** — the person's playlists in the order they were made; `+`
  names a new one (`create_playlist`), the name's problems said as it is
  typed by the mutation's own input checks (`Peer.check`). A name the person
  already has is kept and numbered by the log.
- **Playlist** — its contents in playlist order; swipe to remove
  (`remove_from_playlist`).
- **Where every change stands.** Each change this phone makes is kept with
  its entry id, and `Peer.standing` says where it is: *not synced yet* while
  pending, nothing once confirmed, *not saved:* and the server's own
  sentence when it was rejected.
- **Settings** — the account, the server's address (`http://host:port`;
  the socket, `ws://host:port/sync`, is derived from it), work alone, and
  the status: the link in a word, linked, the cursor, not synced, held,
  diverged, joining, connections, behind, the last close, a denial; a
  verify against the authority; and every change with its standing.
