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
- A page opened from no art (an album's artist, settings) grew from 80% of
  the screen at its middle, fading in over the first third. It rises now,
  and a song's menu grows its pages out of the song's row ("Motion,
  tightened", below).
- The tabs switch at once: no animation, and the tab leaving is not drawn
  for the frames it is still composed. (Each tab keeps its own stack now,
  and a switch is not a navigation at all: "Motion: tabs", below.)
- The move is one number per destination, `animateFloat` on the NavHost
  entry's own transition, so whatever seeks that transition moves the page:
  navigation-compose 2.8.5's predictive back seeks it with the edge swipe's
  progress, and `enableOnBackInvokedCallback` in the manifest turns that on.
  Letting go past the system's threshold finishes it; cancelling runs it
  back. (Now the gesture previews toward a limit, and the NavHost hears only
  that it began and how it ended: below.)
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
  moves the page as soon as the finger does) that then settles. (Springs
  now: below.)

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

### Motion, tightened

The owner tried f35cbec's release APK: "This fixed a lot of the performance
problems and the animations look good." Then six things, each answered
here.

**Search forgot where it was** (an album opened from scrolled-down results,
then back, landed at the top). The results were `remember`ed inside the
Search destination, so coming back composed it with none: for the 150 ms
debounce and the query the list held three items, its restored scroll
clamped to them, and stayed there when the rows came. The answer is now the
app's (`Searched`, one per session), with what it was asked of — the needle
and the identity of the three lists it filtered — so returning finds it on
its first frame and asks nothing again; a changed needle or library asks
again while the last answer stays on screen, with no frame of nothing in
between. Pages had the same bug, hidden from the tests by the inline session:
an album's rows were held while its page was composed and let go when a page
it opened covered it, so coming back started from an empty list while the
peer's thread hydrated a new view. They are held per back stack entry now
(a `ViewModel` on the entry, cleared when it is popped). Home and the
Library's tabs read the app's lists and were already right; every list keeps
`rememberLazyListState`'s saveable default.

- `MotionTests.theSearchKeepsItsPlaceAcrossAPage`: searched "Bach", scrolled
  to the end (14, 58), an album opened and back: (14, 58) on the first frame
  the list is there again, and after.
  - Falsified: the results `remember`ed in the destination again — back at
    the top, the results' last heading nowhere.
- `MotionTests.aPageKeepsItsPlaceAcrossAPageItOpened`: Water Music scrolled
  to (6, 102), its composer opened from a row's menu, back: (6, 102). Run
  over a session with a thread of its own (`ArkSession.over`), as a phone's.
  The menu is a window of its own, which layoutlib does not raise, so its
  entry is chosen through `HarkenApp(onMenu = …)`, which hands a test the
  `MenuActions` the sheet is given.
  - Falsified: the old hold-while-composed — (0, 0).

**Search results "do not show up until a split second after" a fast swipe.**
Not reproduced as missing rows. A probe (kept out of the tree) flung the real
`HarkenApp`'s results for "1" over 2,000 songs — 50 songs, an albums shelf
and an artists shelf, with the motion wrapper in place — four times, hard,
both ways, at about two rows a frame, and measured every frame's pixels row
by row: 382 frames, no row and no shelf drawn empty in any frame it was on
screen. Timed at three rows a frame, the frame a shelf enters costs about
twice a frame of rows (7–8 ms against 4 ms of compose and measure, on this
machine), and finishes inside it. The one thing in that list that is not
drawn in the frame its row enters is the cover: in the first fling through
fresh results, 12 of the 39 rows that entered went looking for theirs and
drew the derived square until it was read off the disk and decoded. On a
phone that is a real cover, hundreds of kilobytes of JPEG, and a fast fling
brings the row to the middle of the screen before it lands; a slow scroll
gives it the time. So the results start decoding every cover they draw the
moment they arrive (`Covers.warm`, at the size each is drawn), and a fling
finds them decoded. If what was seen was the rows themselves, the JVM cannot
show it: the keyboard is the other candidate (a focused search field leaving
the list stops its input, and the keyboard animates away over the rows it
covered), and only a phone can say.

- `FlingTests.searchResultsArriveWithTheirCoversDecoded`: a fling from the
  first result to the last sends 0 of 40 entering rows looking for a cover.
  - Falsified: no warm-up — 12.

**Springs, one set of numbers** (`Springs.kt`): stiffness 700, damping 0.88
for where a thing is, 1 (critically damped) for how large it is and what of
it shows, a rest threshold of 0.0005 of the way (Compose's default, 0.01,
left a last jump of several pixels on a page), a 300 ms cap on a settle.
Nothing in that file knows what moves.

- A page's size, clip, corner and dim run on the critically damped spring and
  its place on the 0.88 one (about 0.3% of overshoot, two pixels on a page);
  the 400 ms tween and `PAGE_MS` are gone. Opening the Brandenburg album from
  Home's card: the first moving frame 33 ms after the tap, within half a
  percent 283 ms later.
  - `aPageGrowsOutOfItsCardAndTheBackGesturePreviewsIt` holds 200–340 ms.
    Falsified with the old tween: 350.
- The player settles on the 0.88 spring from the release's own speed, capped
  at 300 ms, and never on a decay (`Springs.noDecay`: a draggable's settle
  then always takes the spring branch). Let go at 59% of the way closed,
  held still: on the bar in 216 ms; flicked from the same place: 133 ms. The
  old settle, critically damped at stiffness 400, took 700 ms for the same
  slow release. The sideways skip springs back on the same numbers.
  - `aSlowReleaseLandsWithinTheCapAndAFlickSooner`. Falsified with the old
    spec: 700 ms.
- Frame by frame, at 60 fps (ms from the release, or from the tap; `shown`
  is a page's size progress, `p` the player's):

  | ms  | page opening, `shown` | player, slow release, `p` | player, flicked, `p` |
  |-----|-----------------------|---------------------------|----------------------|
  | 0   | 1 (not yet drawn)     | 0.411                     | 0.411                |
  | 33  | 0.000                 | 0.317                     | 0.181                |
  | 66  | 0.218                 | 0.181                     | 0.065                |
  | 100 | 0.521                 | 0.083                     | 0.016                |
  | 133 | 0.741                 | 0.032                     | 0                    |
  | 166 | 0.866                 | 0.010                     |                      |
  | 216 | 0.954                 | 0                         |                      |
  | 283 | 0.990                 |                           |                      |
  | 316 | 0.995                 |                           |                      |

**The back gesture previews, then commits.** Its raw progress `r` is mapped
to a preview `v = L · (1 − e^(−r / L'))` with `L = 0.1` and `L' = 0.12`:
0.057 at r 0.1, 0.092 at 0.3, 0.098 at 0.5, 0.0999 at 0.9. The page sheds
`v` of its size about its middle (never below 0.9 scale), moves aside toward
where the finger goes by up to Android's own `width / 20 − 8 dp` (down, for a
pull), rounds its corners to up to 28 dp, and the page it goes back to shows
around it at the 25% dim. Let go past the threshold — the system's for the
edge swipe, the pull's own fifth for a pull — and the page goes the rest of
the way into its card on the size spring, from the preview (the preview fades
with the shrink, so it lands on the card exactly); short of it, it springs
back to 0. The player's back gesture is the same curve on `p`.

