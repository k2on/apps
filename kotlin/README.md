# The Kotlin runtime

`ark-runtime` (package `dev.arkdb`) is ArkDB's client for the JVM and
Android, at spec version 4: what a phone needs to load a module it did not
author, verify it, interpret it, keep its views up to date, hold a replica
of the log and rebase it, speak the protocol, and be its own authority when
it is alone. It is a port of `rust/ark`, which is the specification
(`spec/README.md`), and it is held to the same vectors the Rust is, file for
file, and to every trace of `packages.trace-corpus`, by `checks.kotlin`
(`docs/plan-phones.md` S3). `ark-client` (package `dev.arkdb.client`) is
the peer around it, a port of `rust/ark-client`: the storage a replica
survives a restart in, written byte for byte as the Rust writes it, the
peer that writes it and goes alone and back, its link to a server and its
sign-in, and maintained views. `ark-ui` (package `dev.arkdb.ui`) is what an
Ark app on Android looks like and how it moves, and `ark-android` (package
`dev.arkdb.android`) the onboarding every Ark app on Android shares, drawn
with it, and the session it hands the app. `harken-peer` is harken's headless peer on it, speaking
`harken-peer`'s lines, which the fleet runs beside Rust peers;
`harken/android` is the app on it.

A phone's runtime is a **client**: it interprets modules and does not
author them. The domain is written once, in Rust, and `arkc` emits the
module (`harken/domain/harken.ark`). So there is no authoring vocabulary
here, and no printed domain — the v3 tree had both, and they are gone.

## What is here

Each file is a port of the `rust/ark/src` file named beside it; the doc
comments name the reference function for each piece.

