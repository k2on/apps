# harken for Android

Compose over the Kotlin client at spec v4 (`kotlin/ark-runtime`,
`kotlin/ark-client`, `kotlin/README.md`), inside the onboarding every Ark
app on Android shares (`kotlin/ark-android`). The phone is a **client**: it
loads `harken.ark` — the module the domain crate writes, the one the
server, the desktop and the browser run — from its own assets, verifies
it, and interprets every function of it. Nothing of the domain is written
in Kotlin, and nothing is generated into the app: what the screens call is
a mutation or a query of the module, by name (`docs/plan-phones.md`). What
it looks like and how it behaves is `docs/plan-android.md`.

**Assembled and rendered, not run.** `nix build .#harken-apk` compiles
every Kotlin file here and runs the onboarding's JVM tests before it
produces the APK; `nix build .#harken-android-screens` renders every
screen, light and dark, on the JVM (below). Nothing in the container this
was written in can install or run an APK, and no phone has run this one.
What *has* been exercised is everything under it on the JVM: the runtime
and the client against every client vector and trace (`checks.kotlin`),
`harken-peer-kt` in the process fleet beside Rust peers
(`checks.fleet-kotlin`), the onboarding's machine, and the screens drawn
by layoutlib. Expect the first run on a real phone to want small fixes —
the media session, the Custom Tab's redirect and the notification
permission are the parts only a phone can say anything about.

## Building

From the repository root, with nothing but nix:

    nix build .#harken-apk              # result/harken-debug.apk
    adb install result/harken-debug.apk
    nix build .#harken-android-screens  # result/index.html and the PNGs

