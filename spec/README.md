# The ArkDB specification

Spec version 4. `rust/ark` is the specification: its modules are
normative, and a runtime in any language is conformant when it agrees
with them on the vectors under `vectors/`, which `rust/ark` writes and
`rust/ark/tests/vectors.rs` reads — two programs against one set of
files. This file is the index: which module is normative for which
section, the few requirements that cannot be a function, and the
vectors. The prose that would otherwise be a specification document is
in the doc comments beside the definitions it explains. `AUTHORING.md`
is the contract a domain is written against; `docs/arkdb.md` is the
architecture, and `docs/plan-v4.md` the design of this version.

Until spec v3 the specification was a Haskell program here, and every
rule was written twice. Swift and Kotlin are frozen at v3
(`swift/FROZEN.md`, `kotlin/FROZEN.md`) and come back against the
vectors of the day they do.

## Reading order

| § | `rust/ark/src/` | what it defines |
|---|---|---|
| 1 | `value.rs` | the eight run-time values and the one total order over them |
| 1.3 | `canon.rs` | the canonical encoding: RFC 8949 §4.2.1 deterministic CBOR plus the mapping; the decoder that refuses anything else |
| 2 | `schema.rs` | tables, columns, indexes, references; the derived relationships; well-formedness |
| 3 | `ir/mod.rs` | the function language: mutators, queries, helpers, guards and providers; routers (§3.10); input fields and their checks (§3.11); statements — the three table writes among them — and expressions; the standard library's names |
| 3.3 | `ir/mod.rs` (`Plan`) | a query is a plan and nothing else: source (a table or a group), filter, lookups, related plans, having, projection, order, limit (`docs/plan-v4.md` §1.3) |
| 4 | `store.rs` | the store as a value: `get`, `scan`, `put`, `delete`, and the three writes a body makes of them — insert (§4.3a), upsert (§4.3b), update (§4.3c); constraints as refusals; what a write reports |
| 5 | `stdlib.rs`, `unicode_tables.rs` | the standard library's semantics; the three pinned Unicode tables, generated from UCD 16.0.0 by `bin/gen-unicode.rs` |
| 6 | `eval.rs` | what a mutator means, which every runtime's native procedures are held to: `apply`, evaluation order (checks, middleware, body), checked arithmetic; the form validator (§6.7) and the default messages (§6.8) |
| 7 | `ir/encode.rs`, `ir/normalize.rs` | the module as a value (its wire and storage form); alpha-normalisation (§7.1), a plan's binders with the function's |
| 7.2 | `ir/decode.rs` | a module, a closure, a schema or a type from its value: the inverse of §7, strict about shape |
| 8 | `hash.rs`, `sha256.rs` | the state hash; closures and the function hash, which covers the helpers a function calls and the middleware it runs |
| 9 | `verify.rs` | what a module must satisfy before anything runs or hashes it, a plan's typing and scope included (`docs/plan-v4.md` §1.10) |
| 10 | `log.rs` | an entry; the module's one log with the facts kept beside each entry; snapshots, the horizon, the state at any retained sequence from facts alone; the log's identity, on its snapshot |
| 11 | `peer.rs` | the replica: pending intents, the optimistic view, the rebase, work authored before anyone signed in made the signer's, applying by intent or by facts, divergence detection; the authority: sequencing, dedupe, verdicts, compaction, retirement of closures no retained entry names, adoption of a log; `local_commit`, the serverless peer |
| 12 | `protocol.rs` | the frames as values and their encodings; the client machine and the server machine (authenticate once, hold every entry to its identity, give every refusal a reason, sequence, fan out a page at a time, rooms); the log's identity as `log`, an id, in `hello`, `batch` and `snapshot` — absent where no log is named, which is the one encoding of that (a null is refused), so a frame naming none is the bytes it was and a v4 runtime that never names a log is conformant as it was, while one that names it emits the field; a `hello` naming another log than the authority's is answered with its snapshot at the head, as one past the head is, and one naming none is served as before (`docs/plan-perf.md` Round 4) |
| 13 | `view.rs` | what a plan means (`pull`, the one evaluator of plans: a query, a mutator's `select`, a hydrating view) and a plan kept up to date: entries, dependencies by plan node, patches, the window under a limit, the correctness contract (`docs/plan-v4.md` §1.5) |
| 13.7 | `ir/reads.rs` | what a query's middleware reads, by static inspection: the tables whose change re-runs it before a view is pushed (`docs/plan-v4.md` §1.7) |
| 14 | `live.rs` | rooms per account over opaque frames: arrive, speak, depart; a snapshot kept when a room empties; a repeated hello is paging |
| 15 | `sim.rs` | the seeded fleet: a server, clients, a network that reorders, duplicates and drops; partition, heal, step, settle |
| 17 | `compat.rs` | `arkc check`: the additive-only rule between two modules, and re-verifying retained closures against a new schema |
| — | `authoring/` | the vocabulary of `AUTHORING.md`: a mutator run `Emit` (the module) or `Native`, a query described under `Emit` as its plan |
| — | `bin/vectors/demo.rs` | the two-table domain every vector is over: `AUTHORING.md` Appendix B |

§16 (the printable form) and §18 (`arkc gen`, `arkc roundtrip`) left
with the Haskell and are not in version 4.

## The toolchain

`rust/ark` has three binaries:

- `ark-vectors OUTDIR` writes every vector (`bin/vectors.rs`, a module per
  directory under `bin/vectors/`), asserting each claim as it goes, so a
  change that breaks one fails the generator rather than emitting a wrong
  vector.
- `arkc` is everything about a module rather than about running one. A
  `.ark` file is a module's canonical CBOR, and `arkc` verifies it before
  anything else:

      arkc verify  M          the module hash
      arkc hash    M          the module hash and every function's
      arkc check   OLD NEW    every break of §17, exit 1; or nothing
      arkc vectors OUTDIR     as ark-vectors

- `gen-unicode UCD-DIR OUT` writes `unicode_tables.rs` from the three UCD
  files it names; run by hand when the pinned Unicode version moves.

## Building

    nix run ..#vectors -- vectors/       # regenerate the vectors, from this directory
    nix run ..#arkc -- check old.ark new.ark
    nix develop ..#rust -c bash -c 'cd ../rust && cargo test -p ark'
    nix flake check ..                   # the vectors are what rust/ark writes, and
                                         # rust/ark passes them

## The vectors

`ark-vectors` writes one JSON file per case under `vectors/<directory>/`.
JSON cannot say bytes, ids or 64-bit integers, so values are written with
wrappers: `{"$bytes": "<hex>"}`, `{"$id": "<8-4-4-4-12>"}`,
`{"$int": "<decimal>"}` — an integer is always wrapped, and an object with
exactly one of those keys is a wrapper, so no field name begins with `$`.
Text, booleans and `null` are themselves; a list is an array; a struct is
an object, keys in code-point order. Only `"`, `\` and the C0 controls are
escaped in a string. A file is an object of named parts, one per line,
which is what makes a regeneration that changes nothing show nothing in a
diff (`bin/vectors/json.rs`).

Every vector also carries the expected canonical bytes of the values it
names, so a runtime is tested on encoding as well as on meaning. Every
directory carries at least one deliberately wrong vector under
`falsify/`, marked `"expect": "fail"`, which a conformant runner must
*fail* — a runner that passes by not reading the expected value is caught
by it.

| directory | one vector is | holds |
|---|---|---|
| `codec/` | a value and its canonical bytes | §1.3 |
| `order/` | values and their sorted order | §1.2 |
| `verify/` | a module and whether it verifies | §9 |
| `eval/` | a module, a store, a mutator applied step by step — the changes, the rows, the hash; the input checks as verdicts; the form validator | §6, §8 |
| `hash/` | a store and its hash, with the module it belongs to | §8 |
| `rebase/` | `three-peers`: a scripted session of replicas and an authority, asserted step by step; `fleet-seed-N`: a seeded simulation's script and the hash every replica must reach after settle | §10, §11, §15 |
| `module/` | a module as a value, its canonical bytes, its hash; decode of encode is the identity | §7 |
| `protocol/` | every frame as a value and its bytes; decode of encode is the identity | §12 |
| `views/` | a query of the vector's own module (`query`, and its `plan` as the module writes it), the context and arguments it is read with, `store_before` and the answer at hydrate (`rows_before`); then `batches` of changes, and after each batch the `patches` and the answer (`rows`). One file per plan feature: a projection, a having that admits a node when a child arrives, a group source, a lookup chain, a related plan on a non-key column, an expression order key under a limit, a related tree three deep, and the two v3 plans (`top-two-by-pos`, `playlist-with-items`) | §13 |

A change in a batch is the protocol's fact form (§12): `{"t": "add",
"table", "row"}`, `{"t": "remove", "table", "row"}`, `{"t": "edit",
"table", "old", "new"}`. A batch is applied to the store whole and then
pushed as one, so the patches are §1.5's: touched keys settle in
ascending key order, each against the list as it stands; an entry that
stays in place is an `update` only if its node changed; a move is a
`remove` at the old place then an `insert` at the new; under a limit an
entry leaving the window is a `remove` then an `insert` of the next at
the last place, one entering an `insert` then a `remove` past the limit;
a key added, edited and removed in one batch gives nothing. A patch is
`{"t": "insert", "at", "node"}`, `{"t": "remove", "at"}` or `{"t":
"update", "at", "node"}`, positions into the list as it stands when the
patch is applied.

## Requirements that are not functions

These bind a runtime and cannot be a vector. Each names the design note
that argues it.

- **Durability of a local write.** A pending intent is committed before
  the peer reports the mutation as made, in one atomic commit with nothing
  else; the optimistic state it produced is never committed. One fsync per
  tap is the floor and the design does not go below it. (`docs/arkdb.md`
  §3.6.)
- **A rebase reports the transitions it made.** A replica keeps, beside
  each pending intent, the changes its run made to the optimistic store.
  A rebase undoes them newest first — each inverts exactly: an `add` by
  deleting its key, a `remove` by putting the row back, an `edit` by
  putting the old row — applies what landed, and runs the surviving
  intents again; every view is told the inverse of what it undid, what
  landed and what it re-applied, in that order. Each is a transition the
  store made, never one invented for it. `Rebuilt` is reserved for a store
  replaced whole: a replica opened, a snapshot adopted. (§3.6, §3.13;
  `docs/plan-perf.md` R2.)
- **A live frame never touches the optimistic view.** `Heard` is taken
  before any rebase logic runs. (§3.14.)
- **The transport pings, the engine does not.** An authority's transport
  sends a keepalive on a quiet connection and closes after three
  unanswered; no client is required to. The engine has no clock. (§3.8.)
- **Nothing consults the platform for Unicode.** `trim`, `lower` and
  `is_alnum` use the tables in `unicode_tables.rs` and nothing else. (§5.)
- **A verified module only.** A runtime executes generated code only for a
  module that passed `verify`, and the hash it records for a function is
  of the form `verify` returned. (§9.)
- **Text is compared by code point.** Never by locale, never by canonical
  equivalence, whatever the host's string type does by default. (§1.2.)

## Changing the spec

A change to a module of `rust/ark/src` named above is a change to what
every runtime must do. It is made by editing the module, running
`ark-vectors` and reading the diff of `spec/vectors` — every file that
moved is named in the commit, with why — and moving `ir::SPEC_VERSION`
when a conformant runtime would now be non-conformant. A change that
moves a mutator's hash, or that `arkc check` reports against the module
it replaces, is a break of the log, and is recorded as one. A new rule
gets a vector, and a directory that gains a vector keeps its `falsify/`
case failing.