| file | `rust/ark/src` | what it is |
|---|---|---|
| `Value.kt` | `value.rs` | the eight values and the total order over them (text by code point, never by UTF-16 unit) |
| `Canon.kt` | `canon.rs`, `sha256.rs` | canonical CBOR, strict: the same `DecodeError` for the same bad bytes; UTF-8 both ways; SHA-256 through the JDK |
| `Schema.kt` | `schema.rs` | tables, columns, keys, indexes, references, text indexes (D4), `check_schema` |
| `IR.kt` | `ir/mod.rs` | IR v4: plans (group sources, lookups, related plans on any column, having, projection, expression order keys, limits), `Pred::Has`, `When`, `Exists`, scopes and holds, private blocks, `HasRole`, routers; `strip` |
| `Decode.kt`, `Encode.kt` | `ir/decode.rs`, `ir/encode.rs` | a module from and to its value, every v4 key written only where present; `calls`, `reaches` |
| `Normalize.kt` | `ir/normalize.rs` | symbols renumbered in evaluation order |
| `Hash.kt` | `hash.rs` | `leaf`, the additive `Digest` mod 2^256, `state_hash`; `closure`, `function_hash` over the stripped function, `stripped`, `reads_roles`, `module_hash` |
| `Verify.kt` | `verify.rs` | every complaint by the reference's name, `complete_orders` |
| `ScopeCheck.kt` | `scope.rs` (the verifier's half) | scopes by table, a filter folded under a context, the union's columns, `check` (`NotHeld`) |
| `Store.kt` | `store.rs` | the rules (`complete`, `judge_put`, `insert`, `upsert`, `update`, `judge_delete`), `MemoryStore` with its secondary, reference and trigram indexes and its per-table digest, the `Overlay` |
| `Eval.kt` | `eval.rs` | the interpreter: input checks, middleware in `uses` order, the body or the plan, one transaction; checked arithmetic; private blocks deferred and run by the authority only; the form validator; §13's wide sum in a plan's node |
| `Std.kt`, `UnicodeTables.kt` | `stdlib.rs`, `unicode_tables.rs` | the standard library; the tables are generated (below) |
| `Raw.kt` | `raw.rs` | `ark.put_row` and `ark.delete_row` by their fixed hashes, as a client replays them |
| `View.kt` | `view.rs` | `pull`, `answer`, `read`: the one evaluator of plans, a bare read served in order by an index where one serves |
| `Maintain.kt` | `view.rs` | `hydrate`, `push_all`, `settle` and its patches, `splice`, `contract`; R9's kept numbers — counts and sums of any size, extremes read again through an index |
| `Facts.kt`, `Show.kt` | `protocol.rs`, `store.rs` | the change form the vectors and the wire write; a text as Rust's `{:?}` writes it, which a refusal's text is made of |
| `Log.kt` | `log.rs` | `LogEntry` (the reference's `Entry`; `View.kt` has the view's), the log a client alone keeps, `widened`, `state_at`, `take_entries` |
| `Peer.kt` | `peer.rs` | `Replica`: pending and its records, the rebase by records (R2), settle once per pump (R8), verdicts and divergences, roles frozen (D1), private entries by facts (D3), raw writes replayed (D4), an intent with no closure kept, a union by facts (D2), `fork_back`; `Authority` as a peer alone is one; `local_commit` |
| `Protocol.kt` | `protocol.rs` | every client and server frame to and from its value, each optional field where the reference writes it; `decode_for`; `Client` — hello, push, paging, settle, held, sign-in and token, verify, snapshots of every kind; `refusal_text` |
| `ScopeCheck.kt` | `scope.rs` | also `deviceSchema`, the one piece of a union's client half a client needs |
| `Journal.kt` | `journal.rs` | a log on a key/value storage: the snapshot and its pages, `Layout.alone()`, open, append, merge, create, destroy |
| `Trace.kt` | `trace.rs` | `Input`, the durable copy (`Durable`, `KeptStore`), `reopenFrom`, alone, commit, join, `Observer`, `Player`, `replay`, `differs` |
| `Reads.kt` | `ir/reads.rs` | the tables a function reads outside its plan: what a maintained view's middleware is re-run on |
| `Json.kt` | `json.rs` | the JSON dialect a headless peer reads and answers in: `decode`, strict where the reference is; printing is `Value.show` |

Three representations differ from the Rust and say nothing different. A row
is a `Value.VStruct` of every column — the Rust lays one out positionally
behind its table's names (`docs/plan-perf.md` R11), and "laid out as the
table's" is here a struct whose fields are exactly the table's columns. An
index posts keys where the Rust posts ordinals (`docs/plan-db.md` D7.2):
the same rows, in key order. And a replica's optimistic view is its own
store, copied from the confirmed store at each replay and moved by every
change the confirmed store moves by, where the Rust shares every table the
view has not written (D7.3): the rows, the hashes, the patches and the
frames are the same, and the cost is a copy per replay. The Rust's native
procedures (`authoring::Procedure`) are an authoring vocabulary's, so an
entry here runs by a raw write, by its closure, or not at all.

## `ark-client`

| file | `rust/ark-client/src` | what it is |
|---|---|---|
| `Storage.kt` | `storage.rs` | `Storage` (a `Keys`), `MemoryStorage`, `DirStorage`; the `replica`, `facts.<n>`, `pending`, `pending.<n>` and `who` records, `Records.load` |
| `Writer.kt` | `peer.rs` (`persist` and its helpers) | what is written and when: a journal page or a snapshot, compaction once the pages outgrow it, the intents by generation |
| `Peer.kt` | `peer.rs`, `domain.rs` | `Domain`; `Peer` — open from a storage (carried across alone and back), mutate by name, query, check, sign-in, leave and join with the local history in the alone journal, `connect`, `pump` and what it moved (`Pumped.quiet`), the wake (`setWaker`) and the redial's `deadline`, `status`, `standing`, verify, `say`; the sans-io `connected`, `recvFrame`, `takeOutgoingFrames` for a transport of the caller's |
| `ClientView.kt` | `view.rs` | a query held as a list: the middleware run first and the tables it reads kept, every change pushed as patches, `Reset` when a signer or the middleware's outcome moves; `splice` |
| `Link.kt` | `link/mod.rs` | `Link`: dial when due, back off half a second doubling to thirty, reset when a connection opens, stop and resume; `Timing`; `QueuedSocket`, a transport's callbacks kept until polled; `platformDial` |
| `Transport.kt`, `WebSocketTransport.kt` | `link/native.rs` | one connection carrying frames; OkHttp's WebSocket, pinging every `pingEveryMs` and giving up when a ping goes unanswered, its handshake bounded by `connectTimeoutMs` |
| `Auth.kt` | `ark-auth`'s `client.rs`, `util.rs` | `Login` and `Account` as serde writes them; `login` (a dev server answers with a code; anything else is a page, handed to `open`, and the code read off the loopback request back), `exchange`, `socketUrl`, `loginUrl` |
| `Autos.kt` | `autos.rs` | the platform's, or a seed's by the reference's generator |

The transport is OkHttp's on the JVM and on Android alike — one dependency
both have, rather than the JDK's `java.net.http` on one and OkHttp on the
other — and sign-in is `HttpURLConnection`, which both have too. OkHttp
calls back on threads of its own; `QueuedSocket` keeps what they said until
the link polls, so the peer is touched only from its own loop.
`DirStorage` writes a temporary file, syncs it, renames it and syncs the
directory (`FileChannel.open(dir, READ).force(true)`), as the reference
does; a platform that refuses to open a directory degrades to the rename's
durability being the file system's, and says so (`syncsDirectory`).

## `ark-android`

The onboarding every Ark app on Android shares (`docs/plan-android.md`),
and what it hands the app once it is done:

```kotlin
ArkApp(
    identity = AppIdentity(name = "Harken", scheme = "harken", mark = painterResource(R.drawable.mark),
        light = …, dark = …, module = { assets.open("harken.ark").use { it.readBytes() } }),
    permissions = listOf(Permission.Notifications(why = "…")),
) { session -> HarkenApp(session) }
```

An app names only what is its own — its name, the scheme its sign-in comes
back on, its mark, its two colour schemes and its type, its module — and
the permissions it wants, a sentence each. Three steps, each remembered:

- **The server.** One field: `host`, `host:port` or a URL (`Address`).
  A URL is taken at its word; otherwise `https`, and `http` too only for
  loopback, the private ranges, link-local, the carrier-grade range a
  tailnet uses, and a name with no dot or under `.local`, `.lan`,
  `.home.arpa`, `.internal`. `/healthz` is asked for JSON (`Probe`) and the
  answer decides (`Health`): on only when the module it runs — or one it
  ran, the server being newer — is this app's, by `Domain.hash`. Every
  failure is a sentence naming which of those it was.
- **Signing in**, through the server's own page in a Custom Tab
  (`androidx.browser`): `/auth/login?redirect=<scheme>://auth`. The server is
  the only OpenID Connect client (`ark-auth`), so the phone holds no secret
  and knows no provider, and a dev server's own form works the same way.
  `<scheme>://auth?code=…` comes back to the activity — **the intent filter
  is the app's manifest's**, on the activity running `ArkApp`, with
  `launchMode="singleTask"` (`ArkApp.kt` has the snippet) — and the code is
  exchanged at `/auth/exchange` (`Auth.exchange`). A code arriving as the
  launch intent, the app having been killed behind the tab, is exchanged
  the same way.
- **Permissions**: one screen per declared permission with the app's
  reason, *Allow* and *Not now*, asked once; one the platform lacks
  (notifications below Android 13) is skipped. `Permission` is open, so an
  app with another need declares its own.

The server, the login and which permissions were asked are kept in the
app's private preferences (`Remembered` over a `Shelf`); a launch with a
server and a login goes straight in. A `Denied` from the server, and
*Sign out* (`ArkSession.signOut`), come back to *Sign in* with the server
kept; the session is closed and written down, and its replica — one
directory per server — and its pending intents stay, so signing in again as
the same person pushes them.

`ArkSession` is the Kotlin `Peer` over that replica, signed in, linked to
the server's `/sync`, on a thread of its own that runs only when something
happens (`docs/plan-reactive.md`): one drain — a pump — when the socket
wakes it, after a `post`, and at the peer's one deadline (the redial after
a close, one one-shot timer). Connected and idle it has no timer at all;
the keepalive is OkHttp's pings. A screen may read `status` (linked, the
link in a word, how much is not synced), published when it changes;
everything else touches the peer on its thread — `post`, `run` (a read,
which drains nothing), and `onChange`, called with what moved after every
drain that moved something and never after one that did not, which is
where an app splices its views.

