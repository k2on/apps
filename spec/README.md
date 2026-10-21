# The ArkDB specification, version 5

**The vectors are normative.** A runtime in another language is
conformant when it agrees with the vectors under `vectors/`. **Rust is the
reference implementation that writes them**: `rust/ark`'s modules, whose
doc comments are the prose, and `rust/ark/tests` and `rust/ark-dev/tests`,
which write the vectors (`ARK_WRITE_VECTORS=1`) and otherwise hold them to
what is on disk; written over an empty `vectors/` they are the tree byte
for byte, which the flake's `vectors` check holds. **A domain is plain
Rust** (§1.16): an entry names its function by name, and code never
crosses the wire; a `views/` vector carries its query as a plan in JSON
(`ark::dynamic`), which a runner in any language reads. This file is the
index; `RUNNERS.md` says what a runner does with the vectors
`rust/ark-dev` writes, and `AUTHORING.md` is writing a domain in Rust.
`docs/plan-v5.md` is the design; where a module and this file disagree,
the module is right. v5 replaced v4 here (R1); v4's specification is in
the history, at `8bda5c3`, the last commit with v4 in the tree.

## §1 Types and bytes

| § | `rust/ark/src/` | what it defines |
|---|---|---|
| 1.1 | `json.rs` | a value is its JSON: `serde_json`, compact, a struct's fields in declaration order, an enum externally tagged, no floats, so one value has one encoding; a key is the same value in the memcomparable encoding (prefix-free, ordered as the values are); bytes and hashes are lowercase hex in JSON; `Typed` serializes a JSON value under a `Type` exactly as the Rust type it describes, so a row without a Rust type has that type's bytes and key; bytes that are JSON (a row, an input) travel as the JSON they are |
| 1.16 | `row.rs` (`Type`, `Input`) | what a type is as a column, a form's field and the explorer read it, records and sums with their names; an input's refinements — `trim`, `non_empty`, `len`, `range`, a predicate — each with the words a form shows (`at least 1 character`, `must not be empty`), checked before a mutator runs: its refusal |
| 1.1 | `hash.rs` | the hash spaces, each its own type |
| 1.2 | `schema.rs` (`compat`, `Table::fit`), `peer.rs` (`open`) | a schema grows by appending: a row of a table as another schema of it wrote it reads under this one completed (an older one's) or cut to its prefix (a newer one's). A log opened under a prefix-extension of the schema it was written under is checked entry against entry by each hash over its own bytes, never derived again; its rows are widened and the store's digests are the new schema's; what it logs then carries new-schema hashes. A replica whose authority runs another schema takes a confirmed entry's state as it says; one that diverges twice at one entry asks for the snapshot (`Hello` at cursor `-1`) |
| 1.6 | `log.rs` (`Origin`, `EntryId`) | an entry's id is `sha256(len(origin) as u64be ‖ origin ‖ local as i64be)[..16]`, `origin` sixteen bytes a replica draws once at its first open and keeps, `local` its count of intents from 1; the `n`-th row id an entry draws is `sha256(id ‖ n as i64be)[..16]`. Neither depends on who is signed in. An authority dedupes and orders by `(origin, local)`, binds an origin to the actor whose intents first came from it, and stamps each entry with the actor, session and roles of the connection it judged it on |

## §7 The kernel

