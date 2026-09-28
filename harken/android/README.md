# harken for Android

Compose over the Kotlin runtime (`kotlin/ark-runtime`), the Kotlin client
(`kotlin/ark-client`) and harken's domain written in the authoring
vocabulary (`domain/gen/kotlin/{Schema,Library,Playlists,Module}.kt`,
package `harken.gen` — see `spec/AUTHORING.md`). It is the phone the
top-level README describes: an exact replica of the log that runs the
domain **natively, in Kotlin** — the same
program that, run under Emit, is the module the server and the desktop hold
by hash — with nothing crossing a bridge and no code generated into a
private shape.

**Assembled, not run.** `nix build .#harken-apk` compiles every Kotlin file
here and produces the APK, so the Compose code is at least what the compiler
accepts; nothing in the container this was written in can install or run
it, and no phone has run it. What *has* been exercised is everything under
it: `ark-runtime` and `ark-client` build and pass their tests under `gradle
build` (JVM 21); the four domain files compile in the runtime's tests, emit
a module that verifies, every procedure of them hashes as `harken.ark`'s,
and each is run natively and by the interpreter over its emitted IR and
must agree, refusals included (`kotlin/ark-runtime/src/test/.../Authoring.kt`).
`Phone.kt`, the part of the app below the screens, has no Android in it;
it was compiled off the phone with the domain and driven once by hand
against a `LocalHub` running the whole of `harken.ark` (a scanner authoring
`add_song`, a signed-out phone signing in, a second and a third device, a
re-login) — a scratch program, not a committed test, since the app's build
has no test dependencies recorded. The Compose and navigation code is
written against the current stable APIs (Compose BOM 2024.12.01, Material
3, Navigation 2.8) and checked by reading and by the compiler. Expect the
first run on a real phone to want small fixes.

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

The domain is referenced by source directory rather than copied:
`app/build.gradle.kts` adds `../../domain/gen/kotlin`. Those four files are
what `arkc gen kotlin` prints from `harken.ark` — the line-for-line Kotlin
spelling of `domain/src/{schema,library,playlists,module}.rs` — with only
what the phone calls (`clientFunctions` in `flake.nix`):

    nix run .#arkc -- gen kotlin harken/domain/harken.ark \
        harken/domain/gen/kotlin --package harken.gen --name Harken \
        --only create_playlist,add_to_playlist,remove_from_playlist,library,playlists,playlists_of,playlist \
        --fmt "ktfmt --kotlinlang-style"

So `Library.kt` has the `library` query and no `add_song`: the scanner's
entries arrive with what the server sends for them, which the replica
applies without running anything. The schema is always whole. `harken.gen.module()` is a
`dev.arkdb.authoring.Module`; `Session.open(dir, module(), user, url)`
emits it once for the IR the replicas hash and verify against, and hands
the session its `procedures()` to run natively. Never edit the four files
by hand once the printer writes them; regenerate.

The app is built for JVM 21 (`compileOptions`, `jvmTarget`), as the
runtime is: the domain calls the vocabulary's inline functions
(`router<S>()`, `col<T, V>()`, `ctx.newId(..)`), and Kotlin refuses to
inline bytecode built for a newer JVM than its caller. D8 accepts Java 21
class files.

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
top bar (`signed out`, `linked`, `offline, retry in 4s`, `alone`, `turned
away: …`, and how many changes are not synced yet):

- **It works with nobody signed in.** The first launch opens the session
  with no user (`Session.open(dir, module, null, url)`): everything is
  authored as nobody, applied, written down, and nothing is sent anywhere.
  Signing in (Settings → Account; dev auth, a name is a login) makes all of
  it that person's (`Session.signIn`) and syncs it; the app then opens as
  them on every launch. Signing out reopens with nobody over the same
  replica — one directory per server, not per person, because the log is
  the server's — and whatever that person left pending stays theirs and
  goes when they sign in again: the server accepts an entry authored under
  an older login of the same person.
