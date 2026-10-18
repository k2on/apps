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

    nix build .#harken-apk              # result/harken.apk
    adb install result/harken.apk
    nix build .#harken-android-screens  # result/index.html and the PNGs

That is gradle over an SDK the flake assembles itself (`nix/android-sdk.nix`):
the platform 35 and build-tools 35.0.0 zips from Google, which are jars and
scripts, and `aapt2` — the one native program AGP runs to assemble an APK
(d8, apksigner and the rest are Java) — compiled from the Android sources
for whatever machine is building (`nix/aapt2.nix`). The app's whole Maven
graph — AGP, Compose, the Kotlin plugins, OkHttp, Media3, the browser
library, Paparazzi and the layoutlib it renders with — is recorded in
`deps.json` and replayed offline, the way nixpkgs builds every gradle
project; the Kotlin runtime and client come in through the composite build
as they do on a laptop. One recording serves the APK, the screenshots and
a laptop's debug build: it runs the onboarding's tests,
`recordPaparazziDebug`, `assembleDebug` and `assembleRelease`. A new
dependency moves that file:

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
keystore under `~/.android`.

**The APK is a release build.** `harken-apk` runs `assembleRelease`, not
`assembleDebug`: a debuggable app runs on ART with its compiler held back
for a debugger, and Compose is several times slower for it — a fling
through the songs most of all. A release build is not debuggable, and it
carries the baseline profiles Compose's own libraries ship, compiled by AGP
into `assets/dexopt/baseline.prof`, which profileinstaller hands to ART on
install. It is not minified: R8 is not in the recorded graph, and a fling
does not need shrinking. Lint's release check is off for the same reason,
since it would fetch lint. The debug build stays what a laptop builds and
what the screenshots and the tests run against.

**One key, committed, and public on purpose.** That keystore was
random: AGP makes one on first use, so every machine had its own, and the
nix builder — a new home each time — had a new one on every build. Android
refuses an update signed by anybody but the installed app's signer, so each
new APK failed with "App not installed" and had to replace the old one by
uninstalling it. `debug.keystore`, beside this file, is the key every build
signs with now, debug and release (`signingConfigs` in
`app/build.gradle.kts`, the usual `android` / `androiddebugkey` /
`android`), nix or local, so one installs over the last. Its name is AGP's
convention; what matters is that it is the key the APKs already installed
carry, certificate SHA-256 `1a8859fb…df431139`. The cost is the one a debug
key's build pays, stated rather than inherited: anyone can sign an APK as
this app, so it installs anywhere and belongs nowhere public. A real release key belongs outside the
nix store, which is world-readable, and is applied after the build.

The version code rises with each commit: `harken-apk` passes the flake's
source time in minutes as `-PversionCode` and `0.1.<date>` as
`-PversionName`, which CI's and a tag's APKs get too, since both are this
same `nix build`. A local gradle build passes neither and is `1`, `0.1`.

**The first install of this build still needs one uninstall.** The APK
already on a phone carries the random key of the build that made it; from
this one on, updates install over each other.

With Android Studio (Ladybug or later) or a command-line SDK with platform 35
and build-tools 35.0.0, plus JDK 21, the same project builds directly:

    cd harken/android
    gradle assembleDebug          # or open the directory in Android Studio
    gradle :ark-ui:testDebugUnitTest :ark-android:testDebugUnitTest :app:testDebugUnitTest
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
under this build's plugin versions. So is `kotlin/ark-ui`, the look and the
motion `ark-android` and the app are drawn with (`include(":ark-ui")`), on
Compose alone.

- The Kotlin build sets `jvmToolchain(21)`, so the libraries arrive as Java
  21 class files; AGP 8.7's D8 accepts them, and the app is built for 21 too.
- The composite needs the same Kotlin plugin version on both sides; both say
  2.0.21.
- The client is written for API 26, the app's `minSdk`: `java.nio.file` only
  where 26 has it (a directory synced after a rename), no `java.net.http`;
  its HTTP is `HttpURLConnection` and its WebSocket OkHttp's.

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
  or the Library's root goes Home, previewed like a page. How is `kotlin/ark-ui` (`Motion.kt`, `ExpandingCard.kt`,
  `Springs.kt`), which every Ark app on Android shares; `PlayerSheet.kt`
  here says which faces the player's card has, and `docs/plan-android.md`
  why.
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