How: the NavHost's own handler would seek the transition with `r`, which is
the page following the finger all the way into its card. `PageBack` takes the
gesture on the activity's dispatcher instead and hands the NavHost a
dispatcher of its own (`Relayed`), on which it says only that a gesture began
— at progress 0, which is what composes the page underneath — and then that
it finished, or, once the preview has sprung back, that it was cancelled. The
NavHost's pop then runs the springs from there. `PageBack` is composed
before the player, so the player's handler, registered after it, is asked
first, and it stands down while the player is up whatever the order. The
pages' own content gets the activity's dispatcher back, so a pull reaches
`PageBack` like the edge swipe.

Found on the way: the player's back gesture never finished. Its handler ran
`close()` in its own coroutine; closing turns the sheet toward the bar, which
disables the handler, which cancels that coroutine, whose catch reopened the
player. It closes from the composition's scope now.

- `thePreviewSaturates`: 0.3 and 0.9 both within `L`, 0.9 less than 15%
  past 0.3, and free at first. Falsified with `L · r`: 0.09 against 0.03.
- `aPageGrowsOutOfItsCardAndTheBackGesturePreviewsIt`: 0.092 at 0.3, 0.0999
  at 0.9, Home composed under it, a cancel back to 0 keeping the page,
  released at 0.098 and closed all the way to 0.
  `aPagePulledDownFromItsTopGoesBack`: the pull previews (0.098) and goes
  back; a short one (0.046) springs back.
- `thePlayersBackGesturePreviewsAndSaturates`: 0.092 at 0.3, 0.0999 at 0.9,
  a cancel back to full, a finish to the bar. Falsified by the linear curve
  (0.09 at 0.9), and the finish failed before the fix above.