The steps are a pure machine (`Onboarding`: a step and an event in, a step
and the effects to run out), run by `ArkHost`, a `ViewModel`; the screens
(`ServerStep`, `SignInStep`, `PermissionStep`) draw a step and nothing
else, so an app's screenshots render them. Everything under the screens is
plain Kotlin, and `OnboardingTests` and `ServerTests` hold it on the JVM
(`testDebugUnitTest`): every transition and its effects, including a
denial while running and signing out; the address normalisation; the
health decision; the probe's order and sentences over a request that is a
lambda; the remembered store.

It is an Android library, so it is **not a project of this workspace**,
which is the JVM's and builds without an SDK: an app's build includes it
by path (`include(":ark-android")`, its `projectDir` here) and supplies the
plugin versions, as `harken/android` does. Its sources are held to ktfmt's
kotlinlang style by `checks.kotlin` with the rest; its tests run in the
app's build (`nix build .#harken-apk` runs them before assembling).

## `ark-ui`

What an Ark app on Android looks like and how it moves (`dev.arkdb.ui`,
`ark-ui/README.md`): the springs, pages grown out of what was tapped and the
back gesture that previews them, a card that expands under the finger, the
derived square and the cover over it, the rows and shelves a list is made of,
and the onboarding's screens with no state of their own, which `ark-android`
draws from its steps. Compose only — no ArkDB — and an Android library like
`ark-android`, so a project of the app's build; `checks.kotlin` holds its
sources to ktfmt, and `harken-apk` runs its tests.

## `harken-peer`

