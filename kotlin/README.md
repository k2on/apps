# The Kotlin runtime

`ark-runtime` (package `dev.arkdb`) is the core of ArkDB's client for the
JVM and Android, at spec version 4: what a phone needs to load a module it
did not author, verify it, interpret it and keep its views up to date. It
is a port of `rust/ark`, which is the specification (`spec/README.md`), and
it is held to the same vectors the Rust is, file for file, by
`checks.kotlin` (`docs/plan-phones.md` S3).

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

Two representations differ from the Rust and say nothing different. A row
is a `Value.VStruct` of every column — the Rust lays one out positionally
behind its table's names (`docs/plan-perf.md` R11), and "laid out as the
table's" is here a struct whose fields are exactly the table's columns. An
index posts keys where the Rust posts ordinals (`docs/plan-db.md` D7.2):
the same rows, in key order.

**The Unicode tables are generated, not copied.** `ark-runtime`'s build
reads `rust/ark/src/unicode_tables.rs` — `gen-unicode`'s output from the
Unicode 16.0.0 Character Database — and writes `UnicodeTables.kt` into
`build/generated`, each table one string of hex parsed once (an array
literal is bytecode in `<clinit>`, which the JVM caps at 64 KB). Moving the
Unicode version is regenerating the Rust file; this build follows.

## What it is held to

    nix build .#checks.x86_64-linux.kotlin     # what nix flake check runs
    nix develop .#kotlin -c gradle --no-daemon --offline :ark-runtime:check   # from kotlin/

`checks.kotlin` holds every source to `ktfmt --kotlinlang-style` (checked,
never written), then runs `gradle build`, whose `check` runs three plain
`main`s — no test framework is on the classpath:

- **the conformance runner** (`dev.arkdb.conformance.Conformance`,
  `spec/RUNNERS.md`): it walks `spec/vectors/MANIFEST.json`, fails on a file
  the manifest does not name or one it names that is missing, runs every
  `client` vector of the kinds it claims — `codec`, `order`, `hash`,
  `module`, `verify`, `eval`, `views` — requires each `falsify/` vector to
  fail its check (a crash is a bug, not a pass), skips by name the
  `rust-only` fleets and the kinds round 3 turns on (`protocol`, `rebase`,
  `trace`), prints what it ran and skipped, and fails on a claimed kind
  with nothing run. 54 of the 129 vectors run;
- **KeptNumbers**: a plan whose kept lists keep lists of their own, three
  levels deep, through 400 seeded random batches, held after each to a
  fresh hydrate, to `View.read`'s fold and to its patches' splice — past
  what the views vectors reach;
- **StoreHints**: every read through the store's indexes — equalities,
  spans, needles, ordered reads — over a `MemoryStore` and an `Overlay`,
  held to the scan it stands for.

The build is offline: `kotlin/deps.json` is the recorded Maven graph,
replayed as a `mitmCache` the way nixpkgs builds every gradle project, and
`nix run .#kotlin-deps` re-records it (it needs the network). The runtime
depends on the Kotlin standard library alone, and uses no JVM API beyond
`java.util`, `java.io`, `java.math.BigInteger` and
`java.security.MessageDigest`, so the same source compiles for Android.

## What is not here yet

`docs/plan-phones.md` round 3: the client — the replica and its rebase,
the protocol client with roles, unions (`scope.rs`'s `Holdings`), private
and raw entries by facts, the alone authority and its journal, storage, the
transport — and with it the `protocol`, `rebase` and `trace` vectors and
`packages.trace-corpus`. Round 4: a JVM `harken-peer` in the fleet, and the
Compose app over this runtime. `ark-client` is the v3 peer over the v3
runtime, kept as it was and out of the gradle build until round 3 brings
it forward. No phone has run any of this, and the Android build is not
exercised here: everything above ran on the JVM.

## Where it came from

This tree was frozen at spec v3 while v4 moved (`FROZEN.md`); it was last
green at `4bacc8f`, and `git show 4bacc8f:kotlin` is that runtime, its
authoring vocabulary and its client.
