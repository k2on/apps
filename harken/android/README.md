# harken for Android

Compose over harken's library (`../ffi`, `docs/plan-v5.md` §1.16): the Rust
kernel with harken's domain linked in, `libharken_ffi.so`, the same code
the server, the desktop and the browser run. The phone is a **client** of
it: the kernel keeps the replica on the phone's disk, dials the server,
rebases, keeps the views, and holds the listening session; the app's own
code is its screens, its player, its car and its onboarding. It calls the
domain by name through the Kotlin `ark-dev gen kotlin` writes from the
registered domain — data classes for every record, row and sum, and a stub
per function (`app/src/main/kotlin/dev/harken/android/gen/Harken.kt`, never
edited: `ARK_WRITE_GEN=1 cargo test -p harken-ffi --test gen` writes it,
and the same test fails when it is not what the domain generates) — over
UniFFI's bindings and the bridge's typed half (`dev.arkdb.ffi`).
Views are typed and functions are by name: the views whose rows are
`LibraryEntry` or `Playlist` (the library, its pages, the playlists) hand
over UniFFI's own records (`uniffi.harken_domain`, `#[ark(ffi_view)]`), with
no JSON per row, while every mutation, query and live call goes by name
with JSON through the generated stubs.
What it looks like and how it behaves is `docs/plan-android.md`.

**Assembled and tested on the JVM, not run.** `nix build .#harken-apk`
compiles every Kotlin file here, runs the app's JVM tests, and assembles
the release APK with the kernel for `arm64-v8a`, `armeabi-v7a` and
`x86_64`. The tests are the app's own over the **real kernel**: the
library built for the machine they run on (`harken-android-lib`'s
`host/`), loaded through the same UniFFI Kotlin and JNA the phone uses —
a library seeded through `add_song` and read by the screens' `Library`,
the reconciler and the lock screen's player against a listening session
in a `Hall` (a harken server in the process, `../ffi`), the onboarding's
machine, and the screens drawn by layoutlib. Nothing in the container this
was written in can install or run an APK, and no phone has run this one:
the media session, the Custom Tab's redirect, JNA's loading on a device and
the notification permission are the parts only a phone can say anything
about.

## Developer loop

