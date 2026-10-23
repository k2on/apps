# harken for iOS

SwiftUI over harken's library (`../ffi`, `docs/plan-v5.md` §1.16): the Rust
kernel with harken's domain linked in, as `HarkenFFI.xcframework` — the
same code the server, the desktop, the browser and the Android app run. The
kernel keeps the replica, dials the server, keeps the views and, through
harken's one reconciler (`harken/player`, the FFI's `Player`), decides what
this phone's player does; the app's code is its screens, its model, and the
player itself. It calls the domain by name through the Swift `arkdb gen
swift` writes (`Harken/Gen/Harken.swift`, never edited: `ARK_WRITE_GEN=1
cargo test -p harken-ffi --test gen` writes it) over UniFFI's Swift and the
bridge's typed half (`rust/ark-ffi/swift/Typed.swift`).

The Android app is the spec: `PARITY.md` lists every screen, sheet, control,
gesture, animation and background behaviour it has, the iOS counterpart and
its API, the motion mapping, every Liquid Glass and iOS 26 API used, and
what is not built and why.

**Never built in Xcode, never run on a device.** It was written in a Linux
container with no Mac. What *was* checked: the non-UI half type-checks and
its tests run (below); every file parses.

## Building

```
nix build .#harken-ios-xcframework -o result-ios     # on a Mac with Xcode
rm -rf ../ffi/ios && cp -RL result-ios ../ffi/ios     # HarkenFFI.xcframework, swift/
brew install xcodegen                                  # once
cd harken/ios
xcodegen generate
xcodebuild archive -scheme Harken -configuration Release -destination 'generic/platform=iOS'
```

`project.yml` is the project: the app (iOS 26.0, Swift 5 language mode —
why not 6 is said at its top — `UIBackgroundModes: audio`, the `harken`
URL scheme, `ITSAppUsesNonExemptEncryption: false`, the icon in
`Harken/Assets.xcassets`, version `1.0 (1)` which CI overrides, automatic
signing, bundle id `dev.arkdb.harken` which CI overrides) over
`../ffi/ios/HarkenFFI.xcframework` and `../ffi/ios/swift`, and the
`HarkenTests` unit tests, in the one scheme `Harken`. `Harken.xcodeproj`
and `Harken/Info.plist` are outputs; regenerate rather than edit. The icon
is `tools/icon.py`'s (standard-library Python: a gold waveform on a dark
ground, 1024 × 1024, opaque).

## How the pieces meet

Foundation only — type-checked and tested on Linux:

- **`Bridge`** — the session for one server and login: the peer opened over
  the replica (`Application Support/harken/server-…/replica`, one directory
  per server), dialled, and drained on one queue whenever it says anything
  moved (`Listener`): the library's views spliced (songs, playlists, albums
  first, then artists, details, the last play; a page's while it is on its
  stack), the reconciler stepped, a rejection or a denial said once, and a
  `Snapshot` sent to the main queue. Every write is a domain function.
- **`Model`** — the main actor's: the onboarding's machine run, the session
  it opens, what plays (`PlayerState`, derived from the reconciler's one
  view), and every button: a press of the account's session while this
  phone is part of it, or, alone, done to the player itself.
- **`Onboarding`** — the server step (`Address`, `Health`, `Probe`) and the
  sign-in, with no button: it goes by itself, once a launch.
- **`Session`**, **`Playing`**, **`Rows`** — the session read, every press as
  harken_player's JSON, the queue edits, the picker, volumes and the
  throttle; what plays and Home's last played; the rows, pages, places.
- **`SoundCore`** — the reconciler's `PhoneSound` over an `Engine`.

Apple's — written, not compiled here: **`AVEngine`** (AVPlayer, the
audio session, interruptions, route changes), **`NowPlaying`** (lock
screen, Control Center, headset), **`Covers`** (disk and memory caches,
ImageIO), and the screens: `Shell` (tabs, accessory, sheets, zoom),
`HomeView`, `SearchView`, `LibraryView`, `Pages`, `PlayerViews`, `Sheets`,
`SettingsView`, `OnboardingViews`, `Components`, `Theme`, `HarkenApp`.

## Checks

`checks.harken-ios-typecheck` (`tools/typecheck.sh`): every file under
`Harken/` whose imports are Foundation and Combine only, with the
generated types, against UniFFI's Swift for the library and its headers
and a stand-in for Combine's `ObservableObject`/`@Published` — nixpkgs'
Swift 5.8, which predates `nonisolated(unsafe)`, so the check's copy of
UniFFI's Swift has it taken out. Then HarkenTests type-check with them
against a stand-in for XCTest's assertions; and, handed the library built
for this machine, they are linked and run:

```
cargo build -p harken-ffi --lib          # rust/
HARKEN_FFI_LIB=$PWD/rust/target/debug/libharken_ffi.so \
  bash harken/ios/tools/typecheck.sh <uniffi swift bindings>
```

On a Mac the same tests are Xcode's: `xcodebuild test -scheme Harken
-destination 'platform=iOS Simulator,name=iPhone 17'`.

## First build

The first compile is the user's CI on a macOS runner (Xcode 26.3). What is
most likely to want a fix, most likely first:

1. **`\.tabViewBottomAccessoryPlacement`** (PlayerViews.swift `MiniPlayer`):
   the environment key's name and its type (`TabViewBottomAccessoryPlacement?`,
   `.inline`/`.expanded`). If it does not compile, delete the property and
   `compact`: the bar then draws its full face always.
2. **The zoom on a `fullScreenCover`** (Shell.swift):
   `navigationTransition(.zoom(sourceID:in:))` on the cover's content with
   the accessory as `matchedTransitionSource`. If the accessory cannot be a
   source, drop `matchedTransitionSource` there: the player then rises as a
   plain full-screen cover.
3. **`web.authenticate(using:callbackURLScheme:)`** (HarkenApp.swift): if the
   SDK has only the iOS 17.4 form, it is
   `authenticate(using: url, callback: .customScheme("harken"), preferredBrowserSession: nil, additionalHeaderFields: [:])`.
4. **Isolation in Swift 5 mode**: `AVEngine` and `NowPlaying` assume the main
   thread (`MainActor.assumeIsolated` in the remote commands); `Model`'s
   closures handed to `Bridge` are formed on the main actor and called on
   the main queue. A warning is expected; an error means a
   `@MainActor`/`nonisolated` to add where it points.
5. **Linking the static library**: Security and SystemConfiguration are
   linked; if the archive reports undefined symbols from Rust's standard
   library, add what `rustc --print native-static-libs` names (`-lc++`,
   `-liconv`, `-lresolv`) to `OTHER_LDFLAGS` in `project.yml`.
6. **The test target**: `HarkenTests` reaches the bindings (`Hall`, `Player`)
   through `@testable import Harken`; if the test host does not load, set
   `TEST_HOST`/`BUNDLE_LOADER` in `project.yml` (XcodeGen normally does).
7. Small things a compiler finds at once: a ternary's colour type
   (`Color.gold : .secondary`), `Slider(value:in:onEditingChanged:)`, a
   `ForEach` over `enumerated()`, `UIDevice.current.name` (it is "iPhone"
   without an entitlement, which is the name other devices' pickers show).

## Not verified

Everything that needs Xcode, a simulator, a device or a real server: the
screens and their Liquid Glass, the motion, the AVPlayer engine and the
audio session in the background, interruptions and route changes, the lock
screen and Control Center, the sign-in sheet and the `harken://auth`
redirect, the covers, the app icon in App Store Connect, the xcframework
link, and the archive.
