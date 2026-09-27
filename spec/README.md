# ark-spec

The ArkDB specification, as a Haskell program.

Every module under `src/Ark/` is normative. A runtime in any language is
conformant when it agrees with this program on the vectors `ark-vectors`
emits into `vectors/`. The prose that would otherwise be a specification
document is in the comments beside the definitions it explains; this file
is the index, and the home of the few requirements that are not functions.

The program is written to be read. It depends on nothing but a compiler
and GHC's boot libraries, uses no language extensions beyond
`OverloadedStrings` and `LambdaCase`, and prefers the obvious definition
to the fast one throughout: `scan` filters and sorts, `Store` is a map of
maps, a rebase is recomputing a value. What it must be is exact.

## Reading order

| § | module | what it defines |
|---|---|---|
| 1 | `Ark.Value` | the eight run-time values and the one total order over them |
| 1.3 | `Ark.Canon` | the canonical encoding: RFC 8949 §4.2.1 deterministic CBOR plus the mapping; the decoder that refuses anything else |
| 2 | `Ark.Schema` | scopes, tables, columns, indexes, references; the derived relationships; well-formedness |
| 3 | `Ark.IR` | the function language: mutators, queries, helpers; statements, expressions, plans, the standard library's names |
| 4 | `Ark.Store` | the store as a value: `get`, `scan`, `put`, `delete`; constraints as refusals; what a write reports |
| 5 | `Ark.Std`, `Ark.Std.Unicode` | the standard library's semantics; the three pinned Unicode tables, generated from UCD 16.0.0 by `tools/GenUnicode.hs` |
| 6 | `Ark.Eval` | what generated code must mean: `apply`, `query`, evaluation order, checked arithmetic, `select` |
| 7 | `Ark.Encode` | the module as a value (its wire and storage form) and alpha-normalisation |
| 8 | `Ark.Hash`, `Ark.Sha256` | the state hash; closures and the function hash, which covers the helpers a function reaches; SHA-256 itself, so the spec imports nothing |
| 9 | `Ark.Verify` | what a module must satisfy before anything runs, generates from or hashes it |
| 10 | `Ark.Log` | an entry; a scope's log with the facts kept beside each entry; snapshots, the horizon, the state at any retained sequence from facts alone |
| 11 | `Ark.Peer` | the replica: pending intents, the optimistic view, the rebase, applying by intent or by facts, divergence detection; the authority: sequencing, dedupe, verdicts, compaction, retirement of closures no retained entry names, adoption of a scope; `localCommit`, the serverless peer |
| 7.2 | `Ark.Decode` | a module, a closure, a schema or a type from its value: the inverse of `Ark.Encode`, strict about shape |
| 12 | `Ark.Protocol` | the frames as values and their encodings; the client machine (subscriptions, hello, push, pages, facts, snapshots, closures, verify) and the server machine (authenticate once, hold every entry to its identity, sequence, fan out a page at a time, rooms) |
| 13 | `Ark.View` | incremental views over a plan: hydrate, push, patches, refill under a limit, child changes as parent updates, the correctness contract; a maintained count is a view's length |
| 14 | `Ark.Live` | rooms per account over opaque frames: arrive, speak, depart; a snapshot kept when a room empties; a repeated hello is paging |
| 15 | `Ark.Sim` | the seeded fleet: a server, clients, a network that reorders, duplicates and drops; partition, heal, step, settle |

| 16 | `Ark.Print` | the printable, diagnostic form of a module — what `arkc print` writes and a diff shows; not canonical |
| 17 | `Ark.Compat` | `arkc check`: the additive-only rule between two modules, and re-verifying retained closures against a new schema |
| 18 | `Ark.Gen` | `arkc gen`: one emitter over the IR, three spellings — Rust, Swift, Kotlin — over the library `GENERATED.md` names |
| — | `Ark.Demo` | the two-table domain every vector and every runtime's first test is built on |