| § | `rust/ark/src/` | what it defines |
|---|---|---|
| 1.3 | `store.rs` | one ordered map per table, rows by key and index entries; every write judges the constraints (canonical row bytes, unique indexes, a reference names a row that exists, a row still named is not removed) and is a `Change` — `Add`, `Remove`, `Edit` — that inverts exactly; the state hash kept incrementally per table, over the durable space only, the schema hash in it; a store has a space; the log is its `_log` table, keyed by sequence, a row a `Logged`'s bytes, its invariant the chain (a row added or removed only at an end it links onto, never edited), outside the schema, the state hash and a snapshot sent |
| 1.6 | `log.rs` | the chained log: `Entry { id, actor, session, origin, local, roles, func, input, now }`, `func` the function's name, `Logged { seq, entry, facts, state, hash }`, the hash the chain to it; the chain is the log's identity; a log stands on its table's first row (a snapshot's head, or the horizon, a delete below); what is durable is one format, the store's rows whole and the writes since (`Record`, `Durable`) |
| 1.7 | `peer.rs` | the one sans-io machine — `Replica` or `Authority { journal }`, alone being an authority with a fork — `step(Input) -> [Output]`: sequencing, dedupe per `(origin, local)`, judging, the rebase (pending lists undone newest-first, the confirmed facts applied, the pending run again, once per page), compaction, going alone and joining (a chain adopted by its facts or refused), a held intent kept pending and what follows it waiting on the authority, `Input::Loaded` judging what was held; a replica applies confirmed entries by their facts; a function is a `Procedure` found by its name in the peer's `Functions` |
| 1.8 | `peer.rs` (`Feed`, `scoped`), `domain.rs` (`Scope`) | partial sync: a scope is, per table, a function of the row and who reads it; an authority keeps a union per scoped connection (`Input::Feed`, the scope by name) and answers it by `Output::To`, a union holding what its rows reference |
| 1.9 | `peer.rs` (the volatile space) | live state: tables marked `Volatile`, a second machine of the same kind over them on the same socket, frames tagged with their `space`; `on_join`, `on_leave` and `on_restore` entries the authority authors; not in the state hash, not journaled |
| 1.10 | `plan.rs`, `dynamic.rs` | maintained views: a plan of operators (`scan`, `filter`, `map`, `sort`, `take`, `group`, `join`) over typed rows, each updated from a delta, a closure's reads recorded and re-run only for what a change hits; after every step the answer equals a fresh run and the patches (`Insert`, `Remove`, `Update` at a position) splice the old list into the new. `dynamic.rs` is the same plans over rows as JSON, written as JSON |
| 1.11 | `frames.rs` | the frames, one enum of four, each frame its JSON: `Hello` (with the schema's hash and the domain's version), `Page` (a peer's intents, or its chain made alone; an authority's log), `Snapshot`, `Verdict` (`Ok`, `Refused`, `Held` naming the function). A peer sends pages of its chain; an authority answers with verdicts and its own pages. No frame carries code |
| 1.16 | `domain.rs`, `db.rs`, `rust/ark-macros` | a domain in plain Rust: rows, inputs and the schema derived; mutators, live transitions, queries, views and scopes registered by name in a `Domain`; reads and writes through a typed `Db`, a write a `Change` the store judges, a refused run undone; `ctx.now` and `ctx.new_id()` a function's only non-determinism |

The drivers are not the specification but the reference reading of it:
`rust/ark-peer` (the one peer, a device that dials — storage, the link,
views for a UI, the live relay — and a server that listens — the
connections, persistence, the sync socket, the admin page's listener) and
`rust/ark-dev` (the demo, the simulator, the
trace recorder and player, the fuzzer, the vector writer).

## The vectors

Every vector is plain JSON, a value its own JSON (§1.1) and a row an
object of its columns; `hash/` is written one named part per line
(`rust/ark/tests/support/json.rs`).
`RUNNERS.md` gives the form of `protocol/`, `trace/` and `rebase/`. Every
directory but `hash/` has a `falsify/` vector a conformant runner must
fail.

| directory | written by | one vector is | holds | since |
|---|---|---|---|---|
| `hash/` | `rust/ark/tests/hash.rs` | a store and its hash under its schema; `leaves-and-digests`, each table's digest and the state hash | §1.1, §1.3 | K1; unmoved by K2; N1 re-derived it, values as JSON |
| `views/` | `rust/ark/tests/views.rs` | a store, a plan (JSON) and a script of batches: after each, the answer and the patches; `top-two-by-pos`, `group-source`, `playlist-with-items` and the rest of v4's shapes, and the maintained sum's overflow both ways | §1.10 | V1; N1 carried each query as a plan and every value as JSON, the patches the language's engine gave, checked step by step before it went |
| `protocol/` | `rust/ark-dev/tests/vectors.rs` | one frame of each kind each side writes, in each space it is sent in: the frame as its JSON | §1.11 | D1; K3a made the frames four; N1 re-derived every one: functions by name, `Hello` with the schema's hash and version, frames as JSON |
| `trace/` | `rust/ark-dev/tests/vectors.rs` | one client's life in one session, step by step: what its two machines were given and said, and where they stood; its domains by name | §1.7, §1.9 | D1; N1 re-derived all: functions by name, then values as JSON, which moved every hash |
| `rebase/` | `rust/ark-dev/tests/vectors.rs` | a fleet run by the simulator from a seed (Rust's only: `RUNNERS.md`): `fleet-seed-7`, its script and where the hub stands settled, and `fleet-fuzz-seed-N` sessions of the fuzzer, each where it stands settled | §1.6, §1.7 | D1; N1: `fleet-seed-7`'s script names functions by name, its head held and its state re-derived with values as JSON (`8316d1f4…`, was `ff04f56d…`); the fuzz sessions are the new fuzzer's, over the demo |