That is gradle over an SDK the flake assembles itself (`nix/android-sdk.nix`):
the platform 35 and build-tools 35.0.0 zips from Google, which are jars and
scripts, and `aapt2` — the one native program AGP runs for `assembleDebug`
(d8, apksigner and the rest are Java) — compiled from the Android sources
for whatever machine is building (`nix/aapt2.nix`). The app's whole Maven
graph — AGP, Compose, the Kotlin plugins, OkHttp, Media3, the browser
library, Paparazzi and the layoutlib it renders with — is recorded in
`deps.json` and replayed offline, the way nixpkgs builds every gradle
project; the Kotlin runtime and client come in through the composite build
as they do on a laptop. One recording serves the APK and the screenshots:
it runs the onboarding's tests, `recordPaparazziDebug` and
`assembleDebug`. A new dependency moves that file:

    $(nix build .#harken-apk-deps --print-out-paths)   # from the repository root; needs the network

(`nix run .#harken-apk-deps` is the same script, on a nix whose `run`
takes a bare file; some want a `bin/`.) The script runs `nix-shell` inside
bubblewrap, so it needs a nix daemon to talk to. Maven Central rate-limits
a cold fetch of the graph (HTTP 429), both while recording and when the
build fetches what was recorded; either is retried after a pause, and the
artifacts already fetched stay in the store.

**Why aapt2 is compiled here, and how.** Google ships the SDK's native tools
for x86_64 Linux and macOS only, so an ARM Linux machine has no `aapt2` to
run — and the newest one anybody else prebuilds for aarch64 glibc, Debian's,
is Android 14's and cannot read platform 35's `android.jar`, whose resource
table is in Android 15's format. `nix/aapt2.nix` builds it at the
`platform-tools-35.0.2` tag: sparse checkouts of `frameworks/base`
(tools/aapt2, libs/androidfw) and `system/core` from GitHub's aosp-mirror,
Debian's source tarball of the same release for the libraries whose
repositories the mirror lacks (libbase, liblog, libziparchive, incfs's
`map_ptr`, the native headers, fmtlib), and nixpkgs' protobuf 3.21, libpng,
expat and zlib. `nix/aapt2/CMakeLists.txt` compiles the host source lists
the Android build files name, with clang: Android's own toolchain, and the
one that accepts C11 `atomic_int` in C++ where gcc needs Debian's patch.
Two things glibc's libstdc++ wanted that Android's libc++ did not: a
prelude that includes `<cstring>`, `<cstdint>` and `<limits>` ahead of
androidfw and aapt2, and Debian's one-hunk patch giving incfs's `map_ptr`
iterator the decrement `std::lower_bound` uses. About two hundred files,
a few minutes, once. The same recipe is what the x86_64 build uses, which
is where it was tested; the aarch64 build differs by nothing but the
compiler's target.

Three more things the derivation says so that AGP stays inside the store:
`buildToolsVersion = "35.0.0"` in `app/build.gradle.kts`, because AGP 8.7
otherwise asks for its own default 34.0.0 and tries to *install* it;
`android.aapt2FromMavenOverride`, pointing at the aapt2 above rather than
the x86_64 one AGP would fetch from Maven; the whole unpacked tree made
writable, because stdenv unlocks only the source root and gradle writes
`.gradle/` and `build/` inside the included build at `../../kotlin` too —
read-only, the build *ends* after loading settings with no task run and no
message; and a writable `HOME`, because AGP keeps its state and the debug
keystore under `~/.android`. The APK is signed with the debug key every
Android toolchain shares: it installs anywhere and belongs nowhere public.

With Android Studio (Ladybug or later) or a command-line SDK with platform 35
and build-tools 35.0.0, plus JDK 21, the same project builds directly:

    cd harken/android
    gradle assembleDebug          # or open the directory in Android Studio
    gradle :ark-android:testDebugUnitTest :app:testDebugUnitTest
    gradle :app:recordPaparazziDebug   # app/src/test/snapshots/images, gitignored
    adb install app/build/outputs/apk/debug/app-debug.apk

The flake's SDK works for that too: `nix develop .#kotlin`, then
`ANDROID_HOME` set to `$(nix build .#android-sdk --print-out-paths)/libexec/android-sdk`
and gradle given the flags `harken-apk` gives it (the JDK, and
`android.aapt2FromMavenOverride` pointing at that SDK's aapt2).

**The module is an asset.** `app/build.gradle.kts` copies
`harken/domain/harken.ark` into `build/generated/ark-assets` before every
build (`copyArk`) and adds that directory to the app's assets; the app reads
`assets/harken.ark` at start and hands the bytes to `Domain.fromBytes`, which
decodes and verifies it. A new release of the domain is a new `harken.ark`
and a rebuild; nothing in the app's sources moves with it.

### How the runtime gets in

`settings.gradle.kts` says `includeBuild("../../kotlin")`: a composite build.
Gradle substitutes the app's `implementation("dev.arkdb:ark-runtime")` and
`implementation("dev.arkdb:ark-client")` with the modules of that build, which
keeps its own settings, plugin versions and toolchain. The onboarding,
`kotlin/ark-android`, is an Android library and so cannot be a project of
that JVM-only build: it is a project of this one, included by path
(`include(":ark-android")`, its `projectDir` set to `../../kotlin/ark-android`),
under this build's plugin versions.

- The Kotlin build sets `jvmToolchain(21)`, so the libraries arrive as Java
  21 class files; AGP 8.7's D8 accepts them, and the app is built for 21 too.
- The composite needs the same Kotlin plugin version on both sides; both say
  2.0.21.
- The client is written for API 26, the app's `minSdk`: `java.nio.file` only
  where 26 has it (a directory synced after a rename), no `java.net.http`;
  its HTTP is `HttpURLConnection` and its WebSocket OkHttp's.

## What it does

`MainActivity` is one line of harken and the rest is the shared onboarding:

```kotlin
ArkApp(identity, listOf(Permission.Notifications(why = WHY_NOTIFY))) { session -> HarkenApp(session) }
```

**There is no alone mode.** harken is never used without a server: the
onboarding (`kotlin/ark-android`, `kotlin/README.md`) asks for the server's
address and checks its `/healthz` is an Ark server serving `harken.ark`,
signs in on the server's own page in a Custom Tab
(`/auth/login?redirect=harken://auth`; the manifest takes `harken://auth`
on a `singleTask` activity, and harken's server allows `harken://`),
exchanges the code, asks for notifications once, and remembers the server
and the login. Only then is there a session: the Kotlin `Peer` over a
replica directory per server, signed in, linked to `/sync`, on a thread of
its own that runs only when the socket wakes it, when the app asks for
something, and at the redial after a close (`docs/plan-reactive.md`); the
library splices what each change moved, and the player's position ticks
only while a track plays, read by the mini player's hairline and the full
player's scrubber alone. A server's `Denied` and
*Sign out* both come back to *Sign in* with the server and the replica
kept, so whatever was pending goes when the same person signs in again.

**The lists are spliced, not re-read.** `Library` holds the library, the
playlists, the albums, the artists and the songs' details as maintained
views (`Peer.view`, `ClientView`): hydrated once on the session's thread,
then handed every `Changes` the peer reports — a rebase's included — and
the patches each answers with are spliced into the list the screens hold
(`ClientView.splice`). Only a `Reset` reads one again. A page — an album,
an artist, a playlist — is a view too, held while a screen shows it and
dropped when the last one leaves. Search is one read per needle of the
module's `search`, over its text index. Every write is a mutation of the
module by name:

```kotlin
peer.mutate("add_to_playlist", mapOf("playlist_id" to Value.id(pid), "media_id" to Value.id(mid)))
peer.view("album", mapOf("playlist_id" to Value.id(Id.nil), "name" to Value.text("Water Music")))
```

and what the phone records is byte-identical to what any other peer
records for the same intent.

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
- **A song's menu** — play next, add to a playlist (every playlist,
  ticked by `playlists_of`, a tap putting it on or taking it off), go to
  the album or the artist, and on a playlist, remove from it.
- **The player** — Media3's ExoPlayer in `PlaybackService`, a
  `MediaSessionService`, streaming `/media/<file>` from the session's
  server (`Media.url`, the desktop's `media_url`: each segment
  percent-encoded, a whole URL passed through); the notification, the lock
  screen and Bluetooth are the media session's. The screens hold a
  `MediaController` (`Playback`): the queue is the list a track was started
  from, snapshotted; shuffle; repeat off, all, one. A mini player above the
  tab bar opens into the full player — cover, title, artist, a scrubber
  with the time gone and the time left, previous / play-pause / next,
  shuffle, repeat — and the queue is a sheet over it.
- **Settings** — the account, the server and whether it is reached, sign
  out, about. **Diagnostics** — every number the old settings screen had,
  each change this phone made with where it stands, and verify — are
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

## The screens as images

    nix build .#harken-android-screens     # result/index.html

Paparazzi (1.3.5) renders every screen and state on the JVM through
layoutlib — the onboarding's three steps and their errors, Home (full,
offline, empty, loading), Search (recent, nothing yet, typing, results,
none), each Library tab, an album, an artist, a playlist (and an empty
and a missing one), the mini player, the full player, the queue, a song's
menu, adding to a playlist, naming one (and a name refused), Settings and
diagnostics — in light and dark, at a Pixel 5's 1080×2340, from
`app/src/test/.../Demo.kt`: rows taken from harken's demo seed
(`harken/iced/src/seed.rs`). `index.html` lays them out light beside dark.
A bottom sheet and a dialog are windows of their own, which layoutlib does
not draw, so the screenshots frame the same composables over a scrim
instead. Nothing behind them talks to a server, so every cover is the
derived square.