Everything the design in `docs/arkdb.md` Part 3 names is written, and so is
the toolchain: `arkc` (`app/Arkc.hs`) verifies, prints, hashes, checks and
generates from a `.ark` file, which is a module's canonical CBOR. The
runtimes (`../rust`, `../swift`, `../kotlin`) and the apps are held to this
package by the vectors and by `GENERATED.md`.

## Building

    nix build ..#ark-spec            # from this directory; the flake is the repository's
    nix run ..#vectors -- vectors/   # regenerate the vectors
    nix run ..#arkc -- gen rust m.ark out/ --name Harken
    nix develop ..#spec              # a shell with GHC and cabal
    nix flake check ..               # the vectors are what the spec writes; every
                                     # runtime passes them; harken's module and
                                     # generated code are what arkc writes

Only GHC's boot libraries are used, so `ghc --make -isrc app/Vectors.hs`
inside that shell is also enough.

## The vectors

`ark-vectors` writes one JSON file per case under `vectors/<directory>/`.
JSON cannot say bytes, ids or 64-bit integers, so values are written with
wrappers: `{"$bytes": "<hex>"}`, `{"$id": "<8-4-4-4-12>"}`,
`{"$int": "<decimal>"}`; text, booleans and `null` are themselves; a list
is an array; a struct is an object. Every vector also carries the expected
canonical bytes of the values it names, so a runtime is tested on
encoding as well as on meaning. Each directory carries at least one
deliberately wrong vector under `falsify/`, which a conformant runner
must *fail* — a runner that passes by not reading the expected value is
caught by it.

| directory | one vector is | holds |
|---|---|---|
| `codec/` | a value and its canonical bytes; inputs to refuse | §1.3 |
| `order/` | values and their sorted order | §1.2 |
| `std/` | a call, its result or fault | §5 |
| `verify/` | a module and whether it verifies | §9 |
| `eval/` | a module, a store, an entry; the changes, the rows, the hash | §6, §8 |
| `hash/` | a store and its hash | §8 |
| `rebase/` | `three-peers`: a scripted session of replicas and an authority, asserted step by step; `fleet-seed-N`: a seeded simulation's script and the hash every replica must reach after settle | §10, §11, §15 |
| `module/` | a module as a value, its canonical bytes, its hash; decode of encode is the identity | §7 |
| `protocol/` | every frame as a value and its bytes; decode of encode is the identity | §12 |
| `views/` | a plan, initial rows, a change list, expected patches and rows | §13 |

## Requirements that are not functions

These bind a runtime and cannot be a vector. Each names the design note
that argues it.

- **Durability of a local write.** A pending intent is committed before
  the peer reports the mutation as made, in one atomic commit with nothing
  else; the optimistic state it produced is never committed. One fsync per
  tap is the floor and the design does not go below it. (`docs/arkdb.md`
  §3.6.)
- **The overlay reports nothing when dropped.** A rebase tells every view
  `Rebuilt`; no runtime may synthesise changes for a rollback. (§3.6,
  §3.13.)
- **A live frame never touches the optimistic view.** `Heard` is taken
  before any rebase logic runs. (§3.14.)
- **The transport pings, the engine does not.** An authority's transport
  sends a keepalive on a quiet connection and closes after three
  unanswered; no client is required to. The engine has no clock. (§3.8.)
- **Nothing consults the platform for Unicode.** `trim`, `lower` and
  `is_alnum` use the tables in `Ark.Std.Unicode` and nothing else. (§5.)
- **A verified module only.** A runtime executes generated code only for a
  module that passed `Ark.Verify.verify`, and the hash it records for a
  function is of the form `verify` returned. (§9.)
- **Text is compared by code point.** Never by locale, never by canonical
  equivalence, whatever the host's string type does by default. (§1.2.)

## Changing the spec

A change to any module here is a change to what every runtime must do.
It is made by editing the module, regenerating the vectors, and moving
`Ark.IR.specVersion` when a conformant runtime would now be
non-conformant. Renaming a field, adding a constructor or changing a rule
fails to compile everywhere it matters, which is the reason this is a
program.