**A page grows out of what was acted on.** A song's menu now says where its
row was as it opened (`PageMotion.tapped`, read off the menu button's own
node — no node of its own on the row): "Go to album" grows the album out of
the row's cover, which is that album's art, exactly as a card's tap does;
"Go to artist" grows the page out of the row, a container whose rectangle the
top of the page opens out of. The bounds are the row's in the page behind,
taken as the menu opened; the sheet goes in the frame the page starts. A page
with no origin at all (settings, a page restored after the process was
killed, a deep link, the full player's menu) rises 24 dp and fades in over
about 120 ms on the same springs, with no 80% scale from the middle. No sheet
opens a playlist today — the playlists sheet ticks and unticks — so "grow
from that sheet's row" has nothing to attach to yet.

- `aMenuOpensItsPageOutOfTheRow`: from Every song's Alla Hornpipe, the album
  grew from (44, 341)–(176, 473), the row's cover, and did not rise; the artist
  from (0, 319)–(1080, 495), the row; settings had no origin and rose. The
  entries are chosen through `onMenu`, as above. Falsified: the menu's page
  opened from no origin.

**Generic, for `kotlin/ark-ui`** (where they now are: "ark-ui", below). `Springs.kt` and `Motion.kt` name no harken
type: `PageMotion` is told which routes are tabs (`Tab.routes`), `rememberHero`
takes its corner, and the back plumbing (`PageBack`, `Relayed`, `Pull`), the
origins (`Origin`, `Tapped`, `Hero`, `Spot`) and the settle (`Springs.settle`,
`Capped`, `noDecay`, `preview`) depend on Compose, activity and navigation
only. What is harken's — which rectangle of a track row is its cover, which
menu entry grows from which — stays in `Components.kt` and `HarkenApp.kt`.

**The fling bench** (HARKEN_FLING_BENCH=2000, the reported two passes; the
times move by tens of percent with this machine's load, so each "after" is
read against a "before" run beside it):

| | compose+measure, per entering row | draw, per frame |
|---|---|---|
| before (2d2cebb), first run | 365 / 334 µs, 32,720 B | 7.4 / 7.1 ms, 27,977 B |
| before, four runs each beside an after run | 375–521 µs, 32,697–32,892 B | 7.4–9.3 ms, 27,977 B |
| the row's place kept by a node of its own on every row (not kept) | 434 / 443 µs, 33,202 B | 8.0 / 8.4 ms, 27,977 B |
| a node on the menu button, kept by a `remember` (not kept) | 416–744 µs, 32,947–33,215 B | 7.3–9.0 ms, 27,977 B |
| after, final: the place asked of the button's own label node on the tap | 411 / 412 µs, 32,717 B | 7.8 / 8.0 ms, 27,977 B |

Bytes are the steady number here, to within about 150 from run to run, and
the final row allocates what it did: its label's node now also answers where
the button is, in place of the `semantics` element and lambda it replaced.
The times are this machine's load; nothing about the scroll's own path
changed, and the draw did not move.

`ScrollTests` still composes nothing per position tick and nothing at rest.

Not verified: the feel of any of it on a phone — the stiffness, the preview's
limit and give, the cap, the rise — and whether the second report was the
covers or the keyboard. Every number above is HotSpot's and layoutlib's.

### ark-ui

The owner asked for the components, the animations and their timing, and
some of the onboarding, in a library other apps reuse: `kotlin/ark-ui`,
package `dev.arkdb.ui`, Gradle project `:ark-ui` — an Android library on
Compose alone, with no ArkDB in it, the Kotlin twin of `rust/arkui`
(`kotlin/ark-ui/README.md`). The SwiftUI twin is a plan,
`docs/plan-arkui-swift.md`. Nothing about how anything looks or moves
changed: the move was held to byte-identical screenshots.

**What moved, and where:**

- `harken/android/…/Springs.kt` → `ark-ui/Springs.kt`, as it was:
  `Springs`, `Capped`.
- `harken/android/…/Motion.kt` → `ark-ui/Motion.kt`, as it was: `PageMotion`,
  `Moving`, `Origin`, `Tapped`, `Hero`, `rememberHero`, `Spot`, `PageFrame`,
  `PageBack`, `Relayed`, `Pull`, `LocalPageMotion`, `LocalPageFrame`,
  `PAGE_DIM`. `PageMotion.rose` is settable from outside now, because
  harken's `MotionTests` resets it.
- `harken/android/…/PlayerSheet.kt` → `ark-ui/ExpandingCard.kt`, made
  generic: `ExpandingCardState` (was `PlayerSheet`; `CardFace.Collapsed` /
  `Expanded` were `Face.Bar` / `Full`), `rememberExpandingCard`, and
  `ExpandingCard(state, stand, onOpen, collapsed, expanded, traveler,
  onSwipeLeft, onSwipeRight, openLabel, spec)` — the anchored drag, the
  capped settle, the axis split, the back gesture onto `p`, the faces laid
  out once and moved, the travelling piece — with every number of the old
  `Geometry` in `ExpandingCardSpec`, whose defaults are harken's. The bar's
  height is a list (56 dp and 2 dp), each rounded before they are summed,
  as the old `Geometry` did, so the card sits on the face's own pixels at
  any density. harken's `PlayerSheet.kt` is now a `typealias PlayerSheet =
  ExpandingCardState` and `PlayerCard`, which hands the card `MiniFace`,
  `FullFace` and the cover.
- `harken/android/…/Art.kt` → `ark-ui/Art.kt`: `Art.hash`, `Art.pick`,
  `Square`, `Cover`. `Cover` asks a `CoverLoader` (`LocalCoverLoader`) where
  it asked harken's `Covers` (`LocalCovers`, gone); `Covers` implements it,
  and counts its launches through `launched()`. `Square` reads the
  gradients from `LocalArkTheme` where it read `LocalHarken`
  (`HarkenExtras`, gone); `HarkenTheme` provides `ArkTheme(Palette.DarkArt
  or LightArt)` — a data class, as `HarkenExtras` was, so a theme made again
  is not a change to every screen under it.
- `harken/android/…/Recompositions.kt` → `ark-ui/Recompositions.kt`.
- From `Components.kt`: `Glyph`, `Heading`, `TabTitle`, `PageBar`, `Empty`,
  `Shelf`, `SheetRow`, `Fetching` and `FETCHING_DELAY_MS`, `LocalMoreGlyph`,
  and the menu button, `MoreButton(about, origin, onMore)`: where a track
  row and its cover are is harken's (`TrackOrigin`, a `RowOrigin`, two
  singletons), asked of the button's node on the tap as before. `RecordCard`
  is `CoverCard`, and its count is `"CoverCard"` in `ScrollTests`. From
  `LibraryScreen.kt`: `ListRow`, whose chevron is now a `trailing` slot that
  defaults to it.
- From `Theme.kt`: `Space` and `quiet`. The palette, the type and
  `HarkenTheme` stay.
- The menu, back and chevron glyphs are `ArkGlyphs` (the same Lucide paths);
  harken's `Glyphs.More`, `Back` and `Chevron` name them.
- From `kotlin/ark-android`: the onboarding's screens without their state —
  `OnboardingPage`, `ServerScreen`, `SignInScreen` (`SignInPhase`),
  `PermissionScreen`, `OpeningScreen` — and the splash's keeping,
  `holdSplashUntil` and `keepsSplash`. `ark-android` keeps `ArkApp`, the
  machine, `ServerStep`, `SignInStep`, `PermissionStep` and `Opening` as
  adapters that read a `Step` into them, and `holdSplash`, which holds it
  until `ArkHost.ready`; it depends on `ark-ui` (`api`).

What stayed harken's: `Track`, `Album`, `Playlist`, `Library`, the player
and its faces, the palette, the glyphs only harken draws, `TrackRow`,
`PageHeader`, `Offline`, `HarkenApp`'s wiring, the screens. What stayed
`ark-android`'s: the session, the host, `Remembered`, `Probe`,
`ModuleMemory`, `Auth`.

**Its public API**, `explicitApi()` and a KDoc on each: `Springs`,
`Capped`; `PageMotion`, `Move`, `Origin`, `Tapped`, `Spot`, `Hero`,
`rememberHero`, `PageFrame`, `Moving`, `PageBack`, `Relayed`, `Pull`,
`LocalPageMotion`, `LocalPageFrame`, `PAGE_DIM`; `CardFace`,
`ExpandingCardState`, `rememberExpandingCard`, `ExpandingCardSpec`,
`ExpandingCard`; `Art`, `Square`, `Cover`, `CoverLoader`,
`LocalCoverLoader`; `Glyph`, `Heading`, `TabTitle`, `PageBar`, `Empty`,
`Shelf`, `CoverCard`, `ListRow`, `Chevron`, `SheetRow`, `Fetching`,
`FETCHING_DELAY_MS`, `MoreButton`, `RowOrigin`, `LocalMoreGlyph`;
`OnboardingPage`, `ServerScreen`, `SignInScreen`, `SignInPhase`,
`PermissionScreen`, `OpeningScreen`, `OPENING_DELAY_MS`; `holdSplashUntil`,
`keepsSplash`, `SPLASH_CEILING_MS`; `Space`, `ArkTheme`, `LocalArkTheme`,
`quiet`; `ArkGlyphs`; `Recompositions`.

**Its tests**, JVM unit tests run by `harken-apk` before the APK
(`:ark-ui:testDebugUnitTest`): `ArtTests` (the art hash's five pins, moved
from `MediaTests`, and the pick read unsigned), `SpringsTests`
(`thePreviewSaturates`, moved from `MotionTests`, and the cap), and
`SplashTests` (moved from `ark-android`). The rest of `MotionTests`,
`ScrollTests` and `FlingTests` drive the real `HarkenApp` and stay
harken's. Falsified once from the new place: FNV's prime changed in
`Art.hash` — `theDerivedSquareMatchesTheDesktop` and
`aNamePicksOneOfTheSetByItsHash` failed, 5 run, 2 failed.

**The build.** `harken/android/settings.gradle.kts` includes `:ark-ui` by
path as it does `:ark-android`; `checks.kotlin` holds its sources to ktfmt
with the rest; `harken-apk`'s task list and its recording's begin with
`:ark-ui:testDebugUnitTest`. The source filters already took all of
`kotlin/`. The recorded graph needed nothing new, with one line: on its
own, `ark-ui`'s test classpath took kotlinx-coroutines 1.7.3's word for
`kotlin-stdlib-jdk8:1.8.20`, which no other configuration asks for and
`deps.json` does not hold, so its build pins `kotlin-stdlib-jdk7` and
`-jdk8` to 2.0.21, the version every other module resolves. `deps.json`
was not re-recorded.

**Held to:**

- `harken-android-screens`: all 92 PNGs (and `index.html`) byte for byte
  what c87689a's build drew, compared file by file: 0 differ.
- `ScrollTests`: at rest for a second `{}`; over four position ticks
  `{Progress=4}` — no row, screen, frame or card composed again.
- `MotionTests`, as before: the slow release lands in 216 ms and the flick
  in 133; the player's back previews 0.0918 at 0.3 and 0.0999 at 0.9; the
  menu's album grew from the row's cover, (44, 341)–(176, 473).
- The fling bench (HARKEN_FLING_BENCH=2000, passes 5 and 6), the
  unchanged tree and this one run side by side:

  | | compose+measure, per entering row | draw, per frame |
  |---|---|---|
  | before (c87689a) | 424 / 397 µs, 32,820–32,822 B | 8.5 / 8.0 ms, 27,977 B |
  | after | 452 / 437 µs, 32,887 B | 8.4 / 8.3 ms, 27,977 B |

  The bytes per entering row are what they were to within 70 (an earlier
  pair read 32,836 before and 32,729 after); the draw is the same to the
  byte. One run of the moved tree read 29,289 B a frame for the draw and did
  not repeat in two more (with and without a profiler attached); the times
  are this machine's load.

### Motion: tabs

The owner, on the release APK: "if i am on the home screen, and then tap an
album, and then tap library on the nav bar, and then tap the home, it still
animates the album when i tap home. and when i tap home again, it stays on
the album. … tapping any nav item again should bring them back to that
screen. also if im on search or library and swipe back to preview, it
instantly changes to the previous screen without the standard backswipe
animation."

Both came from one NavHost holding all three tabs, switched by
`navigate(route) { popUpTo(start) { saveState }; restoreState }`. Home is
that graph's start, so its own pages were saved under the destination every
other tab's state was saved under too, and a tap on Home restored the album
as a fresh navigation — a page growing — then found nothing to restore the
second time. And a tab's root was a page of nothing: back from Library's
root was the NavHost popping to Home, which `PageMotion` called a cut.

What it does now, which is what Material's bottom-navigation guidance, iOS's
tab bars, YouTube and Instagram do:

- **Each tab keeps its own stack.** One NavController per tab, and one
  NavHost per tab drawn by `ark-ui`'s `TabStack`; only the tab shown is
  composed, and the others' state — their stacks, their pages' held rows,
  every list's scroll — is kept by a `SaveableStateHolder` while they are
  not. A tab switch is not a navigation of any NavHost, so it cannot
  animate: Home → album → Library → Home shows the album again on the first
  frame, with nothing moving. "Tab switch" is a property of the change
  (the tab shown moved), not of a route.
- **The tab already shown goes to its root.** From a page it pops to the
  root with back's own motion — the page shrinks into its origin card, or
  reverses its rise. At the root its list scrolls to the top
  (`Reselect`, `ScrollsToTop`, an animated scroll), on Home, Search and
  each of the Library's lists.
- **Back at the root of Search or of the Library goes Home**, previewed
  exactly as a page is: the tab sheds up to a tenth of its size toward the
  limit on the same curve (`Springs.preview`), moves aside, rounds to
  28 dp, and Home — whatever its stack shows — is drawn under it at the
  page dim. Let go past the threshold and it sheds 5% more and fades out on
  a critically damped spring at stiffness 2,500 (`Springs.fade`, about
  150 ms), and Home is the tab, lit in the bar. Cancelled, it springs back.
  The pull-down applies only to pages, so not at a root.
- **Back at Home's root is not taken at all.** No callback is enabled, so
  the system's own back-to-home animation runs
  (`enableOnBackInvokedCallback` is already on). On a page of Home's stack
  back is the page's, as before.
- The player and the sheets keep priority: `PageBack` is composed before
  them and stands down while the player is up, as before.

`PageBack` takes `toFirst` — what back at a root does — and decides between
the page's path and the tab's when the gesture starts. The tab under the
one leaving hears no back (its NavHost has a dispatcher nothing dispatches
to); the one shown is `Relayed` as before.

Measured on the frame clock, at 60 fps, from the back gesture's release at
r = 0.9 (scale): 0.900 at 0 and 16 ms, 0.891 at 33, 0.864 at 66, 0.854 at
100, 0.851 at 133, and 0.850 by 166 ms, gone and Home shown at 233 ms (the
spring's own rest). Reselecting Home on the Brandenburg album: the page's
size from 0.93 at 50 ms to 0.01 at 283 ms, the same curve as a back.

Tests (`MotionTests`, each falsified once):

- `aTabKeepsItsOwnStack`: Home → album → Library → Home has the album on
  the first frame and draws 0 frames mid-move. Falsified: the tab's stack
  popped on a switch — "Home's album is there again on the first frame".
- `reselectingATabOnAPageGoesBackToItsRoot`: Home tapped on the album runs
  the page's size through the in-between to 0 and lands on Home's root.
  Falsified: reselect only asking the root to scroll — the page never moved.
- `reselectingATabAtItsRootScrollsToTheTop`: every song scrolled to (40,
  135), Library tapped: (0, 0). Falsified: `ScrollsToTop` doing nothing —
  still (40, 135).
- `backAtTheLibrarysRootPreviewsAndLandsOnHome`: back is taken at the
  Library's root; a cancel springs back to scale 1 and keeps the Library;
  the preview reads 0.908 at r = 0.3 and 0.9001 at 0.9 — the limit, a tenth
  of the size, all but reached — with Home under it; let go, it is gone by
  233 ms and Home is the lit tab. Falsified: no `toFirst` — "back at the
  Library's root is taken".
- `atHomesRootNoBackIsTaken`: no enabled callback on the dispatcher at Home's
  root, one on a page. Falsified: `toFirst` always given.
- `aTabArrivesWithNoFrameInBetween` now also walks Library → Search →
  Library → Home → Library: 0 frames of a page or a tab moving. Falsified:
  a tab switch that ran the tab preview's spring — 36 frames.

The frames — back from the Library at r = 0.3, 0.6 and 0.9, and three of
its completion — are written with HARKEN_MOTION_FRAMES set
(`tab-back-30`, `-60`, `-90`, `tab-back-done-11`, `-13`, `-15`, by how much
of its size it had shed).

The screenshots are byte for byte what they were (92, 0 differ):
the screens did not change, only how they are reached. `ScrollTests` still
composes nothing per tick or at rest. The fling bench, beside the ark-ui
commit run the same hour: 32,803–32,805 B per entering row against
32,822, 27,977 B a frame for the draw against 27,977.

Not verified on a phone: the system's own back-to-home at Home's root, and
how 150 ms of fade feels.

### Where a destination's state lives

The owner, on the release APK: "when i swipe back from library to home and
ive scrolled down in home, it still shows it at the top, then switches to
the scroll position. this has happened before, so make sure that the
architecture fixes this so its not duct taped or one off fixed for a single
screen, but for everything".

It had happened twice before, each time fixed for the one screen:

- **Search came back at its top after an album** (e284554): its results
  were a `remember` in the screen, empty on the frame it came back, so its
  restored scroll had no rows to land in. They moved to a holder the
  session keeps (`Searched`).
- **An album, an artist or a playlist came back at its top after a page it
  opened**: the same, for a page's rows. They moved to a `ViewModel` on the
  page's back stack entry (`Holding`).
- **Home, under the tab back preview from the Library**, which is this.

**The cause, measured.** `TabStack` drew the tab shown through
`Relayed(motion) { content(t) }` and the first tab, under a tab going back
to it, through `CompositionLocalProvider(… provides deaf) { content(t) }`.
Two call paths, so every `rememberSaveable` inside — each screen's
`rememberLazyListState()`, and the NavHost's own state holder — had a
different key in each: Home's place, saved while it was the tab shown, was
not found where the preview drew it. And when the back landed Home changed
branch again, so the composition the person had been looking at was thrown
away and a third made, which did find the place. A probe at the parent
commit, on a phone-sized screen, Home scrolled to (3, 20):

| | Home's list | its state |
|---|---|---|
| left | (3, 20) | #545448164 |
| under the preview, 8 frames | (0, 0) | #371694173, a second |
| after the release, 13 frames (the fade) | (0, 0) | the second |
| then | (3, 20) | #2110370842, a third |

The same probe with only the two wrappers made one call: (3, 20) on every
frame, one state from the preview to the end. That is the root cause; the
fix is the rule below, so that the next screen cannot have it.

**The rule** (`kotlin/ark-ui`'s `Destination.kt`, `Tabs.kt`, `Motion.kt`):

1. **One identity per destination, wherever it is drawn.** A tab's content
   is one movable content, called from one place with one set of
   wrappers; only the values handed to it differ (which back dispatcher it
   hears, which lifecycle it sees, which `Reselect` it answers). `Moving`
   is one call for a page, whatever it is. A tab under the back preview is
   the composition that becomes the tab shown when it lands. Audited: the
   page under a page going back is the NavHost's own composition of that
   entry, one call (`Moving`); `PageFrame` holds no state of a
   destination's; the expanding card draws no page behind it — the page is
   its sibling and is never composed again by it.
2. **One owner of every scroll state: the destination's entry.** Every
   NavHost destination is made by `destinations(routes, motion, route)`,
   which is `Moving` and `Destination` around each: a `ViewModel` on the
   back stack entry holds a `DestinationState`, the places of its
   containers by name, saved with the entry's own saved state. Every
   scrolling container takes its state from it —
   `rememberDestinationListState(name)`, `…GridState`, `…ScrollState` — so
   a destination drawn anywhere, as often as it is, starts where it was
   left on its first frame, and so after the process is killed. A tab's
   root is its NavHost's start entry, so a tab is the same case. Horizontal
   shelves keep their places too: `Shelf(section, …)` is
   `"shelf:" + section` in its destination, so a shelf scrolled out of its
   list and back, or left and come back to, is where it was. The Library's
   four lists are four names, so each keeps its own. Sheets and the
   onboarding say they are not destinations:
   `rememberTransientListState`, `…ScrollState`, at their top each time.
3. **A destination's first frame has its rows.** Its lists come from
   holders that outlive the composition: Home's and the Library's from the
   session, a page's from its entry (`Holding`), Search's from the session
   (`Searched`), the diagnostics' from their entry (`Reading`, new: they
   were a `remember` filled by an effect, and came back from another tab
   clamped to the top of "Opening…"). Where the rows can genuinely not be
   there yet — read again after the process was killed — the destination
   asks `destinationKeepsAPlace()` and draws nothing for those milliseconds
   rather than a list too short to hold the place: Home before its lists
   are read (under the splash), a page before its view is read (`Ready`),
   Search before its restored needle is answered (which it now asks at
   once, not after the typing beat).
4. **A destination knows whether it is in front.** Its content sees the
   lifecycle of where it is (`LocalLifecycleOwner`): RESUMED in front at
   rest; STARTED while it is drawn and not in front; CREATED, with its
   composition gone and its places kept, while it is not drawn at all. A
   NavHost already moves its entries so, through our relayed back: on the
   gesture's start it calls `prepareForTransition` on both entries, which
   holds each at STARTED until the move completes, then the one shown is
   RESUMED and the other CREATED; that is used as it is. `TabStack` gives
   each tab a lifecycle of its own under the activity's — RESUMED as the
   tab shown at rest, STARTED for both tabs while back from a root is
   under way, CREATED while not drawn — and the tab's NavHost hands it on
   to its entries, so a page on a tab's stack is held the same. Everything
   under the player while it is up or on its way is STARTED
   (`LifecycleCapped` around the page in `Chrome`, reading the card's `p`
   as a snapshot, never in composition).

**Video, an animation, anything that costs while it runs**: owned at the
entry (a `ViewModel`, or an entry-scoped holder like `Holding`), so it
survives the composition moving; the composition only attaches a surface;
and it plays while the lifecycle is RESUMED (`LifecycleResumeEffect`,
`repeatOnLifecycle`). The preview of a back then shows the video's current
frame at once, paused, and it goes on when the back lands in it.

**What holds it** (each falsified once):

- `SourceTests`, a scan of every Kotlin file under harken's app and
  ark-ui: no `rememberLazyListState(`, `rememberLazyGridState(`,
  `rememberLazyStaggeredGridState(` or `rememberScrollState(`; no state
  made by hand outside `Destination.kt`; no lazy list or grid without a
  `state =`, whose default is the same bug with nothing to scan for. Its
  second test holds the scan itself to the shapes it is for. Falsified:
  Home's list made by `rememberLazyListState()` — "HomeScreen.kt calls
  rememberLazyListState(".
- `RestorationTests`, parametrized over `Route` — the list every tab's
  NavHost is built from — and the Library's root once per list: 11 cases
  (Home, Search, Library × Playlists, Albums, Artists, Songs, Album,
  Artist, Playlist, Settings, Diagnostics). A route added to `Route` does
  not compile without a case (`reach` is a `when` over it). Each runs the
  real `HarkenApp` over the demo's songs and enough more that every list
  and shelf scrolls (48 songs in 24 albums by six artists, 16 playlists,
  one with 30 songs), at a phone 1,400 px tall so Settings scrolls too.
  First it checks every scrolling node on screen scrolls by a state its
  destination keeps — reflection on the node, against the destination's
  own — then scrolls every container off its top, and takes it through:
  the tab switched away and back; a page opened from it and gone back from
  by the page's arrow, by the edge (its preview at 0.3 and 0.9 drawn over
  it), and by a pull from the page's top; the tab back preview from the
  Library (0.3, 0.9, cancelled; then 0.3, 0.9, let go) with it underneath,
  or, for Search's and the Library's roots, as the tab leaving, and then
  its tab chosen again; its tab reselected from a page above it (roots);
  its shelves scrolled out of its list and back (Home, Artist, Search);
  and the process killed (everything saved through a
  `SaveableStateRegistry`, the composition and every `ViewModel` thrown
  away, composed again from what was saved). The assertion, every frame
  the destination is drawn on from the first, previews included: every
  container it had, at the index and offset it had. 142 transitions over
  the 11 cases (Home 15, Artist 14, Search, Album, Playlist, Settings and
  Diagnostics 13, each Library list 12), every one green; and where the
  back lands, the composition is the one that was under the preview
  (`entered` unchanged, the same state objects). The Library's chips row
  fits a 1,080 px screen, so it is held at 0 and said to be.
- `LifecycleTests`: under the tab back preview Home is STARTED and the
  Library leaving is STARTED, never RESUMED, on every frame from 0.3 to
  0.9; cancelled, Home is not composed and CREATED, the Library RESUMED;
  let go, Home RESUMED — the same composition and the same list it was
  under the preview — and the Library CREATED. A page under a page's back
  preview is STARTED, as the NavHost says; cancelled, CREATED; landed,
  RESUMED. With the player down Home is RESUMED; half up, STARTED. And
  each tab's graph, read back from its NavController, is exactly the
  routes `Route` lists for it, and every route is a restoration case.

Falsified:

- The two wrappers put back in `TabStack`: every case on Home's stack
  fails — "Home: composed again from scratch when the tab back landed",
  and the same for Album, Artist, Playlist, Settings and Diagnostics — and
  `LifecycleTests` fails with Home's composition entered 4 times where it
  was 3. (The places themselves survive that alone now, since they are the
  entry's; with the helpers also made the composition's again, the first
  check fails on Home: "a scrolling node whose state the destination does
  not keep".)
- The diagnostics' numbers back in a `remember` filled by an effect: every
  transition of the Diagnostics case fails on its first frame, at 96 where
  it was left at 137 — the top of "Opening…" is as far as it can go.
- `TabStack` leaving both tabs RESUMED while back is under way: "at 0.3
  Home under the preview is RESUMED", "the Library leaving is RESUMED".
- A screen's `rememberLazyListState()`: `SourceTests`, as above.

**Held to:**

- The screenshots: all 92 byte for byte what the parent commit draws,
  recorded side by side and compared: identical.
- `ScrollTests`: at rest for a second `{}`; over four position ticks
  `{Progress=4}`. The lifecycle under the player is read as a snapshot in
  an effect, so the card moving composes nothing.
- `MotionTests`, all 17, as before.
- The fling bench (HARKEN_FLING_BENCH=2000, passes 5 and 6), the parent
  commit and this one run side by side, twice, once in each order:

  | | compose+measure, per entering row | draw, per frame |
  |---|---|---|
  | before | 395–500 µs, 32,664–32,788 B | 7.9–9.3 ms, 27,977–27,978 B |
  | after | 419–505 µs, 32,782–32,802 B | 7.9–8.3 ms, 27,977–27,978 B |

  The bytes per entering row are what they were to within the run-to-run
  spread (about 150 B); the draw is the same to the byte. A song's row
  makes no scroll state; a shelf makes one, once.

Not verified on a phone: how the preview looks now (it is the same frames,
at the place Home was left), and a real process death — the test throws
the composition and every `ViewModel` away and composes again from the
saved values, but does not parcel them.
