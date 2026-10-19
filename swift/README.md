# The Swift runtime

`ArkDB` is ArkDB's client for Swift — iOS and macOS, and Linux, where it is
built and tested here — at spec version 4: what a phone needs to load a
module it did not author, verify it, interpret it and keep its views up to
date. It is a port of `rust/ark`, which is the specification
(`spec/README.md`), file for file, and it is held to the same vectors the
Rust and Kotlin are by `checks.swift` (`docs/plan-phones.md` S3). The
Kotlin runtime (`kotlin/README.md`) is the nearest worked reading of each
file and was used as one; the semantics are Rust's.

A phone's runtime is a **client**: it interprets modules and does not
author them. The domain is written once, in Rust, and `arkc` emits the
module (`harken/domain/harken.ark`). The v3 tree's authoring vocabulary and
the printed domain are gone (below).

Rounds 5–7 of `docs/plan-phones.md` landed it: the core, then the client —
the replica, the protocol client, the alone authority and the journal in
`ArkDB`, storage and a peer in `ArkDBClient` — then the peer's link and
sign-in, `harken-peer-swift` in the fleet, and the iOS app rewired onto it
(`harken/ios`, never built for iOS).

## What is here

| file | `rust/ark/src` | what it is |
|---|---|---|
| `Value.swift` | `value.rs` | the eight values, `Id`, `Record` (a struct's fields in code point order), the total order |
| `Canon.swift`, `Sha256.swift` | `canon.rs`, `sha256.rs` | canonical CBOR, strict, every `DecodeError` by the reference's name; SHA-256 in plain Swift (Linux has no CryptoKit) |
| `Json.swift` | `json.rs` | the dialect vectors and a headless peer's lines are written in |
| `Show.swift` | `store.rs` (Display) | a text as Rust's `{:?}` writes it, which refusals are made of |
| `Collections.swift` | — | a sorted map and set, an insertion-ordered map, and `Wide`, a 128-bit integer |
| `Schema.swift` | `schema.rs` | tables, columns, keys, indexes, references, text indexes (D4), `check_schema` |
| `IR.swift` | `ir/mod.rs` | IR v4: plans, scopes and holds, private blocks, `has_role`, routers; `strip` |
| `Decode.swift`, `Encode.swift` | `ir/decode.rs`, `ir/encode.rs` | a module from and to its value; `calls`, `reaches` |
| `Normalize.swift` | `ir/normalize.rs` | symbols renumbered in evaluation order |
| `Hash.swift` | `hash.rs` | `leaf`, the additive `Digest`, `state_hash`, `closure`, `function_hash`, `stripped`, `reads_roles`, `module_hash` |
| `Verify.swift`, `ScopeCheck.swift` | `verify.rs`, `scope.rs` | every complaint by the reference's name; the verifier's half of scopes, and `device_schema` |
| `Store.swift` | `store.rs` | the rules, `MemoryStore` with its secondary, reference and trigram indexes and per-table digest, the `Overlay` |
| `Eval.swift`, `Std.swift` | `eval.rs`, `stdlib.rs` | the interpreter and the standard library |
| `View.swift`, `Maintain.swift` | `view.rs` | `pull`, `answer`, `read`; `hydrate`, `push_all`, patches, R9's kept numbers |
| `Raw.swift`, `Facts.swift` | `raw.rs`, `protocol.rs` | raw writes as a client replays them; the change form |
| `Log.swift` | `log.rs` | an entry with its roles frozen, and the log a peer alone keeps |
| `Peer.swift` | `peer.rs` | the `Replica` — pending, the rebase by records, settle, verdicts, unions, private and raw entries, fork back — and the `Authority` a peer alone is |
| `Protocol.swift` | `protocol.rs` | every frame to and from its value, and the `Client` |
| `Journal.swift` | `journal.rs` | a log's snapshot and pages, in both layouts |
| `Trace.swift` | `trace.rs` | the trace player: inputs, the durable copy, reopen, alone and join |
| `Types.swift`, `Ordering.swift` | `types.rs`, `ir/ordering.rs` | the type section and a value held to it; which transitions need an order (`docs/plan-live.md`) |
| `Live.swift` | `live/state.rs` | live state: instances, devices, calls, the live frames' parts, the `Runner`, a device's `Watch` |

`ArkDBClient` is `rust/ark-client`:

| file | `rust/ark-client/src` | what it is |
|---|---|---|
| `Storage.swift` | `storage.rs` | `Storage` (memory, a directory written whole and synced), the replica, journal, pending and login records, `load` |
| `Writer.swift` | `peer.rs` (`persist`…) | what is written when: a page or a snapshot, for the journal and for the intents |
| `Autos.swift` | `autos.rs` | ids and times, from the platform or a seed |
| `Peer.swift` | `peer.rs` | a peer over a storage: open, sign in and out, mutate, query, check, standing, leave and join, connect and pump, status, the sans-io frames; live state — `live`, `call`, the throttled call, `serverOffset` and `serverNow`, `takeLiveRefusals` |
| `ClientView.swift` | `view.rs` | a query held as a list: hydrated once, patched, its middleware re-run when its `Ctx` or a table it reads moves |
| `Link.swift` | `link/mod.rs` | the link: dial, backoff, stop; `Queues` for a test; `platformDial` |
| `WebSocket.swift` | `link/native.rs` | `NativeSocket` (Linux) and `SessionSocket` (Apple), and RFC 6455's wire |
| `Auth.swift` | `ark-auth` `client.rs`, `util.rs` | `Login`, the URLs, `devLogin`, `exchange`, and on Linux `login` with its loopback port |

`Reads.swift` in `ArkDB` is `ir/reads.rs`, what a view's middleware reads.

**The socket is ours on Linux, and the platform's on Apple.** Linux
Foundation 5.8 declares `URLSessionWebSocketTask`, and a `ws://` task fails
at once with `NSURLErrorUnsupportedURL` (-1002) — probed against a plain
RFC 6455 echo server. So `NativeSocket` is the reference's native transport
ported over POSIX sockets: a thread, the TCP connection and the handshake
each held to the connect patience, 20 ms read slices between which what is
queued is sent, a ping every `pingEveryMs`, the connection given up after
three of those with nothing heard, `ws://` only — TLS would be a
dependency, and the reference's build without its `tls` feature refuses
`wss://` the same way. On iOS and macOS `platformDial` takes
`SessionSocket`, `URLSessionWebSocketTask` with a ping timer: type-checked
here, never run. Sign-in is `URLSession` everywhere, which on Linux is
libcurl and does plain HTTP. No dependency was added.

`harken-peer-swift` (`Sources/HarkenPeer`, `nix build .#harken-peer-swift`)
is `harken/server/src/bin/harken-peer.rs` on this client — the same flags,
the same JSON lines, the same directory records — and interprets every
function of `harken.ark`, installed beside it, or of `--module`'s file.
`checks.fleet-swift` runs it beside Rust peers (`docs/plan-fleet.md` §6).

Three things are Swift's own, and each says nothing different from the
Rust:

- **Text is compared by its UTF-8 bytes, never by `String`.** Swift's
  `String ==` and `<` are canonical equivalence: "é" precomposed and
  decomposed are one `String` and two values. Every equality, order, hash,
  prefix and search of a value's text here is of its bytes, which for valid
  UTF-8 is code point order.
- **Nothing observable iterates a `Dictionary`**, whose order is seeded per
  process. The Rust's `BTreeMap`s are `SortedMap`s — sorted arrays, copied
  on write, so a store's copy costs nothing until it is written — and its
  `Vec`s of pairs are arrays or `OrderedDict`s.
- **`Wide` is Rust's `i128`**, which Swift 5.8 lacks: a plan node's sum and
  R9's kept counts and sums are judged on their total, and a sum of 64-bit
  terms fits.

A row is a `Record` of every column, where the Rust lays one out
positionally (R11), and index postings are keys, not ordinals (D7.2): the
same rows, in key order.

**The Unicode tables are generated, not copied.** `tools/gen-unicode.sh`
reads `rust/ark/src/unicode_tables.rs` — `gen-unicode`'s output from the
Unicode 16.0.0 Character Database — and writes
`Sources/ArkDB/UnicodeTables.swift` (gitignored) before every build, each
table one multi-line literal parsed once.

## What it is held to

    nix build .#checks.x86_64-linux.swift          # what nix flake check runs
    cd swift && nix develop ..#swift -c tools/swift.sh run ArkDBTests ../spec/vectors

`checks.swift` builds the package with SwiftPM in release over nixpkgs'
Swift 5.8 and runs `ArkDBTests`, an executable (there is no XCTest here):

- **the conformance runner** (`spec/RUNNERS.md`): it walks
  `spec/vectors/MANIFEST.json`, fails on a file the manifest does not name
  or one it names that is missing, runs every vector of the kinds it claims
  — `codec`, `order`, `hash`, `module`, `verify`, `eval`, `views`,
  `protocol`, `rebase` and `trace`, 140 of 148, skipping only the eight
  rust-only fleets — requires each `falsify/` vector to fail its check (a crash is a bug, not
  a pass), skips the rest by name with the reason, prints what it ran and
  skipped, and fails on a claimed kind with nothing run;
- **the trace corpus**: every trace of `packages.trace-corpus` (2,565),
  replayed;
- **the storage**: round 3's Rust-written peer fixture
  (`kotlin/ark-client/fixtures/rust-peer.json`, read in place) replayed into
  a Swift `Peer`, every record compared byte for byte after every step;
  `DirStorage` on this machine; and every trace the manifest names, and
  the corpus, again with the client's durable copy kept in the storage
  records;
- **the driver half** (`ClientTests`): the link's backoff over `Queues`,
  the dialect, the sign-in's URLs, views over `harken.ark` spliced against
  a fresh read, RFC 6455's own examples, and `NativeSocket` against a
  loopback server in the process;
- **live state** (`LiveTests`, `docs/plan-live.md`): a peer's calls,
  counters, resyncs, refusals, throttle and the server's clock over the
  demo's room, against frames written by hand;
- **the printed room** (`Printed`): `DemoRoom.swift`, a copy of what `arkc
  gen swift` prints (`spec/vectors/gen/Room.swift`) held to be the same
  text, and every state of `eval/live-room.json` read through it and
  written back as the same value;
- **the iOS app's non-SwiftUI files** (`harken/ios/tools/typecheck.sh`),
  type-checked against the modules just built;
- **StoreHints**: 600 reads through the store's indexes, over a
  `MemoryStore` and an `Overlay`, each the scan it stands for;
- **KeptNumbers**: kept numbers three levels deep through 400 seeded
  batches, each a fresh hydrate, a fold's answer and a splice.

`tools/swift.sh` runs SwiftPM inside `devShells.swift` with
`tools/nix-swiftc` as `SWIFT_EXEC` (the platform's target triple and clang
resource headers) and the Swift runtime on `LD_LIBRARY_PATH`, and generates
the Unicode tables first.

## Where it came from

This tree was ArkDB for Swift at spec v3, with an authoring vocabulary
and harken's printed domain, and was set aside while v4 moved. It was last
green at `4bacc8f`: `git show 4bacc8f:swift` is that runtime, and
`nix build "git+file://$PWD?rev=$(git rev-parse 4bacc8f)#checks.x86_64-linux.swift"`
builds its check as it passed. `docs/plan-phones.md` rounds 5–7 brought it
to v4 as a client; the vocabulary and the printed domain were deleted, not
ported.
