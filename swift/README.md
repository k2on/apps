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
the printed domain are gone (`FROZEN.md`).

Round 5 of `docs/plan-phones.md` landed the core below and round 6 the
client — the replica, the protocol client, the alone authority and the
journal in `ArkDB`, and storage and a peer in `ArkDBClient`; the peer's
transport and the iOS sources are round 7.

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

`ArkDBClient` is `rust/ark-client`:

| file | `rust/ark-client/src` | what it is |
|---|---|---|
| `Storage.swift` | `storage.rs` | `Storage` (memory, a directory written whole and synced), the replica, journal, pending and login records, `load` |
| `Writer.swift` | `peer.rs` (`persist`…) | what is written when: a page or a snapshot, for the journal and for the intents |
| `Autos.swift` | `autos.rs` | ids and times, from the platform or a seed |
| `Peer.swift` | `peer.rs` | a peer over a storage: open, sign in and out, mutate, query, check, standing, leave and join, the sans-io frames |

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
  `protocol`, `rebase` and `trace`, 122 of 130, skipping only the eight
  rust-only fleets — requires each `falsify/` vector to fail its check (a crash is a bug, not
  a pass), skips the rest by name with the reason, prints what it ran and
  skipped, and fails on a claimed kind with nothing run;
- **the trace corpus**: every trace of `packages.trace-corpus` (2,565),
  replayed;
- **the storage**: round 3's Rust-written peer fixture
  (`kotlin/ark-client/fixtures/rust-peer.json`, read in place) replayed into
  a Swift `Peer`, every record compared byte for byte after every step;
  `DirStorage` on this machine; and every trace and the corpus again with
  the client's durable copy kept in the storage records;
- **StoreHints**: 600 reads through the store's indexes, over a
  `MemoryStore` and an `Overlay`, each the scan it stands for;
- **KeptNumbers**: kept numbers three levels deep through 400 seeded
  batches, each a fresh hydrate, a fold's answer and a splice.

`tools/swift.sh` runs SwiftPM inside `devShells.swift` with
`tools/nix-swiftc` as `SWIFT_EXEC` (the platform's target triple and clang
resource headers) and the Swift runtime on `LD_LIBRARY_PATH`, and generates
the Unicode tables first.

## Where it came from

This tree was frozen at spec v3 while v4 moved (`FROZEN.md`); it was last
green at `4bacc8f`, and `git show 4bacc8f:swift` is that runtime, its
authoring vocabulary and its client.
