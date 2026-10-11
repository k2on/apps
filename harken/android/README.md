# harken for Android

Compose over the Kotlin client at spec v4 (`kotlin/ark-runtime`,
`kotlin/ark-client`, `kotlin/README.md`). The phone is a **client**: it
loads `harken.ark` — the module the domain crate writes, the one the server,
the desktop and the browser run — from its own assets, verifies it, and
interprets every function of it. Nothing of the domain is written in Kotlin,
and nothing is generated into the app: what the screens call is a mutation
or a query of the module, by name (`docs/plan-phones.md`).

**Assembled, not run.** `nix build .#harken-apk` compiles every Kotlin file
here and produces the APK, so the Compose code is what the compiler accepts;
nothing in the container this was written in can install or run it, and no
phone has run it. What *has* been exercised is everything under it, on the
JVM: the runtime and the client pass every client vector of `spec/vectors`
and every trace of `packages.trace-corpus` (`checks.kotlin`); the client's
link, sign-in and maintained views have tests of their own
(`ClientTests`); and `harken-peer-kt`, a headless peer on the same client,
runs in the process fleet beside Rust peers — partitions, restarts, a lost
log, alone and joining, old and new servers, a scoped module
(`checks.fleet-kotlin`, `docs/plan-fleet.md`). None of it has run on
Android's runtime. Expect the first run on a real phone to want small fixes.

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

Moving the app onto the v4 client did not move it: the client's one
dependency, OkHttp, was already in the graph, and the Kotlin build's
`harken-peer` uses gradle's own `application` plugin. Maven Central rate-
limits a cold fetch of the graph (HTTP 429); a build that fails on it is
retried after a pause, and the artifacts already fetched stay in the store.

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
    adb install app/build/outputs/apk/debug/app-debug.apk

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
keeps its own settings, plugin versions and toolchain.

- The Kotlin build sets `jvmToolchain(21)`, so the libraries arrive as Java
  21 class files; AGP 8.7's D8 accepts them, and the app is built for 21 too.
- The composite needs the same Kotlin plugin version on both sides; both say
  2.0.21.
- The client is written for API 26, the app's `minSdk`: `java.nio.file` only
  where 26 has it (a directory synced after a rename), no `java.net.http`;
  its HTTP is `HttpURLConnection` and its WebSocket OkHttp's.

## What it does

`Model` holds one `dev.arkdb.client.Peer` on a thread of its own: opened
over a `DirStorage` in a directory per server (or alone), not per person —
the log is the server's and every person reads it — linked to the server's
`/sync`, pumped every fifty milliseconds. Everything that touches the peer
runs on that thread; the screens read `StateFlow`s.

**The lists are spliced, not re-read.** `library`, `playlists` and the open
`playlist` are maintained views (`Peer.view`, `ClientView`): hydrated once,
then handed every `Changes` the peer reports — a rebase's included — and the
patches each answers with are spliced into the list the screen holds
(`ClientView.splice`). Only a `Reset` reads one again: the optimistic store
replaced (opening over a snapshot), another signer, or a refusal of the
view's middleware appearing or clearing. Changing the selected playlist
re-opens the two views read against it, which is a hydrate.

Four screens under one `Scaffold`, with a status line in the top bar
(`signed out`, `linked`, `alone`, `turned away: …`, the link in a word —
`connecting`, `waiting`, `joining` — and how many changes are not synced):

- **It works with nobody signed in.** The first launch opens the peer signed
  out: everything is authored as nobody, applied, written down, and nothing
  is dialled. Signing in makes all of it that person's and syncs it; the
  login is kept and the app opens as them on every launch. Signing out
  stops the link and forgets the token; whatever that person left pending
  stays theirs and goes when they sign in again.
- **Library** — everything, in the order it was added (`library`, read
  against the selected playlist so each row knows whether it is on it). The
  row's button puts it on the selected playlist or takes it off; its menu
  lists every playlist, ticked by `playlists_of`, each a toggle. Songs arrive
  from the server's scanner — `add_song` is the library's role's, not a
  phone's — so alone or signed out the library is empty, and the screen says
  why.
- **Playlists** — the person's playlists in the order they were made; a `+`
  names a new one, checked as it is typed by `create_playlist`'s own input
  checks (the form validator, `Peer.check`); a name the person already has
  is kept and numbered by the log, "Favorites (1)". Tapping one selects it
  and opens it. **There is no rename**: harken's module has no mutation for
  one, and the app invents nothing the module does not say.
- **Playlist** — its contents in playlist order (`playlist`, which answers
  with the library's own rows). Swipe left, or the trash button, removes one
  (`remove_from_playlist`).
- **Favorites.** Adding with no playlist at all makes "Favorites" first
  (`create_playlist`, then `add_to_playlist` on it).
- **Where every change stands.** Each change this phone makes is kept with
  its entry id, and `Peer.standing` says where it is: under a playlist or an
  item, *not synced yet* while pending, nothing once confirmed, and *not
  saved:* followed by the server's own sentence when it was rejected. The
  newest rejection is also a snackbar, and Settings lists every change.
- **Settings** — the account; the server (`http://host:port`, the socket
  `/sync` beside it; `10.0.2.2` reaches the emulator's host) and **work
  alone**, which opens the peer as its own authority over a local history;
  applying closes the peer, writes it down, and reopens it. A status block
  prints the cursor, what is not synced, what the server holds back (a
  module older than this phone's), what diverged, the link in a word,
  whether it is linked, how many connections it has had, whether the server
  runs a newer module, the last close and a denial; a button asks the
  authority to verify, and its answer is said when it arrives.

**Sign-in is dev auth only.** The app signs in the way `harken-peer` does
under a dev server: a name, `/auth/login` answered with a code and no page,
`/auth/exchange` for the login (`Auth.login` in the client). A server with a
real OpenID Connect provider answers with a page for a browser, and the app
says so instead of opening one: the client's `Auth.login` takes the
desktop's loopback flow (an `open` callback and a port the code comes back
to), but a phone wants a custom tab and a redirect it owns, which nothing
here has built or tried.

Every mutation and every query goes through the peer by name and is
interpreted:

```kotlin
peer.mutate("add_to_playlist", mapOf("playlist_id" to Value.id(pid), "media_id" to Value.id(mid)))
peer.view("playlist", mapOf("playlist_id" to Value.id(pid)))
```

What the phone records is byte-identical to what any other peer records for
the same intent, which is what the suite and the fleet hold the client to.
