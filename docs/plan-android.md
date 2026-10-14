# The Android app, as a product

`docs/plan-phones.md` put the Kotlin client on the phone and rewired the
app to it; what it rewired was an engineer's app — a settings screen of
counters, an alone switch, dev sign-in by typing a name. This is the
design that makes it harken. Android only for now; iOS mirrors it later.

## Decided by the owner

- **harken is never used without a server.** The demo web UI is the one
  server-less client. The Android app has no alone mode: no switch, no
  path that opens a replica with no server behind it.
- **Onboarding is shared, and is three steps**: the server's address,
  signing in through the server's OAuth in a browser tab, then the
  permissions the app needs — for harken, only notifications, which is
  what lets the player show its notification and lock-screen controls.
  It remembers the server and the person and never asks again; signing
  out, or the server refusing the session, brings back only the steps
  that are needed.
- **The debug information is out of the way.** Nothing a person sees in
  ordinary use is a cursor, a hash or a count of pending intents.
- **Home, Search, Library and the player are designed**, not listed.

## The shared onboarding: `kotlin/ark-android`

An Android library module beside `ark-runtime` and `ark-client`, package
`dev.arkdb.android`, that any Ark app on Android uses — harken is its
first caller and names only what is harken's:

```kotlin
ArkApp(
    identity = AppIdentity(name = "Harken", scheme = "harken", mark = painterResource(R.drawable.mark)),
    permissions = listOf(Permission.Notifications(why = "Show what's playing and its controls")),
) { session -> HarkenScreens(session) }
```

- **Server.** One field. Accepts `host`, `host:port`, or a URL; tries
  `https` and, for a private or loopback address, `http`; asks the
  server's `/healthz` and goes on only when the answer is an Ark server
  serving this app's module. Errors say which of those failed, in a
  sentence.
- **Sign in.** Opens the server's `/auth/login?redirect=<scheme>://auth`
  in a Custom Tab; the server is the OAuth client (`ark-auth`), so the
  phone holds no secret and knows no provider, and a dev server's own
  sign-in page works the same way. `<scheme>://auth?code=…` comes back to
  the activity, the code is exchanged at `/auth/exchange`, and the login —
  user, session, token, roles — is remembered.
- **Permissions.** One screen per permission the app declares, each with
  the app's reason in a sentence, "Allow" and "Not now". A permission the
  platform does not have (notifications below Android 13) is skipped.
  Asked once; never again unless the person goes to settings.
- **Remembered.** Server and login in the app's private storage; on
  launch with both, straight into the app. A `Denied` from the server
  (revoked, expired) returns to *Sign in* with the server kept and the
  replica and its pending intents kept, so signing in as the same person
  pushes them. *Sign out* forgets the login, keeps the server.
- **The session** it hands the app is the Kotlin `Peer` opened over that
  server, pumped on its own thread, with the status a screen may ask for
  (`linked`, `pending > 0`) and nothing else by default.

## harken on Android

- **Home**: what was played last and a way back to it; recently added;
  the person's playlists; albums — shelves of covers, each opening its
  page.
- **Search**: one field over the library's `search` (the text index), as
  you type, results grouped as songs, albums, artists; recent searches.
- **Library**: Playlists, Albums, Artists, Songs, and the pages each
  opens (a playlist, an album with its track order and sections, an
  artist with their albums); create a playlist, add to and remove from
  one.
- **The player**: Media3's ExoPlayer in a `MediaSessionService`, streaming
  `/media/…` from the server — the notification, the lock screen and
  Bluetooth controls are the media session's. A mini player above the
  tab bar; a full-screen player it grows into, with the cover, the
  title, a scrubber, previous / play-pause / next, shuffle, repeat and
  the queue.
- **Settings**: the account, the server, sign out, about. Diagnostics —
  every number the old settings screen had — behind *About*: tap the
  version seven times, as Android's own build number does.
- **Look**: harken's palette — the desktop's neutrals and its gold — in a
  light and a dark theme, the same type scale throughout.

## Proving it without a phone

No phone or emulator runs here, so the screens are rendered on the JVM
(Paparazzi, or Roborazzi if Paparazzi cannot be built offline) in light
and dark from a fixed demo library, and the PNGs are a nix output
(`packages.harken-android-screens`) — what the owner looks at, and what a
change to a screen is reviewed by. The onboarding's state machine, the
address handling and the remembered store are unit tests.

