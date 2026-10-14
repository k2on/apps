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
derived squares); how the full player animates in and out; the
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