Every generated file the app compiles of the domain — `gen/Harken.kt` and
UniFFI's own Kotlin for its twins, `gen/uniffi/harken_domain.kt`
(`uniffi.harken_domain`, which the library's `kotlin/` leaves out) — is
checked in and written from the domain's source by one command, with no
compiler: `ark-dev gen` (built once, `cargo build --release -p ark-dev`;
`--check` writes nothing and fails on a file that differs). It takes about
40 ms, so it runs on save and the IDE's Kotlin sees a changed domain type
at once:

    autocmd BufWritePost harken/domain/src/*.rs silent !ark-dev gen
    watchexec -w harken/domain/src -e rs -- ark-dev gen    # or: ls harken/domain/src/*.rs | entr ark-dev gen

## Building

From the repository root, with nothing but nix:

    nix build .#harken-apk              # result/harken.apk
    adb install result/harken.apk
    nix build .#harken-android-screens  # result/index.html and the PNGs

`harken-apk` is gradle over two things the flake builds. **The kernel**,
`harken-android-lib`: `harken/ffi` compiled by cargo for each ABI against
API 26 with the NDK nixpkgs packages (`aarch64-linux-android`,
`armv7-linux-androideabi`, `x86_64-linux-android`, linked by the NDK's
clang, which also compiles ring's C), stripped, as `jniLibs/<abi>/`;
UniFFI's Kotlin for it and the typed half as `kotlin/`; and the library
for the build machine as `host/`, which the JVM tests load
(`jna.library.path`). **An SDK** the flake assembles itself
(`nix/android-sdk.nix`): the platform 35 and build-tools 35.0.0 zips from
Google, which are jars and scripts, and `aapt2` — the one native program
AGP runs to assemble an APK — compiled from the Android sources for
whatever machine is building (`nix/aapt2.nix`). The app's whole Maven graph
— AGP, Compose, the Kotlin plugins, Media3, the browser library, JNA,
Paparazzi and the layoutlib it renders with — is recorded in `deps.json`
and replayed offline, the way nixpkgs builds every gradle project. A new
dependency moves that file:

    $(nix build .#harken-apk-deps --print-out-paths)   # from the repository root; needs the network

(JNA 5.17.0 and `org.json` were added to it by hand, their hashes those of
Maven Central's files.) Maven Central rate-limits a cold fetch of the graph
(HTTP 429); a fetch that fails is retried after a pause, and the artifacts
already fetched stay in the store.

On a laptop, with Android Studio or a command-line SDK with platform 35
and build-tools 35.0.0 plus JDK 21, the kernel built once and gradle
pointed at it:

    nix build .#harken-android-lib -o harken/ffi/android   # what -PharkenLib defaults to
    cd harken/android
    gradle assembleDebug
    gradle :app:testDebugUnitTest
    adb install app/build/outputs/apk/debug/app-debug.apk

**Why aapt2 is compiled here, and how.** Google ships the SDK's native tools
for x86_64 Linux and macOS only, so an ARM Linux machine has no `aapt2` to
run — and the newest one anybody else prebuilds for aarch64 glibc, Debian's,
is Android 14's and cannot read platform 35's `android.jar`, whose resource
table is in Android 15's format. `nix/aapt2.nix` builds it at the
`platform-tools-35.0.2` tag from sparse checkouts of the AOSP mirror,
Debian's source tarball of the same release for the libraries the mirror
lacks, and nixpkgs' protobuf, libpng, expat and zlib, compiled with clang
(`nix/aapt2/CMakeLists.txt`). About two hundred files, a few minutes, once.

Three more things the derivation says so that AGP stays inside the store:
`buildToolsVersion = "35.0.0"` in `app/build.gradle.kts`, because AGP 8.7
otherwise asks for its own default 34.0.0 and tries to *install* it;
`android.aapt2FromMavenOverride`, pointing at the aapt2 above rather than
the x86_64 one AGP would fetch from Maven; and a writable `HOME`, because
AGP keeps its state under `~/.android`.

**The APK is a release build.** `harken-apk` runs `assembleRelease`, not
`assembleDebug`: a debuggable app runs on ART with its compiler held back
for a debugger, and Compose is several times slower for it. A release
build carries the baseline profiles Compose's own libraries ship. It is not
minified: R8 is not in the recorded graph. Lint's release check is off for
the same reason.

**One key, committed, and public on purpose.** `debug.keystore`, beside
this file, is the key every build signs with, debug and release
(`signingConfigs` in `app/build.gradle.kts`), so one APK installs over the
last; its certificate's SHA-256 is `1a8859fb…df431139`. Anyone can read
this file, so anyone can sign an APK as this app: it installs anywhere and
belongs nowhere public. A real release key belongs outside the nix store,
and is applied after the build. The version code rises with each commit:
`harken-apk` passes the flake's source time in minutes as `-PversionCode`.

## Through F-Droid

The APK CI builds from `main` is also an F-Droid repository, served with
the browser peer by GitHub Pages:

    https://k2on.github.io/apps/fdroid/repo?fingerprint=E48F7E6E1AB9C74C4F0404835580DE590334C636EAF654680E1BF0D250BEF3DD

Add it in F-Droid (Settings → Repositories → **+**) and every later build
arrives as an update. `nix build .#harken-fdroid-repo` is the repository
for this flake's APK, signed and checked; `fdroid/README.md` is how it is
published, why its index key is committed, and how to move to a private
one.

The first install from F-Droid over a copy installed by hand works without
an uninstall, because both are signed with the committed debug key — as
long as that copy was built at or after 59fd287. A copy from before it
carries its own build's random key and needs one uninstall, as above.

## What it does

`MainActivity` is one line of harken and the rest is the onboarding
(`onboarding/`, drawn with `ui/`, both the app's own code now):

```kotlin
ArkApp(identity, listOf(Permission.Notifications(why = WHY_NOTIFY))) { session -> HarkenApp(session) }
```

**There is no alone mode.** harken is never used without a server: the
onboarding asks for the server's address and checks its `/healthz` is a
harken server (its `domain` line against this library's, `harken_domain`),
signs in on the server's own page in a Custom Tab (`login_url`, the
manifest taking `harken://auth` on a `singleTask` activity, which harken's
server allows), exchanges the code through the kernel (`exchange`), asks
for notifications once, and remembers the server and the login. Only then
is there a session (`onboarding/ArkSession.kt`): the kernel's peer over a
replica directory per server, signed in, dialling `/sync`. The peer drives
itself — its link, its redial and its views run on a thread of the
kernel's — and tells the session when anything moved; the session then runs
one drain on its own thread, where the library splices what moved, and
nothing runs on a clock. A server's denial (the status's `denied`) and
*Sign out* both come back to *Sign in* with the server and the replica
kept, so whatever was pending goes when the same person signs in again.

**The lists are spliced, not re-read.** `Library` holds the library, the
playlists, the albums, the artists and the songs' details as the kernel's
maintained views, typed (`gen.View<T>`): read once on the session's thread,
then asked after every change what they did — a rebase's included — and
their patches spliced into the list the screens hold. Only a reset reads
one again. A page — an album, an artist, a playlist — is a view too, held
while a screen shows it and let go when the last one leaves. Every write is
one of the domain's functions, typed:

```kotlin
h.addToPlaylist(AddToPlaylist(playlistId, mediaId))           // its intent's id
h.album(AlbumInput(Id.nil, "Water Music")).rows()               // List<LibraryEntry>
```

and the input the phone records is the JSON the domain's serde writes, the
same bytes any other peer records for the same intent.

The screens, each a function of plain values (so the screenshots draw
exactly what the app draws):

- **Home** — what was played last, from where, and a button that plays
  that list again from that track (remembered on the phone, `Device`);
  recently added; your playlists; the albums — shelves of covers, each
  opening its page. Settings is the one thing in its bar.
- **Search** — one field, answered as you type: songs from `search`,
  albums and artists from the held lists whose names hold the needle;
  the last few searches when nothing is typed, on the phone only.
- **Library** — Playlists, Albums, Artists, Songs; a new playlist from
  the `+`, checked as it is typed by `create_playlist`'s own validator,
  whose sentence is shown under the field.
- **An album** in the order the work goes, in sections where it has
  parts and with each track's number on the release; **an artist** with
  their albums and then their songs; **a playlist** in the order it was
  made. Each opens with its cover, what it is, the numbers true of the
  whole of it, and Play and Shuffle.
- **A song's menu** — play next and add to queue (the session's
  `enqueue`, the track playing playing on; and a car's or a watch's
  added, removed and moved items are `enqueue`, `remove` and `move`),
  add to a playlist (every playlist,
  ticked by `playlists_of`, a tap putting it on or taking it off), go to
  the album or the artist, and on a playlist, remove from it.
- **The player** — Media3's ExoPlayer in `PlaybackService`, a
  `MediaLibraryService` (a car browses it, "In a car" below), streaming `/media/<file>` from the session's
  server (`Media.url`, the desktop's `media_url`: each segment
  percent-encoded, a whole URL passed through); the notification, the lock
  screen and Bluetooth are the media session's. The screens hold a
  `MediaController` (`Playback`): the queue is the list a track was started
  from, snapshotted; shuffle; repeat off, all, one. A bar above the tab
  bar is the full player's other face — cover, title, artist, a scrubber
  with the time gone and the time left, previous / play-pause / next,
  shuffle, repeat — and the queue is a sheet over it. The bar is dragged
  up into the full player under the finger, or tapped; dragged sideways,
  it skips.
- **Motion** — a page grows out of the cover that was tapped, and goes
  back into it: by the back button, by the system's back gesture
  (Android 14 and later draw the page shrinking under the finger), or
  pulled down from its top. A page opened from no cover rises. Each tab
  keeps its own stack and switches to it at once; the tab already shown
  goes back to its root, or scrolls its root to the top; back at Search's
  or the Library's root goes Home, previewed like a page. How is `ui/` (`Motion.kt`, `ExpandingCard.kt`,
  `Springs.kt`), what was ark-ui and is the app's own now; `PlayerSheet.kt`
  here says which faces the player's card has, and `docs/plan-android.md`
  why.
- **Settings** — the account, the server and whether it is reached, sign
  out, about. **Diagnostics** — every number the peer's status has, the
  listening session's, and each change this phone made with where it
  stands — are
  behind the version, tapped seven times, and listed under About once
  found.

**Covers** are fetched from the server once and cached twice — the encoded
original on disk under the cache directory, decoded bitmaps in a
byte-bounded memory cache — and decoded already shrunk (`inSampleSize` from
the image's own bounds), with no image library. Under every cover, and in
place of a missing one, is the desktop's derived square: FNV-1a over UTF-16
picks one of the same six gradients, so an album is one square on every
screen.

**The look** is the desktop's: `Theme.kt` is the only file with a literal
colour — AppKit's neutral greys and the two golds as Material 3 light and
dark schemes, one type scale, one set of spacings — and the glyphs are the
desktop's Lucide paths (`Glyphs.kt`). An empty library says where music
comes from; a server that cannot be reached is one sentence over every
tab, and what is on the phone keeps playing.

## In a car

harken is an Android Auto media app (`docs/plan-android.md`, "Android
Auto"): `PlaybackService` is a Media3 `MediaLibraryService`, and the car
browses Home, Playlists, Albums and Artists, searches, takes "play … on
Harken" by voice and plays on the phone — from the replica, so browsing
works with no signal; playing streams from the server, and a car out of
its reach says so. Nothing here has been run in a car or on the Desktop
Head Unit.

**Android Auto hides an app it did not get from the Play Store**, which is
every way this APK arrives (F-Droid, a hand install, CI). To let it in, on
the phone, once:

1. Open Android Auto's settings: on Android 10 and later, Settings → Apps
   → See all apps → Android Auto → Additional settings in the app (on some
   phones Settings → Connected devices → Connection preferences → Android
   Auto); on Android 9 and earlier, the Android Auto app's menu → Settings.
2. Scroll to **Version** at the bottom and tap **Version and permission
   info** ten times; answer **OK** to "Allow development settings?".
3. Open the **⋮** menu at the top right → **Developer settings**, and turn
   on **Unknown sources** near the bottom.
4. Back in Android Auto's settings, **Customize launcher** (or "Customise
   launcher"), and tick **Harken** if it is not ticked.
5. Disconnect from the car and connect again; if Harken still is not
   there, restart the phone, which some phones need before the change
   shows.

Steps 1 and 2 are Google's own (developer.android.com, "Test Android
apps for cars"); Google's page says the developer option exists and that
it covers media apps but does not spell out its label or where it sits,
so steps 3 to 5 are as the option is commonly described for current
versions of Android Auto, and the wording may differ on yours. The setting
applies to media apps like this one; it does nothing for apps built with
the Car App Library.

## The screens as images

    nix build .#harken-android-screens     # result/index.html

Paparazzi (1.3.5) renders every screen and state on the JVM through
layoutlib — the onboarding's three steps and their errors, Home (full,
offline, empty, loading), Search (recent, nothing yet, typing, results,
none), each Library tab, an album, an artist, a playlist (and an empty
and a missing one), the mini player, the full player, the player held at
each quarter of the way up, the queue, a song's
menu, adding to a playlist, naming one (and a name refused), Settings and
diagnostics — in light and dark, at a Pixel 5's 1080×2340, from
`app/src/test/.../Demo.kt`: rows taken from harken's demo seed
(`harken/iced/src/seed.rs`). `index.html` lays them out light beside dark.
A bottom sheet and a dialog are windows of their own, which layoutlib does
not draw, so the screenshots frame the same composables over a scrim
instead. Nothing behind them talks to a server, so every cover is the
derived square.