## Landed

All of it, in five commits (`git log -- kotlin/ark-android harken/android`):
the shared onboarding; harken without an alone mode; the player, the
screens and the look together (each needed the others to compile); the
screens as images; and the recorded graph with these docs. What each
screen is, what was added and why, what was broken on purpose to see a
test fail, and what nobody has seen yet.

### `kotlin/ark-android`

`ArkApp(identity, permissions) { session -> … }`, as above, with two
changes to the sketch: `AppIdentity` also carries the app's two colour
schemes, its type and its module (the onboarding's health check compares
the module's hash, and the session interprets it), and `Permission` is an
open class rather than a sealed one, so the next app's need is its own
declaration. Under the screens is a pure machine — `Onboarding.next(step,
event)` returning the step and the effects — that `ArkHost`, a
`ViewModel`, runs. The intent filter for `<scheme>://auth` is the app's
manifest's, on a `singleTask` activity; `ArkApp` takes the code from the
launch intent and from every new intent (`addOnNewIntentListener`), and a
return to the activity with no code puts the button back. `ArkSession` is
the peer on its own thread with `status`, `post`, `run`, `onPump` and
`signOut`. Documented in `kotlin/README.md`.

### harken's screens

- **Onboarding** (the module's, in harken's identity): the server field
  with its sentence under it — unreachable, not an Ark server, an Ark
  server for another app; signing in, waiting for the browser,
  exchanging, or back after a denial with the server's reason; the
  notification permission with harken's reason.
- **Home**: the last played track and where from, with a button that plays
  that list again from it; recently added; your playlists; albums. Empty,
  it says where music comes from; before the first read, that it is
  opening; unreachable, a sentence over it.
- **Search**: as you type, songs from the module's `search`, albums and
  artists by name; recent searches when empty; "nothing matches" when
  nothing does.
- **Library**: Playlists (with "New playlist"), Albums (a grid), Artists,
  Songs — each with its own empty sentence.
- **Album / artist / playlist pages**: header with cover, kind, name, who
  by (a link to the artist), the counts and length, Play and Shuffle; an
  album in sections by part with release track numbers; an artist's albums
  shelf then songs; a playlist that is empty or gone says so.
- **A song's menu, adding to a playlist, a new playlist** (validated as
  typed by `create_playlist`'s checks, the module's sentence inline).
- **The player**: the mini player above the tab bar, the full player, the
  queue sheet.
- **Settings** and **diagnostics** (seven taps on the version; listed
  under About once found).

### Dependencies added, and why

- `androidx.browser:browser:1.8.0` — the Custom Tab the sign-in opens in.
- `androidx.media3:media3-exoplayer:1.5.1`, `media3-session:1.5.1` — the
  player, and the media session that is the notification, the lock screen
  and Bluetooth. 1.5.1, of the same months as the rest of this graph
  (AGP 8.7, the Compose BOM of 2024.12), whose `compileSdk 35` it fits.
- `app.cash.paparazzi` 1.3.5 (test only), and through it layoutlib 14 —
  the screenshots. 1.3.5 because it is the release built against Kotlin
  2.0.21, which this build uses.
- `junit:junit:4.13.2` (test only) — the onboarding's and the app's JVM
  tests; Paparazzi runs under it.
- No image library: covers are `HttpURLConnection` and `BitmapFactory`.
- `material-icons-core` was dropped: every glyph is the desktop's Lucide
  path.

### Falsified

Each test was broken on purpose once and seen to fail:

- `OnboardingTests`: a denial that did not forget the dead login —
  `deniedWhileRunningGoesBackToSignInKeepingTheServer` failed.
- `ServerTests`: every host taken as local (`addresses`, `probing`
  failed); any Ark server accepted whatever its module (`health`,
  `probing`); `markAsked` overwriting the others (`remembered`); an https
  answer that refused retried in the clear (`probing`).
- `MediaTests`: segments not percent-encoded; the square's hash walked
  over code points instead of UTF-16 units (the emoji pin failed);
  `sampleSize` shrinking below what is wanted; a clock with no hours.
