# harken for iOS

SwiftUI over harken's library (`../ffi`, `docs/plan-v5.md` §1.16): the Rust
kernel with harken's domain linked in, as `HarkenFFI.xcframework` — the
same code the server, the desktop, the browser and the Android app run. The
kernel keeps the replica, dials the server and keeps the views; the app's
code is its screens and its model. It calls the domain by name through the
Swift `ark-dev gen swift` writes from the registered domain — a struct per
record and row, an enum per sum, a stub per function (`Harken/Gen/
Harken.swift`, never edited: `ARK_WRITE_GEN=1 cargo test -p harken-ffi
--test gen` writes it, and the same test fails when it is not what the
domain generates) — over UniFFI's Swift and the bridge's typed half
(`rust/ark-ffi/swift/Typed.swift`).

**Never built for iOS, never run.** The library for iOS is
`nix build .#harken-ios-xcframework` — `harken/ffi` compiled for
`aarch64-apple-ios` and `aarch64-apple-ios-sim`, bound into an xcframework
by `xcodebuild`, with the Swift UniFFI writes for it — a derivation that
evaluates everywhere and builds only on a macOS builder with Xcode
(`meta.platforms`; it asks for the builder's Xcode outside the store). It
has not been built: this was written in a Linux container with no Mac, no
Xcode and no SwiftUI. What *was* checked here: the Rust targets install
(`rustup target add aarch64-apple-ios aarch64-apple-ios-sim`), UniFFI
writes the Swift for the library built for Linux (`checks.ffi`), and the
generated `Harken.swift` is held to the domain; and `Rows.swift`,
`Bridge.swift`, `Model.swift` and the generated Swift **type-check** on
Linux against UniFFI's Swift and the library's headers, with a stand-in for
SwiftUI (`tools/typecheck.sh`, `checks.harken-ios-typecheck`: nixpkgs'
Swift 5.8, which predates the `nonisolated(unsafe)` UniFFI writes twice for
Swift 6, so the check's copy has it taken out — Xcode's Swift reads it as
written). That check found two things a Mac would have: the app's own
`Track` beside the domain's (the rows are `LibraryEntry` now), and
`URLSession` living in `FoundationNetworking` off Apple's platforms. The
views were written against iOS 17's SwiftUI and read, not compiled. Expect
the first build in Xcode to want small fixes.

## Developer loop

Every generated file the app compiles of the domain — `Harken/Gen/Harken.swift`
and UniFFI's own Swift for its twins, `Harken/Gen/harken_domain.swift`
(which the xcframework's `swift/` leaves out) — is checked in and written
from the domain's source by one command, with no compiler: `ark-dev gen`
(built once, `cargo build --release -p ark-dev`; `--check` writes nothing
and fails on a file that differs). It takes about 40 ms, so it runs on save
and Xcode or sourcekit-lsp sees a changed domain type at once:

    autocmd BufWritePost harken/domain/src/*.rs silent !ark-dev gen
    watchexec -w harken/domain/src -e rs -- ark-dev gen    # or: ls harken/domain/src/*.rs | entr ark-dev gen

## Generating the project

```
nix build .#harken-ios-xcframework -o ../ffi/ios   # on a Mac
brew install xcodegen                               # once
cd harken/ios
xcodegen generate
open Harken.xcodeproj
```

`project.yml` is the project: one target, iOS 17, over
`../ffi/ios/HarkenFFI.xcframework` and the Swift beside it
(`../ffi/ios/swift`). `Harken.xcodeproj` and `Harken/Info.plist` are
outputs; regenerate rather than edit.

## How the pieces meet

- **`Bridge`** holds the one peer, `harkenOpen` over a replica directory,
  and a serial queue of its own for the lists the screens draw. The peer
  drives itself (its link, its redial and its views are the kernel's) and
  tells the bridge when anything moved (`Listener`); the bridge then asks
  `library`, `playlists` and the open `playlist` — the kernel's maintained
  views, typed (`View<T>`) — what they did, and splices their patches into
  the lists the screens were given, reading one again only on a reset or
  when the selection moves. What the screens see goes to the main queue as
  a `Snapshot` when something in it moved.
- **`Model`** is SwiftUI's half: the prefs in `UserDefaults` (the server,
  work alone, the login whole so a restart reuses it), the snapshot, the
  last notice, and the sign-in (dev auth: the code read off the server's
  redirect, exchanged through the kernel).
- **`Phone`** (Rows.swift) reads harken's views as the domain's own rows
  (`LibraryEntry`, `Playlist`: UniFFI's records, read typed with no JSON;
  the generated Swift gives them only their JSON, as an extension) and makes each write one of the domain's
  functions through the generated stubs, returning the entry's id so that
  its standing can be asked later.

The replica is `Application Support/harken/<server-…|alone>/replica`, the
kernel's own files — one directory per server (or alone), not per person,
because the log is the server's.

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
  typed by the function's own input checks (`check`). A name the person
  already has is kept and numbered by the log.
- **Playlist** — its contents in playlist order; swipe to remove
  (`remove_from_playlist`).
- **Where every change stands.** Each change this phone makes is kept with
  its entry id, and the peer's `standing` says where it is: *not synced yet* while
  pending, nothing once confirmed, *not saved:* and the server's own
  sentence when it was rejected.
- **Settings** — the account, the server's address (`http://host:port`;
  the socket, `ws://host:port/sync`, is derived from it), work alone, and
  the status: the link in a word, linked, the cursor, not synced, joining,
  connections, the disk's last refusal, a denial; and every change with
  its standing.
