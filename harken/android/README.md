# harken for Android

Compose over the Kotlin runtime (`kotlin/ark-runtime`), the Kotlin client
(`kotlin/ark-client`) and the generated domain
(`domain/gen/kotlin/HarkenGen.kt`). It is the phone the top-level README
describes: an exact replica of the `library` and `playlists` scopes, running
the same generated code the server and the desktop run, with nothing
crossing a bridge.

**Assembled, not run.** `nix build .#harken-apk` compiles every Kotlin file
here and produces the APK, so the Compose code is at least what the compiler
accepts; nothing in the container this was written in can install or run
it. What *has* been exercised is everything under it: `ark-runtime` and `ark-client` build
and pass their tests under `gradle build` (JVM 21), `HarkenGen.kt` compiles
against them, and a JVM smoke test made exactly the calls `Model.kt` makes
— open a session, `create_playlist`, `add_to_playlist`, `remove_from_playlist`
through the generated bodies, the three queries through `HarkenGen.query`, a
refusal surfaced, both scopes verified against the peer's own authority. The
Compose and navigation code is written against the current stable APIs
(Compose BOM 2024.12.01, Material 3, Navigation 2.8) and checked by reading.
Expect the first build on a real machine to want small fixes.

## Building

From the repository root, with nothing but nix:

    nix build .#harken-apk            # result/harken-debug.apk
    adb install result/harken-debug.apk

That is gradle over an SDK the flake assembles itself (`nix/android-sdk.nix`):
the platform 35 and build-tools 35.0.0 zips from Google, which are jars and
scripts, and `aapt2` — the one native program AGP runs for `assembleDebug`
(d8, apksigner and the rest are Java) — compiled from the Android sources
for whatever machine is building (`nix/aapt2.nix`). The app's whole Maven
graph — AGP, Compose, the Kotlin plugins, OkHttp — is recorded in
`deps.json` and replayed offline, the way nixpkgs builds every gradle
project; the Kotlin runtime and client come in through the composite build
as they do on a laptop. A new dependency moves that file:

    nix run .#harken-apk-deps         # re-records deps.json; needs the network

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
and build-tools 35.0.0, plus JDK 17 or 21, the same project builds directly:

    cd harken/android
    gradle assembleDebug          # or open the directory in Android Studio
    adb install app/build/outputs/apk/debug/app-debug.apk

The domain must have been generated first, because the app references it by
source directory rather than copying it:

    nix run .#arkc -- gen kotlin harken/domain/harken.ark \
        harken/domain/gen/kotlin --name Harken \
        --only create_playlist,add_to_playlist,remove_from_playlist,library,playlists,playlist_items

`app/build.gradle.kts` adds `../../domain/gen/kotlin` as a source directory.
The generated `object HarkenGen` (package `harken.gen`) carries the whole
module's bytes (`MODULE_BYTES`, `add_track` included, so the replica can
replay the server's entries by intent), the three mutators the phone
authors, the three queries, `apply`, `query` and the typed `<name>Args`
builders. Regenerate it whenever `domain/` changes; never edit it.

### How the runtime gets in

`settings.gradle.kts` says `includeBuild("../../kotlin")`: a composite build.
Gradle substitutes the app's `implementation("dev.arkdb:ark-runtime")` and
`implementation("dev.arkdb:ark-client")` with the modules of that build, which
keeps its own settings, plugin versions and toolchain. A project reference
(`include(":ark-runtime")` with a redirected `projectDir`) would have worked
too, but would have pulled those modules under this build's plugin
management; the composite is the smaller intrusion.

Two things to know if the build objects:

- The Kotlin build sets `jvmToolchain(21)`, so the two libraries arrive as
  Java 21 class files. AGP 8.7's D8 accepts them; an older AGP would not, and
  the fix is to bump AGP rather than to lower the toolchain (the JVM build
  is pinned to 21 by its own contract).
- The composite needs the same Kotlin plugin version on both sides; both say
  2.0.21.

## What it does

Four screens under one `Scaffold`, with a bottom bar and a status line in the
top bar (`linked`, `offline, retry in 4s`, `alone`, `turned away: …`, and the
pending count):

- **Library** — every track, by artist, album, title. A `+` per row puts the
  track on the *selected* playlist. Tracks arrive from the server's scanner
  as facts (this build carries no `add_track`); alone, the library is empty
  and the screen says so.
- **Playlists** — every playlist by name; a `+` opens a dialog to name a new
  one (`create_playlist` trims and refuses a blank; a second playlist of the
  same name for the same person is a no-op, not a duplicate). Tapping one
  selects it and opens it.
- **Playlist** — its items in `pos` order, each joined to its track by the
  model (the two are in different scopes, so a query reads one store and the
  join is a map lookup). Swipe left, or the trash button, removes one. An item
  whose track has not arrived is drawn as `unavailable`.
- **Settings** — the server URL (`ws://…/sync`; `10.0.2.2` reaches the
  emulator's host), the user (dev auth: a name is a login), and **work
  alone**, which opens the session with no server so the phone sequences its
  own playlists (docs/arkdb.md §3.10). Applying closes the session and
  reopens it. A status block prints each scope's cursor, pending, rejections
  and divergences, and a button asks the authority to verify.

Every mutation goes through `Session.mutateWith`, so it is the **generated**
code that computes the optimistic change:

```kotlin
session.mutateWith("add_to_playlist", HarkenGen.addToPlaylistArgs(pid, tid)) { db ->
    HarkenGen.addToPlaylist(db, ctx, autos, args)
}
```

The entry recorded is byte-identical to what the interpreter would record,
and `kotlin/ark-client`'s tests hold the two to the same changes and the same
hash on the demo module. A rebase, though, replays pending intents through
the interpreter (the runtime's `Replica` does), which is why the module bytes
travel with the app.

`Model.kt` is one `AndroidViewModel` around a `Session`: it pumps it every
50 ms from `viewModelScope` (so every call into the session is on the main
thread, the session's contract), re-reads the three lists on the session's
change listener, and turns refusals and rejections into a snackbar. State
lives under `filesDir/ark/<user>/<alone|server>/` as one canonical-CBOR file
per scope — confirmed store, cursor, pending intents, and, alone, the log the
phone sequenced — written to a temp file and renamed.

## Not here yet

- Sign-in: dev auth only; the token on the socket is the user's name.
- Playback: `track.file` is shown, not played.
- Adopting a phone's own log into a server it later meets (§3.10) is not in
  the client machine; switching from alone to a server starts a fresh
  replica directory.
- The server's `Agree` after **Verify** arrives on a later pump and is not
  yet drawn; alone, the answer is immediate and shown.