- `Screens`: a mini player that throws — the seven tests that draw it
  failed, and `recordPaparazziDebug` with them.

### What ran green here

- `nix build .#checks.x86_64-linux.kotlin` — ktfmt over the runtime, the
  client, `ark-android` and `harken-peer`; 122 of 130 vectors; the trace
  corpus; `KeptNumbers`, `StoreHints`, `ClientTests`, `StorageTests`.
  The runtime and the client did not change.
- `nix build .#harken-apk` — the onboarding's 11 JVM tests, then
  `assembleDebug`: `result/harken-debug.apk`, 13,748,869 bytes, debug-signed.
- `nix build .#harken-android-screens` — 80 PNGs (40 screens and states,
  light and dark, 1080×2340) and `index.html`, all looked at.
- On a laptop gradle (the flake's SDK and JDK): `MediaTests` (4) and the
  same `Screens` rendering.
- The recording (`harken-apk-deps`) ran its three tasks green against the
  network. Its script needs a nix daemon to reach (it runs `nix-shell`
  inside bubblewrap), and `nix run` here wants a `bin/`, so it was run as
  the store path it builds. Building from the new graph met Maven
  Central's 429 on one artifact (`paparazzi-gradle-plugin-1.3.5.jar`)
  through every retry; it was put in the store from Google's mirror of
  Maven Central, under the same name, after checking its SHA-256 against
  `deps.json` — the store path of a flat fixed output is its name and its
  hash, so that is the same path the fetch would have made.

### Not verified

No phone or emulator has run any of this. In particular: that the Custom
Tab's redirect reaches the `singleTask` activity on a real device (and
that a server with a real provider, not the dev form, sends it there);
that Media3 shows its notification and lock-screen controls and answers
Bluetooth; that `POST_NOTIFICATIONS` is asked at the right moment on
Android 13+ and skipped below; streaming and seeking over `/media` from a
real server; covers from a real server (the screenshots draw only the
derived squares); how the full player animates in and out (since drawn
and tested, "Motion" below, but not felt); the
diagnostics' numbers against a live peer. The screenshots are layoutlib's
rendering of the composables — faithful to layout, type and colour, not to
a device's insets, fonts or animation — and the sheets and the dialog are
framed over a scrim there rather than raised as windows.

### The launch: a remembered session opens on Home, populated

What the owner saw was two loading pages on every launch. They were the
onboarding's "Opening your library…" while `ArkHost` decoded and verified
`harken.ark` and `ArkSession.open` loaded the replica and replayed what was
pending. Then Home's `Loading()` until `Library` had hydrated five maintained
views through the interpreter, `track_details` over every track. Nothing on
that path waits for the network. All of it is local CPU, done one stage after
another after the activity starts. `ColdOpen` (`kotlin/harken-peer/src/test`)
measures it over replicas the Kotlin client writes. Every song is `add_song`
authored by the library account and confirmed by an `Authority`, the way the
server confirms the scanner's entries. On HotSpot on this machine, before
anything changed, launch → Home's lists was 435 / 852 / 3,190 ms at 200 /
2,000 / 20,000 songs.

What a launch does now:

1. The system splash (`holdSplash`, `androidx.core:core-splashscreen`)
   stays up until `ArkHost.ready`, with a ceiling of 1.5 s.
2. `ArkSession.open` returns at once. The peer opens as the first task on
   its own thread: the module, verified only if these bytes have not passed
   before (`ModuleMemory`), then the replica. A table's digest and its text
   indexes are no longer built at load. They are built the first time a hash
   or a search asks.
3. Meanwhile, on a thread of its own, Home's three **launch views** (below)
   are read from disk and published as provisional, and the splash goes.
   Home is populated before the replica has been read at all.
4. The peer opens. The live views of `library`, `playlists` and `albums`
   answer against what was shown. Equal means nothing is published.
   Different means the difference is spliced in. Then, on a later turn of the
   thread, `artists` and `track_details` are hydrated. Library's Artists tab
   draws nothing until they arrive, not "no artists".
5. Past the splash's ceiling, the app shows whatever it has: Home's shell,
   whose lists fill in. It never shows a loading page. The onboarding's
   opening page (`Step.Opening`) is drawn only right after signing in, while
   the server's snapshot is on its way. It says "Getting your library from
   <server>…" and appears only after 600 ms. A remembered login never reaches
   that step: `OnboardingTests.aRememberedLaunchNeverOpens`.

