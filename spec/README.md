# The ArkDB specification, version 5

**Rust is the one implementation** (`docs/plan-v5.md` §1.16): `rust/ark`
is the kernel, a domain is a crate of plain Rust linked into it, and the
phones run the same kernel through FFI (`rust/ark-ffi`). **The
specification is what that kernel's bytes and behaviour are held to**,
written down as vectors under `vectors/`:

- **the bytes** — the value format, the keys, the state hash and the
  chain (`hash/`, and every vector below, which carries them);
- **the machine** — a fleet replayed (`rebase/`) and one client's life
  input by input (`trace/`);
- **the frames** — every frame of each kind, each side, each space
  (`protocol/`);
- **the views** — a named view of a reference domain, its input, a store
  and a script of row changes, the answer and the patches after each
  (`views/`).

The vectors are the kernel's regression suite and the contract its other
builds keep: the wasm build the browser runs and the FFI library the
phones embed are the same source, and a vector that moves is a change to
every peer that will ever read an older one's bytes. `rust/ark/tests` and
`rust/arkdb/tests` write them (`ARK_WRITE_VECTORS=1`) and otherwise hold
them to what is on disk; written over an empty `vectors/` they are the
tree byte for byte (the flake's `vectors` check). Each module's doc
comment is the prose; where a module and this file disagree, the module
is right. `RUNNERS.md` says what `arkdb` does with `protocol/`, `trace/`,
`rebase/` and `views/`; `AUTHORING.md` is writing a domain.

## The modules

| § | `rust/ark/src/` | what it defines |
|---|---|---|
| 1.1 | `format.rs` | the value format, postcard, that rows, inputs, entries, frames, snapshots, journal records and the hashed bytes are written in, one encoding per value; a key is the same value memcomparable (prefix-free, ordered as the values are); JSON is the human form (hashes and ids hex; a row inside a frame its hex) |
| 1.1 | `json.rs` | a value without a Rust type: JSON under a `Type`, written as the Rust type it describes would be (`Typed`) and read back from those bytes (`Seed`) |
| 1.1 | `hash.rs` | the hash spaces, each its own type: `StateHash`, `EntryHash`, `SchemaHash` |
| 1.2 | `schema.rs` | tables, columns, keys, indexes, references, spaces; append-only growth (`compat`); a row as another schema of its table wrote it read under this one, completed or cut to its prefix (`Table::fit`) |
| 1.2 | `migrate.rs` | rows of a schema the domain's does not grow from read through its migrations: steps, each naming the schema it reads, over every row as JSON, written under the next step's schema or the domain's |
| 1.3 | `store.rs` | one ordered map per table, rows by key and index entries; every write judged (the row's one encoding, unique indexes, references at both ends) and a `Change` that inverts exactly; the state hash, `sha256(schema_hash ‖ per-table sums of sha256(table ‖ row))` over one space, kept per write; the log as the `_log` table, its invariant the chain |
| 1.6 | `log.rs` | `Entry`, `Logged`, the chain `sha256(prev ‖ enc(seq, entry, facts, state))`; ids from `(origin, local)`, row ids the entry's draws; `Snapshot`; what a peer makes durable (`Record`, `Durable`) |
| 1.7–1.9 | `peer.rs` | the one sans-io machine, replica or authority (alone is an authority with a fork): sequencing, dedupe, judging, the rebase, compaction (in either role, up to the head: a replica's pending intents are rebased by their own changes, so it folds as an authority does), going alone and joining (the chain made alone pushed as intents), a held intent, opening a log (each entry read once; a grown schema keeps it); partial sync (`Feed`, `scoped`); the volatile space (its authority keeping the verdicts of each origin's last 64 `local`s, a push below them overtaken) |
| 1.10 | `plan.rs` | maintained views: `scan`, `filter`, `map`, `sort`, `take`, `group`, `join` over typed rows, each kept from deltas, a closure's reads recorded and re-run only for what a change hits; the answer a fresh run's, the patches splicing |
| 1.16 | `dynamic.rs` | a row without a Rust type as JSON under the schema, its bytes and its key's; a table browsed (`Browse`: equal columns, a key range, a sort by a column, the first `n`) as a plan of the same operators, kept by patches |
| 1.11 | `frames.rs` | the four frames — `Hello`, `Page` (a peer's intents, or an authority's chain), `Snapshot`, `Verdict` — in the value format; no frame carries code |
| 1.16 | `row.rs` | `Row`, `Input` and its refinements (the form's words), `Describe` and `Type`, `Id`, typed indexes |
| 1.16 | `db.rs` | the typed store a function reads and writes, a run reading its own writes, a refusal undoing them |
| 1.16 | `domain.rs` | a `Domain`: the schema and functions by name — mutators, live transitions, queries, views, scopes |
| — | `lib.rs` | `Refusal` and the prelude a domain uses; the derives and attributes are `rust/ark-macros` |

The drivers are the reference reading, not the specification:
`rust/ark-peer` (the one peer, dialling or listening: storage, the link,
the hub, the app kit) and `rust/arkdb` (the demo, the simulator, the
trace player, the fuzzer, the vector writer).

## The vectors

Every vector is JSON, a value its human form (§1.1); `hash/` is one named
part per line. Every directory but `hash/` has a `falsify/` vector the
writer's own runner must fail.

| directory | written by | one vector is | holds |
|---|---|---|---|
| `hash/` | `rust/ark/tests/hash.rs` | a store under its schema: each row's bytes and leaf, each table's digest, the state hash | §1.1, §1.3 |
| `views/` | `rust/arkdb/tests/views.rs` | a view named in a reference domain (`rust/arkdb/src/library.rs`), its input, a store and batches of row changes: after each, the answer and the patches | §1.10 |
| `protocol/` | `rust/arkdb/tests/vectors.rs` | one frame of each kind each side writes in each space: its bytes, and its JSON | §1.11 |
| `trace/` | `rust/arkdb/tests/vectors.rs` | one client's life in one session: what its two machines were given and said, and where they stood | §1.7, §1.9 |
| `rebase/` | `rust/arkdb/tests/vectors.rs` | a fleet run by the simulator from a seed, and the fuzzer's sessions that once found a bug: where each stands settled | §1.6, §1.7 |

## The data directory

What a peer keeps on disk is a contract too: a new build opens the
directory an old one wrote. `disk.rs` (`rust/ark-peer/src/`) is its
format — `kv`, `log`, `local.kv`, `local.log` per peer, under a
server's `log/`; a server's `session.key` beside it (`ark-auth`). The
fixture `rust/ark-peer/tests/data/server/` is a server's directory with a
device's replica in it, written once by an older build and opened by
`rust/ark-peer/tests/old_data.rs`, which reads back its rows, its chain
and its sign-in. **A change to the on-disk format comes with a migration
that opens the fixtures as they are, or with a new fixture beside them
and a note here of what moved and what reads the old one**; a fixture is
never written over.

Every disk keeps the schema its rows were written under beside them,
`schema.json` (`rust/ark-peer/src/migrate.rs`). Opened under a schema
that does not grow from it, an authority's rows are migrated by its
domain's steps (`migrate.rs` above) and written as a snapshot one past
its head, what it stood on kept as `kv.<hash>` and `log.<hash>`; with no
step that reads them it does not open. A replica's start empty, its
pending intents and origin kept, and are sent again. The fixture's
replica predates `schema.json`, so it now opens empty, its origin kept.
