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
| 1.2 | `schema.rs` | tables, columns, keys, indexes, references, spaces; append-only growth (`compat`); a row as another schema of its table wrote it read under this one, completed or cut to its prefix (`Table::fit`), wherever a store takes rows or facts from elsewhere (`Store::apply_facts`) |
| 1.3 | `store.rs` | one ordered map per table, rows by key and index entries; every write judged (the row's one encoding, unique indexes, references at both ends) and a `Change` that inverts exactly; the state hash, `sha256(schema_hash ‖ per-table sums of sha256(table ‖ row))` over one space, kept per write; the log as the `_log` table, its invariant the chain |
| 1.6 | `log.rs` | an entry (`Ctx`: an intent as recorded, all a function knows of it, and, with no intent in it, who someone is), `Logged`, the chain `sha256(prev ‖ enc(seq, entry, facts, state))`; ids from `(origin, local)`, row ids the entry's draws; `Snapshot`; a fill's `Cursor`; what a peer makes durable and opens from (`Record`), a device's pending intents beside it (`Record::Pending`) |
| 1.7–1.9 | `peer.rs` | the one sans-io machine, replica or authority: sequencing, dedupe, judging, the rebase, compaction (in either role, up to the head: a replica's pending intents are rebased by their own changes, so it folds as an authority does), every push answered by where its origin stands (`At { seq, last }`) or refused, a push saying where its device's pending intents begin (its page's `from`) and what its origin had before them passed, so an intent keeps its number, opening a log (each entry read once; a grown schema keeps it) and the pending intents beside it (their changes put back, not run again, where they were made); a device with no server a replica not connected, its intents pending; a fill (a replica the log cannot page sent the rows in parts — begun again, from nothing, where facts do not apply — each asked for by a `Hello` saying where the fill stands once the one before is held, as they stand when it is sent, put as they are; a domain's `later` tables last; an entry logged meanwhile put as it is, the state checked once the last part is in; a fill made durable part by part, reopened where it was); partial sync (`Peer::feed`, a connection's union `Served`); an authority's own facts (an intent naming no function); compaction by the machine's own rule (`Fold`); the volatile space, under the same rules (an authority restarted from nothing fills its replicas and passes what their pushes say they no longer hold) |
| 1.10 | `plan.rs` | maintained views: `scan`, `filter`, `map`, `sort`, `take`, `group`, `join` over typed rows, each kept from deltas, a closure's reads recorded and re-run only for what a change hits; the answer a fresh run's, the patches splicing |
| 1.16 | `dynamic.rs` | a store's human form: a row as JSON under the schema and back, and a store whole as its tables of rows (an export), made a store again with every row judged (an import) |
| 1.11 | `frames.rs` | the frames — `Hello`, `Page` (a peer's intents, or an authority's chain), `Verdict`, `Part` (a fill's: the rows of each table from where it begins to where the next does; a `Hello` says where the replica's fill stands, and how big a part, or a page, it would have), `Call` and `Reply` (a server's own function a device calls, outside both machines; what a device is and what it said of itself ride calls nobody answers, `ark.device` and `ark.events`), and `Offer`, `Want`, `Chunk`, `Have`, `Absent` (the bytes of the files rows name, beside the log) — in the value format; no frame carries code |
| 1.16 | `row.rs` | `Row`, `Input` and its refinements (the form's words), `Describe` and `Type`, `Id`, typed indexes |
| 1.16 | `db.rs` | the typed store a function reads and writes, a run reading its own writes, a refusal undoing them |
| 1.16 | `domain.rs` | a `Domain`: the schema and functions by name — mutators (of the space each writes), queries, views, scopes |
| — | `file.rs` | `FileRef`, how a row names a file: the hex sha256 of its bytes, their size and media type, an ordinary record column; the files a value names (`named`), and whether a store holds a row naming one (`held`, who may read a file) |
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
format — `kv`, `log`, `local.kv`, `local.log` per peer (and a
device's pending intents, `local.pending`), under a
server's `log/` (its journal and its volatile space as it last closed
beside them, `local.journal` and `local.volatile`); a server's `session.key` beside it (`ark-auth`). The
fixture `rust/ark-peer/tests/data/server/` is a server's directory with a
device's replica in it, written once by an older build and opened by
`rust/ark-peer/tests/old_data.rs`, which reads back its rows, its chain
and its sign-in. A change to the on-disk format writes the fixture again,
with a note in `old_data.rs` of what moved: logs are started afresh, by
decision, rather than migrated. Only a server's rows must cross a build: a
device's directory may break, as it refills from its server. A server's
directory written by main before the restructure (which this build does
not read) crosses by `tools/from-main/migrate.sh`: its rows exported by
that build, imported by this one, and checked row for row.

Every disk keeps the schema its rows were written under beside them,
`schema.json` (`rust/ark-peer/src/disk.rs`). A schema only grows (a table
appended, or a column that is an `Option` or defaulted), so a peer reads
the rows and facts of any build before or after its own; an authority
whose disk's schema its own does not grow from does not open, and a
replica's starts empty, its pending intents and origin kept. Data moves
within a schema by `arkdb export`, a transform of the rows as JSON, and
`arkdb import` into an empty directory.