Measured with `ColdOpen`: a fresh HotSpot JVM per launch, the median of 3, on
this machine, with other builds sharing it. These are **HotSpot numbers, not
ART on a phone**. Assume a phone is two to five times slower at this
allocation-heavy work. Nothing here has been timed on a device, and neither
Paparazzi nor layoutlib gives a closer number: they run on the same HotSpot.

| ms, from `main`                                 |   200 | 2,000 | 20,000 |
|-------------------------------------------------|------:|------:|-------:|
| before (4b43b51): Home's lists                  |   446 |   839 |  2,863 |
| before: everything hydrated                     |   497 |   928 |  3,634 |
| without launch views (lazy digests, Home first) |  506* |   682 |  2,031 |
| **with launch views: Home's lists**             |  **74** | **100** | **220** |
| with launch views: live, nothing published      |   369 |   613 |  2,183 |
| of which `Peer.open`                            |    75 |   179 |    953 |
| floor: Home's lists off disk, no engine         |    43 |    66 |    170 |
| a JVM's start before `main` (empty main)        |    92 |    91 |     95 |

\* A noisy run. In an earlier set, the same row read 361 / 698 / 2,032.

The floor is one file holding exactly Home's three lists as plain CBOR,
read and decoded with no engine at all: the physical floor for "data on
Home". A launch view's read is within 30–50 ms of it, which pays for the
binding header and the checksum. The JVM's own start is about 95 ms before
any of our code runs. A launch view's write at 20,000 songs is a 3.6 MB
file, about 120 ms with the fsync (1 / 28 / 117 ms for the library at the
three sizes). It runs off the UI thread and off the peer's, a second after
the last change and when the app goes to the background.

Candidates measured and not taken: decoding the module (~150 ms cold) in
parallel with decoding the replica's CBOR (~150 ms at 20,000). It would save
at most that much of launch → live, which no longer gates the first frame.
Remembering the closures' hashes beside the verified module's SHA-256
(~100 ms cold, ~12 ms warm) for the same reason. The verifier's checks, which
`ModuleMemory` now skips, turned out to be the small part of the module's
cost (tens of milliseconds cold). Decode, prepare and hashing are most of it.

Not verified on a device: the splash's hand-off (the keep-on-screen
condition polled per frame), how the 1.5 s ceiling feels, the launch-view
write in `onStop` when the system kills the process soon after, and every
number above on ART.

### Launch views