`harken-peer-kt` (`dev.harken.peer`, `nix build .#harken-peer-kt`) is
`harken/server/src/bin/harken-peer.rs` on this client: the same flags, the
same JSON-lines commands and answers, the same directory records. It
interprets every function of `harken.ark` — carried in its jar — or of the
`.ark` `--module` names; nothing is native. `checks.fleet-kotlin` runs it
beside Rust peers (`harken/server/tests/fleet_kotlin.rs`,
`docs/plan-fleet.md`).

**The Unicode tables are generated, not copied.** `ark-runtime`'s build
reads `rust/ark/src/unicode_tables.rs` — `gen-unicode`'s output from the
Unicode 16.0.0 Character Database — and writes `UnicodeTables.kt` into
`build/generated`, each table one string of hex parsed once (an array
literal is bytecode in `<clinit>`, which the JVM caps at 64 KB). Moving the
Unicode version is regenerating the Rust file; this build follows.

## What it is held to

    nix build .#checks.x86_64-linux.kotlin     # what nix flake check runs
    nix develop .#kotlin -c gradle --no-daemon --offline check \
        -ParkTraceCorpus=$(nix build .#trace-corpus --print-out-paths)    # from kotlin/

`checks.kotlin` holds every source to `ktfmt --kotlinlang-style` (checked,
never written), then runs `gradle build`, whose `check` runs five plain
`main`s — no test framework is on the classpath:

- **the conformance runner** (`dev.arkdb.conformance.Conformance`,
  `spec/RUNNERS.md`): it walks `spec/vectors/MANIFEST.json`, fails on a file
  the manifest does not name or one it names that is missing, runs every
  `client` vector — `codec`, `order`, `hash`, `module`, `verify`, `eval`,
  `views`, `protocol`, `rebase/three-peers` and `trace` — requires each
  `falsify/` vector to fail its check (a crash is a bug, not a pass), skips
  by name the 8 `rust-only` fleets and the two steps of `three-peers` that
  are a server's (adopting a log, compaction), prints what it ran and
  skipped, and fails on a claimed kind with nothing run: 122 of the 130
  vectors run. It replays every trace of the corpus `-ParkTraceCorpus` or
  `ARK_TRACE_CORPUS` names: `packages.trace-corpus`, 2,565 traces, in the
  check;
- **KeptNumbers**: a plan whose kept lists keep lists of their own, three
  levels deep, through 400 seeded random batches, held after each to a
  fresh hydrate, to `View.read`'s fold and to its patches' splice — past
  what the views vectors reach;
- **StoreHints**: every read through the store's indexes — equalities,
  spans, needles, ordered reads — over a `MemoryStore` and an `Overlay`,
  held to the scan it stands for;
- **ClientTests** (`ark-client`): the link's backoff, stops and resets
  over a queue (the reference's link test), the JSON dialect's printing and
  its refusals, the sign-in's URLs and login, and views over `harken.ark` —
  a peer alone, `playlists` held, the spliced list equal to a fresh query
  after every change;
- **StorageTests** (`ark-client`): `fixtures/rust-peer.json`, written by a
  Rust `ark_client::Peer` (`fixtures/rust-peer.rs`, a scratch program over
  `rust/ark-client` that no build here compiles), replayed into a Kotlin
  `Peer` with the storage compared key for key and byte for byte after
  each of its 73 steps; and every trace again with the client's durable
  copy kept in these records (`StoredCopy`), so a reopen opens what they
  say; and a `DirStorage` on this machine, the directory synced after every
  rename.

The build is offline: `kotlin/deps.json` is the recorded Maven graph,
replayed as a `mitmCache` the way nixpkgs builds every gradle project, and
`nix run .#kotlin-deps` re-records it (it needs the network); the client's
link and `harken-peer` moved nothing in it. The runtime depends on the
Kotlin standard library alone, and uses no JVM API beyond `java.util`,
`java.io`, `java.nio.charset` (the dialect's strict UTF-8),
`java.math.BigInteger` and `java.security.MessageDigest`; the client adds
OkHttp, `java.net`'s `HttpURLConnection` and sockets, and `java.nio.file`
for a directory's sync — all of which Android has at API 26 — so the same
source compiles for the app (`harken/android`).

## What is not here

A server is not here and will not be: no `Server`, no adopting a log, no
compaction, no union's server half. `Auth.login` is the desktop's loopback
flow; a phone signs in through `ark-android`'s Custom Tab and
`Auth.exchange`. No phone has run any of this: `nix build .#harken-apk`
compiles the client and the onboarding for Android inside the app, and
everything above ran on the JVM.

## Where it came from

This tree was frozen at spec v3 while v4 moved (`FROZEN.md`); it was last
green at `4bacc8f`, and `git show 4bacc8f:kotlin` is that runtime, its
authoring vocabulary and its client.