- **Library** — everything, in the order it was added (`library`, read
  against the selected playlist so each row knows whether it is on it). The
  row's button puts it on the selected playlist or takes it off; its menu
  lists every playlist, ticked by `playlists_of`, each a toggle. Songs arrive
  from the server's scanner (this build carries no `add_song`); alone or
  signed out, the library is empty and the screen says why.
- **Playlists** — the person's playlists in the order they were made
  (`playlists`); a `+` opens a dialog to name a new one (`create_playlist`
  trims and refuses a blank; a name the person already has is kept and
  numbered by the log, "Favorites (1)"). Tapping one selects it and opens it.
- **Playlist** — its contents in playlist order (`playlist`, which answers
  with the library's own rows). Swipe left, or the trash button, removes one.
- **Favorites.** The app makes a default "Favorites" only for somebody with
  no playlist at all, and only once it knows: alone at once; signed in after
  the link has been up two seconds, so the log has arrived; signed out, on
  the first add with no playlist to add to — not on opening, so that signing
  in later on a second device does not hand the person a "Favorites (1)".
- **Where every change stands.** Each change this phone makes is kept with
  its entry id, and `Session.statusOf` says where it is: under a playlist or
  an item, *not synced yet* while pending, nothing once confirmed, and *not
  saved:* followed by the server's own sentence when it was rejected. The
  newest rejection is also a snackbar, and Settings lists every change.
- **Settings** — the account; the server URL (`ws://…/sync`; `10.0.2.2`
  reaches the emulator's host) and **work alone**, which opens the session
  with no server so the phone sequences its own log; applying closes the
  session and reopens it. A status block prints the cursor, what is not
  synced, not saved and diverged, and a button asks the authority to verify.

Every mutation and every query goes through the session by name, and the
session runs the procedure **natively**: the replica holds
`module().procedures()` by the hash an entry names, so the optimistic
apply, the rebase's replay, and a peer alone sequencing its own entries all
run the Kotlin body directly. The interpreter runs only what arrives that
the phone has no procedure for:

```kotlin
session.mutate("add_to_playlist", mapOf("playlist_id" to Value.id(pid), "media_id" to Value.id(mid)))
session.query("playlist", mapOf("playlist_id" to Value.id(pid)))
```

The entry recorded is byte-identical to what the interpreter would record,
and `kotlin/ark-client`'s tests hold a native session and an interpreted
one to the same entries, changes and hash on the demo module.

The **new-playlist dialog** checks the name as it is typed with the form
validator — `Session.check("create_playlist", input)`, which runs
`create_playlist`'s own input checks (`trim`, `min(1).why("a playlist
needs a name")`, `max(120)`) — and shows the message under the field; the
button is enabled exactly when the mutation would accept the name. A query
the `owned` middleware refuses (a playlist that is not this person's) draws
as an empty list.

`Phone.kt` is the domain as the screens see it, with no Android in it:
the rows as data classes, each query and mutation by name, the default
playlist's rule, and the caption a standing earns. `Model.kt` is one
`AndroidViewModel` around a `Session`: it pumps it every 50 ms from
`viewModelScope` (so every call into the session is on the main thread, the
session's contract), re-reads the lists on the session's change listener,
re-reads where each change stands on every pump, and turns refusals and
rejections into a snackbar. State lives under
`filesDir/ark/<server-…|alone>/` — confirmed store, cursor, pending
intents, verdicts, and, alone, the log the phone sequenced — written to a
temp file and renamed.

## Not here yet

- Sign-in: dev auth only; the token on the socket is the user's name and
  the login is `dev`. OpenID Connect (what `ark-auth` does for the desktop
  and the page) has no phone flow here yet.
- Playback: a track's `file` is known, not played.
- Albums, artists, composers and works: the domain has them; the phone
  carries only the procedures its four screens call.
- Adopting a phone's own log into a server it later meets (§3.10) is not in
  the client machine; switching from alone to a server starts a fresh
  replica directory.
- The server's `Agree` after **Verify** arrives on a later pump and is not
  yet drawn; alone, the answer is immediate and shown.