An engine feature (`kotlin/ark-client`, `LaunchView.kt`). An app opens a
query with a stable key: `session.launchView(key, query, args)
{ rows -> … }` in `ark-android`, over `LaunchViews` and `LaunchView` in the
client. The view's first value may come from disk and is then
**provisional** (Zero's resultType `unknown`). It stops being provisional
when the live view has hydrated (`Library.provisional`). harken declares
`library`, `playlists` and `albums` as launch views and draws provisional and
live rows the same.

**What is saved**, per (server directory, account, key): one file beside the
replica (`<server>/launch/<account hash>/<key hash>.view`). It has a
versioned header, the view's output rows in canonical CBOR, and a SHA-256 of
everything before it. The header binds:

- the module: the SHA-256 of its bytes. They are its closures and its
  schema, and they are known before anything is decoded.
- the view: the SHA-256 of the canonical (query, arguments).
- the account: the user id, and a digest of the roles the login holds.
- the position it reflects: the confirmed sequence, the last pending
  intent's id, and a digest of the scope union (`partial`).

Each write goes to a temporary file in the same directory, is fsynced, and is
renamed over the old one, so a reader never sees half a file. The files are
deleted with the login, on signing out or a denial. They hold nothing the
replica does not.

**When it is written**: off the UI thread, a second after the last change to
one of the views, and immediately when the activity stops (`ON_STOP` →
`ArkHost.stopped`), the write that matters most. The rows are taken on the
peer's thread (a reference, no copy) and encoded and written on the launch
views' own thread. Nothing is written on the launch path before the first
publish.

**At launch**: before `Peer.open`, the file is read and checked. It is
rejected unless the version, module, view and account all match and the
checksum verifies. A rejected, torn or corrupt file is deleted and ignored,
and the launch takes the ordinary path under the splash's ceiling. It never
throws. A valid file is published as provisional and the splash goes. Then
the peer opens and the live view hydrates, and its first result is compared
with what was published. Equal publishes nothing: no recomposition, scroll
untouched. Different publishes the difference as patches against the
provisional list (`LaunchViews.diff`: the common head and tail kept, the
middle updated in place, the rest inserted or removed), so the scroll
survives. The live view always wins. The header's position is compared with
the opened replica's, and a mismatch is counted (`positionMismatches`). It
can only lead to that correction.

**Why it is right**: nothing but this process writes the replica, so it
cannot change while the app is closed. A file written at the replica's last
position holds what hydrating the same query over the same module and login
produces. A crash between a store write and the file's write, or a file from
before an update, costs one correction. It never keeps a wrong list.

**Acting on a provisional row**: playing one starts at once, since it needs
only the row's file. A change (making a playlist, adding to one) is posted to
the peer's thread like any other. That thread is busy opening, so the change
runs after the open, against the real replica, and the mutation's own checks
refuse a row that is gone. There is no queue of our own.

Tests, each falsified once:

- `LaunchViewTests` (ark-client, in `checks.kotlin`): after a clean close
  every file equals the hydrated list, with intents pending, after a
  confirmed batch from another device rebases a pending one, and under
  other roles. A stale file is corrected by patches that take it to the live
  list, one Insert for one playlist, and the position mismatch is counted.
  A file bound to another module, view or account is not read and is
  deleted. Truncated, a byte flipped, or empty: not read, deleted, nothing
  thrown. `diff` takes any list to any other.
  - Falsified: the checksum skipped ("a byte flipped: not read"); `live`
    always `Unchanged` ("stale: patched"); the account not checked
    ("account: a file bound elsewhere is not read").
- `LaunchTests.aLaunchViewIsHomeBeforeThePeerOpens` (the app): the second
  launch, with the peer's open held shut, publishes Home's three lists from
  the files (provisional, the splash released). When the peer opens it
  publishes nothing more for an equal library. A playlist made before the
  open is made once, after it, and is one pending intent.
  - Falsified: publishing an equal live result (7 publishes, not 4);
    dropping posts made before the open (the launch never hydrates).

**For Rust's `ark-client` and Swift to match**: the file format above,
byte for byte (magic `ARKLV`, version 1, big-endian lengths, a canonical-CBOR
header with fields `module`, `view`, `account`, `roles`, `cursor`, `pending`,
`partial`, the rows as one canonical-CBOR list, a SHA-256 trailer), so a file
is portable and a vector can pin it. Also the binding rules, the
equal-means-silent and patch-on-difference contract, `diff`'s shape, and the
deletion on sign-out. Neither has launch views yet. Rust's peer and Swift's
would add them beside their maintained views. A conformance vector (a file,
a replica, and the expected `Unchanged` or patches) is the natural way to
hold all three to one behaviour.

### The scroll fix

While a track played, every visible row recomposed on each of the player's
half-second position ticks: `act`, `controls` and `albumOf` were new objects
on every composition. They are remembered per session, `HomeData` and
`LibraryData` are `@Immutable`, and one album's rows share one cover load.
`ScrollTests` drives the real `HarkenApp` over `ArkSession.inline` on
Paparazzi's frame clock. While the position ticks four times, the mini player
recomposes for each tick and no row recomposes at all.

Not done: a release variant. The recorded Maven graph holds `profileinstaller`
but not `com.android.tools:r8`, so `assembleRelease` with minification needs a
re-recording with `:app:assembleRelease` among the recorded tasks. A
generated baseline profile needs a device.

### A fast fling

With the ticks gone, the Songs list was still choppy when flung hard. A fling
brings several rows on screen in a frame, so what one entering row costs is
what matters. `FlingTests.aFastFlingThroughEverySong` measures it, run when
`HARKEN_FLING_BENCH` is set. It drives the real `HarkenApp` over 2,000 songs
in 200 albums, each with its cover decoded, at three rows a frame. Each pass
starts from the top: four warm the JIT and the covers, and two are reported.
The cost is this thread's CPU time and HotSpot's byte count, for the compose
and measure a scroll does and for one software draw of each frame. These are
HotSpot and layoutlib numbers, a relative proxy. They are not frame times on
a phone.

| after                              | compose+measure, per entering row | draw, per frame (≈13 rows) |
|------------------------------------|-----------------------------------|----------------------------|
| before                             | 424 µs, 40.0 KB                   | 7.5 ms, 36.7 KB            |
| `albumOf` by name                  | (within noise), 40.0 KB           | 7.5 ms, 36.7 KB            |
| `Id.hex` kept                      | (within noise), 38.7 KB           | 7.3 ms, 36.7 KB            |
| `Cover` launches only to load; `Square` draws from a cache | 400 µs, 37.9 KB | 7.5 ms, 31.0 KB |
| `MoreButton`; `clock` by hand      | 370 µs, 34.9 KB                   | 7.2 ms, 27.7 KB            |
| one painter for the menu glyph     | **386 µs, 32.7 KB**               | **7.1 ms, 27.7 KB**        |

The bytes repeat to within 0.5% from run to run. The times are medians, and
they move by about 10% with the machine's load, so a single step's time is
not to be read on its own. In the profile of the compose and measure,
`String.format` in `Media.clock` was 4% and is gone. `IconButton` was 9%, and
`MoreButton` in its place is 4%. The scan of every album was 1.9% at 200
albums, and the map lookup is about half of that. `Id.hex` went from 1.2% to
0.4%. What remains is Compose's own: applying an entering row's nodes,
placing it, and laying out its three lines of text. No `contentType` was
added: every row of the list is one type, so the default already reuses
every row that leaves for the next that enters.

- `RowTests.theAlbumByNameIsTheAlbumTheScanFound`: over a seeded library of
  49 songs and 7 albums, the album by name equals the old scan's for every
  song. It also does with an album's row missing, and is none for a song on
  no album.
  - Falsified: no made-up album when the row is missing.
- `RowTests.anIdsHexIsSpeltOnce`: the same spelling, and the same string.
  - Falsified: the cache removed.
- `FlingTests.aCoverAlreadyDecodedLaunchesNothing`: five covers already in
  memory launch nothing, and three only on disk launch one each and share
  one read.
  - Falsified: launching for every cover (8, not 3).
- `ScrollTests` still composes nothing per tick.
  - Falsified: `albumOf` remade per tick.

### The release build

The rest is the build type, and on a phone it is likely the larger part: the
APK was `assembleDebug`, debuggable, with no profile installed. ART runs a
debuggable app with its compiler held back for a debugger, so the JVM
numbers above do not show it. `harken-apk` now runs `assembleRelease` and
ships `result/harken.apk`, which apk.yml uploads as `harken-apk`:

- not debuggable: neither `aapt2 dump badging` nor the manifest says
  `debuggable`, and fdroid's "debuggable or testOnly set in
  AndroidManifest.xml", which the debug APK raises, is gone from
  `harken-fdroid-repo`;
- with Compose's own baseline profiles, which AGP compiles into
  `assets/dexopt/baseline.prof` (7,845 bytes) and `baseline.profm` for a
  variant that is not debuggable. profileinstaller 1.3.1, already in the
  graph through Compose and now named, merges its `ProfileInstallReceiver`
  and initializer into the manifest;
- signed with the same committed key, certificate SHA-256
  `1a8859fb49cfacceb8984f7a70ac3993f9eda207c1fe2d72fe6627b8df431139`, as
  `apksigner` reports for both the debug APK and this one. It installs over
  what the F-Droid repository and a hand install already put on a phone;
- version code `self.lastModified / 60` as before: 29866923 for the tree
  this was built from, above the live 29866861;
- 10,418,060 bytes, where the debug APK was 13,824,048.

It needed nothing the recorded graph lacks. Minification is off: R8
(`com.android.tools:r8`) is not in `deps.json`. Turning it on means
`isMinifyEnabled = true`, keep rules for what the app reaches by
reflection (the media session service and its receiver are in the
manifest; Compose and Media3 ship their own rules), and a re-recording with
`harken-apk-deps`, whose task list now includes `assembleRelease`. Lint's
release check is off for the same reason: it would fetch lint. The debug
build stays what a laptop builds and what the screenshots and unit tests run
against.

Not verified: the scroll on a phone. Nothing here can run an APK, and every
number above is the JVM's.


### Motion

What the owner asked for, in the owner's words: a back swipe that shows the
page it goes back to; no fade; a tap on an album's or a playlist's art that
turns into its page, the way Pinterest moves between pages rather than
sliding in from the side; and the player's bar swiped up and down the same
way.

**Pages** (`Motion.kt`). Every move of the NavHost says its transitions, all
four of them `None`, so its own cross-fade is gone everywhere. What moves is
drawn by the page (`Moving`, around every destination):

- A page opened from art grows out of it. The tapped cover notes where it is
  (`Hero`, a node that keeps its coordinates and nothing else), and the page
  is drawn scaled, offset and clipped so that its header's art sits exactly
  on the tapped cover, then opens out to the screen, its corner going from
  the cover's to none. The page it came from stays and dims to 25%. Going
  back runs the same move the other way, into the same cover: an origin is
  kept per back stack entry. A page scrolled past its header shrinks its top
  square into the cover instead.
- A page opened from no art (an artist from a song's menu, an album's
  artist, settings) grows from 80% of the screen at its middle, fading in
  over the first third.
- The tabs switch at once: no animation, and the tab leaving is not drawn
  for the frames it is still composed.
- The move is one number per destination, `animateFloat` on the NavHost
  entry's own transition, so whatever seeks that transition moves the page:
  navigation-compose 2.8.5's predictive back seeks it with the edge swipe's
  progress, and `enableOnBackInvokedCallback` in the manifest turns that on.
  Letting go past the system's threshold finishes it; cancelling runs it
  back.
- A page pulled down from its top goes back the same way (`Pull`): the pull
  is dispatched to the activity's `OnBackPressedDispatcher` as a back
  gesture — `dispatchOnBackStarted`, `…Progressed`, then `onBackPressed` or
  `dispatchOnBackCancelled`, all public in activity 1.9.3 — so the drag and
  the edge swipe are one transition. It engages only through nested scroll's
  post-scroll, which is what the list has left over at its top, only from a
  finger (never a fling arriving at the top), and only for a drag at least
  twice as vertical as it is horizontal. A pull past a fifth of the way, or
  let go faster than 1.2 page heights a second, goes back; short of that it
  springs back.
- 400 ms, on a curve close to linear for its first third (so the gesture
  moves the page as soon as the finger does) that then settles.

It is not `SharedTransitionLayout`. Wrapping the NavHost in one, with no
shared element anywhere, cost the Songs list 43% per entering row in the
fling bench (525 and 515 µs against 365 and 359, 36.5 KB against 32.8 KB):
a lookahead scope measures everything under it twice. The page is instead
laid out once at its full size and the move is a scale, an offset and a clip
applied as it is drawn, after layout, which is also the first moment its
header's place is known on the frame it appears. Only album, playlist and
artist art that opens a page carries a `Hero`; a song row's thumbnail does
not.

**The player** (`PlayerSheet.kt`). The bar and the full player are one card
and one number `p`, the offset of an `AnchoredDraggableState` (foundation
1.7.6's constructor, with positional and velocity thresholds and a decay).
The card's bounds go from the bar's to the screen's and its corner from the
bar's to none; the bar's face goes out by 20% of the way and the full face
comes in over the last 40%; the art, one `Cover` the card draws itself,
travels from the thumbnail to the big square, its corner from 4 to 12 dp;
the tab bar slides away under it and the page dims to 40%. A tap on the bar
animates along the same path. The anchored drag is on the card, which is
laid out at the card's size, so it touches only what it covers; the faces
inside it are laid out once and moved. A sideways drag on the bar skips —
left the next track, right the one before — and leaves `p` alone, split
from the vertical drag by each detector's own axis. (There was no skip
swipe in the Kotlin app before; this is it.) Collapsed, the full face is not
composed at all, so it takes no touch and no tick, and the bar's face is not
placed past 20%, so its buttons cannot be pressed through the full player.
The edge's back gesture takes the full player down under the finger
(`PredictiveBackHandler`, progress onto `p`). This is the Expo player's
shape: one shared value, two faces, the tab bar under it.

`p` and each page's number are read in layout and drawing, never in
composition, except where they flip (whether the full face exists, whether
the bar is at rest): the card moving composes nothing and the page behind
it is not composed again.

The bar's static layout did not move: the card draws it now, rather than the
bottom bar, on the same pixels. The 30 screenshots
with a player (Home full and offline, every Search state, each Library tab,
both albums, the artist, the playlist) differ from the last build's at the
bar's shadow and corners only, by at most 8 of 255, over about 6,000 pixels:
the shadow and the clip are one layer now, where `Surface` drew a background
under a separate shadow. The other 52 are byte for byte what they were. The
ten new ones are the player held at 0, 25, 50, 75 and 100% of the way up,
light and dark (`d2_player_rising`).

The fling bench (`FlingTests`, HARKEN_FLING_BENCH=2000, the reported two
passes), on this machine, with other work running:

| | compose+measure, per entering row | draw, per frame |
|---|---|---|
| before (cccf421) | 365 / 359 µs, 32,752 B | 6.9 / 7.0 ms, 27,707 B |
| `SharedTransitionLayout` around the NavHost, nothing else | 525 / 515 µs, 36,532 B | 7.1 / 7.4 ms, 27,705 B |
| after, first run | 379 / 361 µs, 32,951 B | 7.1 / 6.9 ms, 27,977 B |
| after, final | 356 / 354 µs, 33,229 B | 7.1 / 7.0 ms, 27,984 B |

About 1.5% more bytes per entering row and 1% per frame, from the Library
tab's destination wrapper (a background and a draw block); the times are
within the bench's noise. `ScrollTests` still composes nothing per position
tick and nothing at rest.

Tests (`MotionTests`, the real `HarkenApp` over a peer holding the demo's
songs, and `Chrome` for the player, on Paparazzi's frame clock with real
`MotionEvent`s), each falsified once:

- `aPageGrowsOutOfItsCardAndTheBackGestureSeeksIt`: a tap on Home's
  Brandenburg card opens the album through in-between frames; back progress
  0.3 and 0.6 draw it part-way and further; a cancel puts it back at 1 and
  keeps it; a finished gesture goes back to Home.
  - Falsified: the page's animation snapped for anything but a grow —
    "seeked at 0.3: 0.0".
- `aPagePulledDownFromItsTopGoesBack`: scrolled down, a drag down scrolls the
  list and pulls nothing; from the top, a pull shrinks the page and goes
  back; a short pull moves it and springs back.
  - Falsified: the pull always cancelled — "and far enough goes back".
- `aTabArrivesWithNoFrameInBetween`: Home to Library draws no frame mid-move;
  opening an album draws some.
  - Falsified: a tab grown like a page — 24 frames drawn mid-move.
- `theBarFollowsTheFingerAndSettlesToTheNearerEnd`: held at 0.29 and let go,
  back to 0; at 0.69, up to 1; dragged down on the full player, back to 0.
  - Falsified: a positional threshold of a tenth — "short of half, back to
    the bar", was 1.
- `aSidewaysDragOnTheBarSkipsAndStaysDown`: left skips ahead once, right back
  once, `p` stays 0.
  - Falsified: the sideways drag disabled — no skip.
- `theCollapsedPlayerTakesNoTouch`: open, the full player's menu answers;
  collapsed, a tap there reaches the page, a tap on the bar opens it, and
  the full face is not in the tree.
  - Falsified: the full face always composed and placed — the bar's tap no
    longer reached the bar (and the tap-to-open and skip tests failed with
    it: an invisible face over the bar swallowed both).
- `theBarOpensOnATapAlongTheSamePath`: `p` through the in-between to 1.

With HARKEN_MOTION_FRAMES set to a directory, these tests write the frames:
a page opening at 0, 25, 50, 75 and 100% open, the back gesture at 0.3 and
0.6, a page halfway back into its card, the player halfway up. All were
looked at.

Not verified: how any of it feels on a phone, and real frame times — every
number above is HotSpot's and layoutlib's. The in-app preview of predictive
back needs Android 14 or later with the gesture navigation; below that the
back button and the pull run the same move without the preview, which is
untested on a device. The player's back gesture over a page that is itself
mid-move, a rotation mid-move, and a page reopened after the process was
killed (it grows from the middle: origins are not saved) are reasoned, not
tested.
