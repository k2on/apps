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
| 11 | `Ark.Peer` | the replica: pending intents, the optimistic view, the rebase, applying by intent or by facts, divergence detection; the authority: sequencing, dedupe, verdicts, compaction, retirement, adoption of a scope; `localCommit`, the serverless peer |

Not yet written, in the order they are needed (see `docs/arkdb.md`,
Part 3, for the design each one implements):

| § | module | what it will define |
|---|---|---|
| 12 | `Ark.Protocol` | the frames, as values, and the state machine that sends and receives them, including fan-out to many replicas and the `Need`/`Facts`/`Snapshot`/`Verify` exchanges |
| 13 | `Ark.View` | incremental views: source, filter, join, take, tally; patches; `Rebuilt` |
| 14 | `Ark.Live` | rooms, frames, `keep`, the second-`Hello` rule |
| 15 | `Ark.Sim` | the seeded simulation that emits `rebase/` vectors |

## Building

    cabal build && cabal run ark-vectors -- vectors/

or, with nix, `nix build` / `nix develop`. Without either, `ghc --make
-isrc app/Vectors.hs` is enough; only boot libraries are used.

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
| `rebase/` | a scripted session of replicas and an authority: the mutations, the deliveries, the expected view and confirmed hashes at each step | §10, §11 |

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
