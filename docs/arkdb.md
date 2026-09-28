# ArkDB

A design for the successor to Petros. Three parts: what Petros is and what
harken does with it, what the compiled-domain-over-FFI approach costs now that
the phone is becoming two native apps, and the architecture of a system in
which the domain is *data* rather than a binary — authored through a builder
in Rust, Swift or Kotlin, carried in one specified encoding, compiled to
native source for every language, and run by a runtime in each language that
is held to one conformance suite. A local peer with no server is the ordinary
case of it, not a mode; a server is a peer that sequences a log for others.

This is the second revision of Part 3. The first made mutator bodies an
*interpreted* language and a single global log; this one compiles them to
native source instead, carries facts beside intents, and makes authority a
role. It also split the database into scopes, which spec version 3 removed
again (`docs/scopes.md` records them): a module has one log, as in Petros.
Parts 1 and 2 are unchanged.

Nothing here is built. It is the shape of the thing, the reasons, and the
order to build it in — the same kind of document `docs/decisions.md` and
`docs/ivm.md` were before the code existed.

---

## Part 1 — What Petros is

### The idea, in one paragraph

A server owns an append-only, totally ordered log of *intents*. An intent is
`AddToPlaylist { playlist, media }`, never `ItemInserted { pos: 7 }`: `apply`
reads the database to decide what to write, so an entry still means the right
thing when it lands after entries its author never saw. Every peer holds the
whole log and materialises its tables by replaying it. A client's view is
`replay(confirmed) then replay(pending)`; confirmed state only moves forward,
and the only thing ever undone is the client's own pending intents, replayed on
top when confirmed entries land. That rebase is the entire concurrency story.
All non-determinism (`NewId`, `Now`) is drawn once at the originating client by
`fill_auto` and frozen into the entry. The log is permanent: a variant is never
renamed or removed, a field is never retyped, and `tests/wire.rs` pins a
checked-in byte fixture against that.

### The crates, and what each one is for

| crate | role | what it depends on |
|---|---|---|
| `petros` | the engine: `Client` (savepoint rebase), `Server` (sequence, dedupe, fan-out), `live` (rooms), `backend` (SqliteStore), `proto` (frames), transports | Diesel, SQLite, ciborium |
| `petros-schema` | the app contract: `Value`, `Store`, `Plan`/`Node`/`Query<T>`, `Change`, `Id<T>`, `Ctx`, `row!`, the declared verb surface and `compat` | none |
| `petros-sql` | `tables!()` — a proc macro that runs `schema.sql` through an in-memory SQLite and generates row types, typed columns, `key_of`, and both directions of every `REFERENCES` | rusqlite at build time only |
| `petros-ivm` | `Source`, `Filter`, `Join`, `Take`, `Tally`, and a `View` that holds the answer and reports `Patch`es | petros-schema |
| `petros-macros` | `#[mutation]`, `#[query]`, `peer!` — dispatch, `fill_auto`, the schema line, the UniFFI `impl Peer` block | syn |
| `petros-wasm-guest` / `-host` | `export!` and wasmi: `apply` as a module, for the one peer that cannot relink in under a second | wasmi |
| `petros-codegen` | reads the `petros_schema` custom section out of a `.wasm` and writes TypeScript; `log-compat` holds a module to a recorded surface | none |
| `petros-axum` | one WebSocket handler, `Hub`, `Hub::exchange` for a peer with no socket, the 20 s ping | axum, tokio |
| `petros-auth` | the server as the only OpenID Connect client; sessions; the code exchange each client performs | axum, ureq |
| `petros-testkit` | `Sim<A>`: a seeded in-process fleet with partitions, duplication and drops, and a schema-walking state hash | petros |

### The shapes that matter

**The entry and the frames** (`crates/petros/src/proto.rs`). CBOR via
ciborium, internally tagged with `"t"`:

```rust
pub struct Entry<M> { id: Id, actor: ActorId, seq: Option<Seq>, #[serde(rename="m")] mutation: M, session: Option<String> }

enum ClientMsg<M> { Hello { since: Seq, token: Option<String> }, Push { entries: Vec<Entry<M>> }, Say { say: Vec<u8> } }
enum ServerMsg<M> { Batch { entries, has_more }, Ack { ids, seqs }, Reject { id, reason }, Denied { reason }, Heard { hear: Vec<u8> } }
```

The mutation itself is not a Rust enum. `app!` wraps the raw CBOR map
`{ "t": "AddSong", ...args }` as `Payload(cbor::Value)`, so a peer that has
never heard of a verb still carries it through the log intact and applies it
once it has code that knows what it means.

**The store the domain writes through** (`petros-schema/src/store.rs`):

```rust
pub enum Value { Null, Int(i64), Text(String), Blob(Vec<u8>) }        // no floats, total order
pub trait Store {
    fn fetch(&mut self, plan: &Plan) -> Vec<Vec<Value>>;               // one statement: filter, order, limit, seek
    fn get_row(&mut self, table: &str, key: &[Value]) -> Option<Vec<Value>>;
    fn put_row(&mut self, table: &str, row: &[Value]) -> Result<(), String>;    // reports Add or Edit{old,new}
    fn delete_row(&mut self, table: &str, key: &[Value]) -> Result<(), String>; // reports Remove
    fn take_changes(&mut self) -> Vec<Change>;
}
pub struct Plan { table, filter: Option<Node>, order: Vec<(String, Dir)>, limit: Option<u32>, start: Option<Vec<Value>> }
enum Node { In { column, values }, Cmp { column, op, value }, All(Vec<Node>), Any(Vec<Node>), Not(Box<Node>) }
```

Five operations, a query that is *data*, and a write that says what it
changed. That last property is what the incremental views are built on, and
the reason `favorite_all` is a loop rather than an `INSERT … SELECT`.

**The rebase** (`crates/petros/src/client.rs`). The optimistic view lives in
an open SQLite savepoint, held only while something is pending:

```
local write:       SAVEPOINT one; apply; RELEASE one; commit the intent to <db>-intents (a second file)
confirmed arrives: ROLLBACK TO pending; RELEASE; COMMIT; apply confirmed…; SAVEPOINT pending; replay remaining pending
last ack:          RELEASE; COMMIT
```

Intents are in their own file so that a tap is one fsync and O(1) rather than
O(pending). A rollback reports nothing, so a view is told `Changes::Rebuilt`
and re-hydrates rather than being lied to.

**The server.** Assigns `seq = head + 1`, dedupes by entry id (a replayed
push is re-acked with its old seq), applies to its own database before
appending — which is what makes a `Reject` a deterministic verdict rather than
one machine's opinion — and after every message sends every connection every
entry above its cursor, 256 at a time. One rule covers first sync, resume and
broadcast. Identity is asked once at `Hello` through `Authenticate`; every
pushed entry's `actor` and `session` are held to it.

**The realtime channel** (`live.rs`). `Say`/`Heard` are opaque bytes on the
same socket; a room is one account; the app's `Live` machine decides what a
frame means and whether a room is worth a snapshot row (`petros_live`) when it
empties. A live frame is taken *before* the rebase rollback, so a position
report a second cannot re-hydrate every view. Three lifetimes: the log is
permanent, a room is current, a frame is now.

**The wasm ABI** (`petros-wasm-guest`, ABI 3). The guest imports two host
functions, `store(request) -> len` and `take(into, len)`, and exports
`petros_alloc/free/apply/fill_auto/abi_version`. Every read and write crosses
as a CBOR `Request { Fetch | Get | Put | Delete }`; changes are recorded on the
host side. The module also carries its declared verb surface in a custom
section, which is what `petros-codegen` and `log-compat` read.

**The domain surface** (`petros-macros`). Parameters are classified by
*type*: `&mut Db` is the store, `NewId<T>` and `Now` are the autos, `Ctx` is
who authored it, everything after is a caller argument and reaches the schema
line (`AddSong title:Text playlist:Id(playlist)`), the authoring function, the
generated TypeScript, and — under `feature = "foreign"` — a
`#[uniffi::export] impl Peer` method. `peer!` is a *separate* list that wires
dispatch, which is the hole `set_artwork` fell through.

### What it has proved, and what it has not

Proved, by tests that were each falsified at least once: convergence under an
adversarial network (proptest over 48 sessions, three peers, partitions,
duplicates, drops); a mutation costs the same at pending depth 400 as at 5;
native and wasm builds agree on rows, refusals and `fill_auto`; maintained
views equal a re-run over random sessions and do O(changed) work; the wire
fixture from before `Id<T>`, `session` and `token` still decodes.

Named and not solved (`decisions.md`, "What the engine does not do"): no
compaction, no authorisation, no partial sync, and — the one that matters most
for what follows — **the log freezes arguments, not meaning.** `tests/history.rs`
shows two peers on one log, built either side of a change to what `Add` does,
ending at `[1,2,3]` and `[10,20,30]` with nothing to say so. The discipline is
"a verb's meaning is immutable; add a verb"; the mechanism (a version on the
entry, every version compiled in forever) was deferred for want of a reason.
ArkDB has the reason and a cheaper mechanism; see Part 3.

---

## Part 2 — How harken uses it, and what that costs now

### The one domain, three ways in

harken's `domain/` is ten tables in `schema.sql`, ten mutations and sixteen
queries in `functions.rs`, and a live protocol in `listening.rs`. It reaches
three programs by three different mechanisms:

| peer | how `apply` arrives | how it reads | how it talks |
|---|---|---|---|
| server (axum) | linked, `petros::app!(HarkenApp)` | Diesel through `SqliteStore` | `Hub`, `Hub::exchange` for the scanner and the Home Assistant bridge |
| desktop / browser (iced) | linked, same crate | same; a `petros::ivm::View<Media>` maintained from `take_changes()` | `transport::ws` natively, `transport::web` in a browser |
| phone (Expo) | **wasm module inside a UniFFI `.so`**, installed by `before()` before `open` | `library_update()` patches across the bridge; every other query re-read | the socket is JavaScript (`petros-js`), frames cross as `ArrayBuffer` |

The phone's path is the one this document exists to replace. Written out,
one tap on "add to playlist" is: TypeScript → UniFFI `addToPlaylist(list,
media)` with ids as `String` → `serde_json` → `from_json` converts by declared
type → CBOR payload → `fill_auto` in wasmi → `apply` in wasmi, every read and
write crossing back to the host as a CBOR `Request` → Diesel → SQLite →
changes recorded host-side → views settled → `LibraryUpdate` as a UniFFI
record → Hermes. Six encodings for one intent, and four toolchains (cargo,
`ubrn`, gradle/Xcode, Metro) to keep in step to ship it.

### What that costs, as found in the code

**Types are mirrored by hand where the generator does not reach.**
`mobile/src/listening.ts` restates the peer's six `listen_*` exports as a
structural `Listener` interface, and the `Wire` class is a line-for-line
translation of `iced/src/listening.rs` down to `DRIFT_MS = 1100`.
`mobile/src/media.ts` re-implements `harken::listening::url` and already
diverges from it (`encodeURIComponent` leaves `!*'()` alone; the scheme check
is case-insensitive in one and not the other). `ui/artwork.tsx` carries an
FNV-1a that `iced/src/art.rs` has to match by walking UTF-16 on purpose.
`peer.ts` knows that a recording key is `"{of}@{who}"` and splits on `@`.
Each of these is domain logic in TypeScript, which CLAUDE.md forbids, and each
exists because the boundary could not carry it.

**Integers are a conversion at every crossing.** Twenty-odd `bigint`/`number`
translations across `listening.ts`, `player.tsx`, `trackrow.tsx`,
`format.ts` and `petros-js` — `Count = bigint | number` because `u64` arrives
as one type from one engine build and another from the next.

**The generated shape has never been seen.** Under "Not verified": the
phone's half of the listening session was written against what the Rust
declares and checked by reading, because `ubrn generate` and `tsc` cannot run
where the code was written. `i64` is *assumed* to arrive as `bigint`,
`Option<T>` as `T | undefined`. iOS has never been built at all.

**Two `SCHEMA_VERSION`s.** `HarkenApp` is at 4; `ForeignApp` (the phone) is
pinned at 0 because "a migration is the one thing that should not arrive over
the air" — so a phone that had the duplicate playlists keeps them until it is
reinstalled, while every other peer rebuilt. Two replicas of one log,
deliberately allowed to disagree because the phone's `apply` is a different
kind of thing.

**The verb that could not apply.** `set_artwork` compiled, type-checked at
every call site, reached `mutations.txt` and the TypeScript, and was refused
at run time for two commits because `peer!` is a second list. The boundary
had four places that could say a verb existed and one that decided whether
it did.

**The build is the cost that is hardest to see from the code.** The phone's
CLAUDE.md is mostly about the toolchain: `nodeModulesHash` per platform, a
recorded 1642-artifact Maven graph, `ubrn-Cargo.lock`, a gradle state layer,
qemu wrapping for the NDK, `LD_BIND_NOW`, aapt2 overrides, the 16 KiB page
wall on Apple Silicon, a stale fixed-output derivation that shipped a phone
whose client had no `tick`. None of that is about music, and most of it is
about getting a Rust `apply` into a JavaScript app.

### Why splitting the phone into Swift and Kotlin makes it worse, not better

Today there is one foreign client and one hand-mirrored TypeScript layer.
With native apps there would be two of each: UniFFI Swift bindings and UniFFI
Kotlin bindings, each with its own `bigint`-shaped gaps (`UInt64` vs `ULong`,
`Data` vs `ByteArray`), each with its own copy of the listening state machine,
each needing the wasm module bundled and installed before `open`, and both
still unable to do the one thing a native app most wants — hold a maintained
view natively and drive a `List`/`LazyColumn` from patches — without crossing
the bridge for every row. `apply` would still be a Rust artifact the app
cannot inspect, cannot version per entry, and cannot receive from the server.

The domain is nineteen functions. What they *do* is trim strings, derive keys
from names, read `MAX(pos)`, compare and put rows. That is not a workload
that needs a compiled language; it is a workload that needs to be the *same*
in three languages, which is a different requirement, and compiling it is the
wrong tool for it.

---

## Part 3 — ArkDB

### The thesis

**The domain is a program in Ark IR, and nobody runs the IR.** A mutation or
a query is authored through a *builder* — a typed library in Rust, Swift or
Kotlin whose calls construct an IR tree rather than execute anything — and
emitted as data at build time. `arkc` verifies the IR and generates native
source from it for every language: Rust for the server and the desktop,
Swift for iOS, Kotlin for Android. A function authored with the Swift builder
runs on the Rust server as generated Rust; one authored with the Rust builder
runs on the phone as generated Swift. The generated code is what runs
everywhere; the builder is a compile-time tool; there is no interpreter, no
wasm and no FFI.

**The log is intents, and every peer that holds the log whole is an exact
replica of it.** That is Petros's model, kept because it is the only one
under which a peer that has never met a server is a first-class citizen: its
history is intents, so it can be adopted, verified and rebased later. What
is added around it is what a fleet of native apps needs from a sync system:

- **facts, retained beside the log**, so a peer that cannot apply an entry
  (an old build, a partial holder) takes its effects instead and ends in the
  same state;
- **snapshots**, so nothing replays from sequence one and retention has a
  horizon;
- **authority as a role**, so "server" is a thing a peer does for a log,
  and a peer alone does it for itself.

A module has one log, with one sequence and one authority, as Petros has:
every reference between tables is checked and any function may read or write
any table. A design that split it into several logs was built and removed;
`docs/scopes.md` says what it bought and what a return to it must answer.

| kept from Petros | dropped | changed |
|---|---|---|
| intents, not facts, as the log; `apply` reads to decide | SQLite as a requirement; Diesel; SQL anywhere | the store is an ordered-KV interface; SQLite is one backend |
| `replay(confirmed) then replay(pending)`; the rebase as the whole concurrency story | the savepoint | an in-memory overlay the store spec defines |
| all non-determinism in `fill_auto`, once, at origin | wasm, wasmi, the guest ABI; UniFFI, `ubrn`, Expo, `petros-js` | the domain is generated native source in every language |
| `Ctx { user, session }`; entries held to the login that pushed them | `#[mutation]`, `peer!`, `tables!` as proc macros | builders that emit IR; `arkc gen` emits the row types and the call surface |
| typed writes that report `Change`; queries as data; trees via declared references | | `Change`s are *kept* on the authority and are the facts a peer may take |
| incremental views; `Rebuilt` after a rebase | | views are in the spec so every runtime maintains them natively |
| sans-io machines; the deterministic simulation; one log per module | | the log has snapshots and a horizon (§3.9) |
| three lifetimes; rooms per account; one socket; server pings | | live frame types declared in the module, generated everywhere |
| server as the only OIDC client | | |
| "the log freezes arguments, not meaning" as a hazard | | entries name their function by hash; versions are retained above the horizon |

### The layers

```
  ┌───────────────────────────────────────────────────────────────────────────┐
  │  builders (compile time)   Rust  ·  Swift  ·  Kotlin   — typed, symmetric │
  │      each emits ───────────────────────────────┐                          │
  ├────────────────────────────────────────────────▼──────────────────────────┤
  │  Ark module (canonical CBOR, content-addressed)                           │
  │     schema · functions (mutators, queries, helpers) · live types          │
  ├───────────────────────────────────────────────────────────────────────────┤
  │  arkc      verify · hash · check · gen {rust,swift,kotlin} · vectors      │
  │      generates ────────────────────────────────┐                          │
  ├────────────────────────────────────────────────▼──────────────────────────┤
  │  generated domain code, per language: row types, apply fns, queries,      │
  │  call surface, live types — ordinary source, compiled into the program    │
  ├───────────────────────────────────────────────────────────────────────────┤
  │  runtime, one per language, all held to spec/vectors                      │
  │     codec · std (pinned) · store (layered) · views · peer                 │
  │     (log machine, authority role) · live · protocol                       │
  ├───────────────────────────────────────────────────────────────────────────┤
  │  backends   sqlite-as-btree · memory · (later) lmdb, indexeddb            │
  │  transports URLSession · OkHttp · tungstenite · axum — thin               │
  └───────────────────────────────────────────────────────────────────────────┘
```

### 3.0 Revision: one vocabulary, three spellings, and no generated code

Sections 3.3 to 3.5 below describe the first build of this design, in which
a domain was authored through builders, emitted as IR, and **generated**
into native source for each runtime — a Rust-authored mutator ran on the
phone as Swift that `arkc gen` had written. That is superseded, and the
sections are kept because the reasoning about the IR, the hash and the
store still holds. What replaced the generator, and why:

- **Compiling Rust to Rust was the wrong shape.** A domain author wrote
  builder calls that emitted IR, and then ran code generated from that IR
  rather than what they wrote. Now a domain is written once, as ordinary
  functions over a small typed vocabulary (`spec/AUTHORING.md`), and the
  same program runs two ways behind one API: under *Emit* it records the
  module, under *Native* it applies an entry directly. Every runtime holds
  its native procedures to the interpreter over their own emit, so what
  runs is what was written and what was written is what the log names.
- **Routers and middleware, as tRPC has them.** A procedure hangs off a
  router, which is a group of procedures and names no tables, and off the
  middleware chain it was built from: `router::<Harken>("playlists")`, then
  `signed_in.input::<CreatePlaylist>().mutation(…)`, `owned.query(…)`, with
  a provider handing the body the row it found. `Harken` is the module's one
  `Tables` type, the struct of every table a body's `db` is. A guard's
  refusal is the procedure's.
- **An input is a schema, and the schema is a form's validation.** Each
  field carries its checks — trim, length, range, non-empty, *exists* —
  and a failing check is a refusal with a message every runtime spells the
  same. `Eval.check` runs the same walk over a partial input and returns
  every field's first message, so the new-playlist sheet on each phone
  shows exactly what `create_playlist` would refuse with, in any language.
- **Writes are an ORM's, and they match inside.** `db.playlist.insert(row)
  .on((Playlist::user_id, Playlist::name))` writes unless a row matches on
  a declared unique index, which is what makes a second device's default
  playlist a no-op; `upsert` and `update` are its siblings, and `put` is
  gone. The non-determinism is `ctx.now("added_ms")` and
  `ctx.new_id("id")`, drawn once at the origin and frozen as before.
- **The round trip is the property.** `arkc gen rust|swift|kotlin` prints a
  module back as the source that emits it, in the vocabulary, with each
  language's formatter owning its layout; `arkc roundtrip` holds a source to
  that print. harken's domain is written once in Rust; the Swift and Kotlin
  the phones compile *are* the print, restricted to what a phone carries;
  and every procedure of all three hashes identically. A function's hash is
  still over the normalised IR, now with the middleware it runs among its
  dependencies, so editing a guard re-hashes every procedure behind it.

Spec version 3 is this contract: version 2 without scopes
(`docs/scopes.md`). What follows describes version 1 where they differ;
`spec/AUTHORING.md` is authoritative for authoring, and the modules under
`spec/src/Ark/` for everything.

### 3.1 Values and the canonical encoding

Small on purpose, because every type is one three generated codebases have
to agree about to the byte.

| type | notes |
|---|---|
| `Null` | only as the absent case of `Option<T>` |
| `Bool` | |
| `Int` | signed 64-bit; arithmetic is *checked* and overflow is a refusal |
| `Text` | valid UTF-8; **compared by code point**, never by locale or canonical equivalence. Generated Swift compares UTF-8 bytes explicitly, since Swift's `==` treats `é` and `e◌́` as equal; generated Kotlin does too, since UTF-16 order misplaces astral characters. The vectors include both traps |
| `Bytes` | |
| `Id<table>` | 16 bytes, CBOR tag 37; the table is a type and the bytes are Petros's |
| `List<T>`, `Struct`, `Option<T>`, `Enum` (fieldless) | |

No floats; a gain is an `Int` in millibels. One total order over all values
(type rank, then within type; lists and structs lexicographically), because
`ORDER BY`, index keys and the state hash all need it. **Encoding is RFC 8949
§4.2.1 deterministic CBOR** with the mapping above, internally tagged maps
with `"t"` where Petros has them, and a **state hash** — SHA-256 over the
canonical encoding of every table's rows in key order — that is a spec'd
quantity rather than a testkit convenience, because §3.8 exchanges it.

### 3.2 The schema

A module has one set of tables and one append-only intent log over them,
with one sequence, one authority, one snapshot series and one access rule.
Every reference between tables is checked, and any mutator, query or helper
may read and write any table, so `add_to_playlist` keeps the
`exists(media)` check it has in Petros. (Spec version 2 split the tables
into scopes, each its own log; `docs/scopes.md` says why they are gone.)

The schema is a value in the module:

```
Schema { tables: [Table] }
Table { name, columns: [Column { name, ty, nullable }], key: [column],
        indexes: [Index { columns, unique }], refs: [Ref { column, table }] }
```

`arkc gen` derives from it what `tables!` derives from SQLite's pragmas, for
every language: row structs, typed column handles, `key`, and both
directions of every reference as a relationship (reading down keeps a
childless parent, reading up drops an orphan). The generated code enforces,
as deterministic refusals and identically everywhere, not-null, unique
indexes and references; a backend is never asked what a reference
is. A schema is additive-only; a column is *retired* (reads as its default,
writes dropped) rather than removed; a change to a live column is a rebuild
from snapshot plus replay, as `SCHEMA_VERSION` is today.

### 3.3 Ark IR

A module carries mutators, queries and pure **helpers** (harken's `slug`,
`key_part`, `work_key`, `art_to_write` are helpers), all typed, all verified.
The verifier is part of the spec and a module's hash is of its verified,
alpha-normalised form — names an author chose travel in a side table that is
not hashed, so the same function authored in two languages with different
local names is the same function.

```
stmt ::= let x = expr                      -- x is a symbol; reads are lets, so a select runs once
       | if expr { stmts } else { stmts }
       | for x in expr { stmts }           -- over a List; finite by construction
       | put(table, expr)                  -- reports Add | Edit; refuses on constraint
       | delete(table, key)                -- reports Remove; a missing row is a no-op
       | refuse(expr: Text)
       | return
expr ::= literal | $arg | x | ctx.user | ctx.session | e.field | Struct { … }
       | op(e, e) | call(helper, …) | std.f(…)
       | select(plan) | get(table, key) | exists(table, key)
       | map/filter/first/fold/… over lists ; match over Option
plan ::= from(table) filter(pred) order(cols) take(n) related(rel, plan)
```

No `while`, no recursion (helpers may call helpers declared before them),
no I/O, no clock, no floats, every `select` in a mutator totally ordered
(the verifier appends the key as a tie-break), checked arithmetic. A
**query** is a *plan* — maintainable, §3.13 — followed by a pure *shape*
over its rows; harken's sixteen queries all split that way today.

**The standard library is the only escape hatch, and admission is an exact
definition plus vectors.** Text (`trim`, `concat`, `lower`, `is_alnum`,
`chars`, `split_once`, `starts_with`, `len`), int (`min`, `max`, `clamp`),
hash (`fnv1a64`, `sha256`), id and list operations — what `functions.rs`
uses, checked line by line. Because the IR is *compiled* rather than
interpreted, admitting a function costs one mapping per target generator
plus its implementation in each language's pinned `ArkStd`, not a new
evaluator case in three interpreters; so the library may grow at the pace
the vectors can hold it. Unicode is still the trap: `lower` and `is_alnum`
key the log through `slug`, and Rust, ICU-on-iOS and ICU-on-Android disagree
at the edges. The spec pins a Unicode version and ships the three tables it
needs (White_Space, Alphabetic ∪ Numeric, simple lowercase) as data; every
`ArkStd` embeds them and none calls the platform.

### 3.4 Authoring: builders, and going from any language to any other (superseded by 3.0)

There are no macros. A builder is an ordinary library in each language whose
calls construct IR; an author writes an ordinary function against it; a
build step runs that function and writes the module. The technique is old
and well-trodden — LINQ expression trees, Exposed and Slick, JAX tracing,
Petros's own `Song::all().filter(Song::done.eq(false))`, which already builds
a `Plan` this way. Ark extends it from plans to statements.

The three things a builder has to get right, and each language's answer:

- **Typed symbols.** An argument, a `let`, a loop variable and a `ctx` field
  are `Expr<T>` values, not host values. `Expr<Int> + 1` builds an add;
  `Expr<Bool>` is not `Bool`, so a host `if` cannot accidentally branch on
  one. Rust overloads `+ - * !` and uses `.eq()` for comparison, as Diesel
  does; Kotlin overloads the arithmetic and uses infix `eq`; Swift can
  overload `==` to return `Expr<Bool>`.
- **Control flow as combinators.** `if_(cond, |b| …).else_(|b| …)`,
  `for_each(rows, |r, b| …)` in Rust; result-builder components `If(cond)
  { … }` and `ForEach(rows) { r in … }` in Swift; `iff(cond) { … }` and
  `forEach(rows) { r -> … }` in Kotlin. The closure receives the bound
  symbol, which is how a loop variable gets its binding.
- **Reads are statements.** `select`, `get` and `exists` bind through
  `let`, so a host-level reuse of an `Expr` never runs a query twice; every
  other expression is pure and may be inlined freely.

One function, three builders. Column handles and row constructors
(`PlaylistItem.pos`, `PlaylistItem.row(...)`) are generated from the schema
by `arkc gen` for the *authoring* language first; schema and functions are
two build steps, as they are in every code-generating system.

```rust
// Rust
pub fn add_to_playlist(f: &mut Mutator) {
    let added_ms    = f.now("added_ms");
    let playlist_id = f.arg::<Id<Playlist>>("playlist_id");
    let media_id    = f.arg::<Id<Media>>("media_id");
    let b = f.body();
    b.if_(b.exists(PlaylistItem::key((playlist_id, media_id))), |b| b.ret());
    let last = b.let_("last",
        b.select(PlaylistItem::all().filter(PlaylistItem::playlist_id.eq(playlist_id))
                 .order_by(PlaylistItem::pos.desc()).limit(1))
         .first().map(|r| r.pos).unwrap_or(0));
    b.put(PlaylistItem::row().playlist_id(playlist_id).media_id(media_id)
          .pos(last + 1).added_ms(added_ms).user_id(b.ctx().user()));
}
```

```swift
// Swift
let addToPlaylist = Mutator("add_to_playlist") { f in
    let addedMs    = f.now("added_ms")
    let playlistId = f.arg(Id<Playlist>.self, "playlist_id")
    let mediaId    = f.arg(Id<Media>.self, "media_id")
    If(exists(PlaylistItem.key(playlistId, mediaId))) { Return() }
    Let("last", select(PlaylistItem.all.filter(PlaylistItem.playlistId == playlistId)
                       .orderBy(PlaylistItem.pos.desc).limit(1)).first.map { $0.pos } ?? 0) { last in
        Put(PlaylistItem.row(playlistId: playlistId, mediaId: mediaId, pos: last + 1,
                             addedMs: addedMs, userId: ctx.user))
    }
}
```

```kotlin
// Kotlin
val addToPlaylist = mutator("add_to_playlist") {
    val addedMs    = now("added_ms")
    val playlistId = arg<Id<Playlist>>("playlist_id")
    val mediaId    = arg<Id<Media>>("media_id")
    iff(exists(PlaylistItem.key(playlistId, mediaId))) { ret() }
    val last = let("last", select(PlaylistItem.all.filter(PlaylistItem.playlistId eq playlistId)
                                  .orderBy(PlaylistItem.pos.desc).limit(1)).first().map { it.pos }.orElse(0))
    put(PlaylistItem.row(playlistId = playlistId, mediaId = mediaId, pos = last + 1,
                         addedMs = addedMs, userId = ctx.user))
}
```

Kotlin reads best of the three, which reverses the first revision's
asymmetry: builder DSLs with lambdas-with-receivers are the thing Kotlin is
good at, and Compose, Gradle and Exposed are all this shape. Rust is the
noisiest because closures over a block builder are the only way to bind a
symbol without a macro. That is the honest ordering, and none of the three
is second-class.

**All three emit the same bytes.** The `frontend/` vectors (§3.16) hold a
function authored in each builder and assert one hash, after
alpha-normalisation. That is the test that "you can go from Swift or Kotlin
to Rust": authorship leaves no trace in the module, so the generator does
not know or care which builder made it.

**What comes out the other end** is ordinary source. From any of the three
above, `arkc gen rust` writes:

```rust
pub fn add_to_playlist(db: &mut impl Store, ctx: &Ctx, added_ms: i64,
                       playlist_id: Id<Playlist>, media_id: Id<Media>) -> Result<(), Refusal> {
    if db.exists::<PlaylistItem>(&PlaylistItem::key((playlist_id, media_id)))? { return Ok(()); }
    let last = db.select(PlaylistItem::all().filter(PlaylistItem::playlist_id.eq(playlist_id))
                         .order_by(PlaylistItem::pos.desc()).then_by(PlaylistItem::media_id.asc()).limit(1))?
                 .first().map(|r| r.pos).unwrap_or(0);
    db.put(&PlaylistItem { playlist_id, media_id, pos: std::checked_add(last, 1)?,
                           added_ms, user_id: ctx.user.clone() })?;
    Ok(())
}
```

and `arkc gen swift` writes the same function as a Swift method on the
generated `Harken` domain type, with the same tie-break and the same checked
add, calling `ArkStd` for anything the spec pins. The generated code is
idiomatic enough to read and to step through in a debugger, which is what
"converted back into those languages" buys and an interpreter never could;
it is never edited, and it is regenerated from the module on every build.
The author's own language gets generated code too: a Rust-authored domain
runs on the server as generated Rust, not by running the builder.

**The verifier catches what a host type system cannot.** A loop variable
used outside its closure is an unbound symbol; a select bound in one branch
and read in another is the same; a helper calling a helper declared after it
is a cycle. The builders make these hard to write and the verifier refuses
them anyway, because a module may arrive from anywhere.

**`arkc`** is one CLI, in Rust: `verify`, `print` (the diagnostic text form,
what a diff shows and what replaces `mutations.txt`), `check` (§3.12),
`gen rust|swift|kotlin`, `vectors` (§3.16). Each language's build invokes it:
a `build.rs`, a SwiftPM plugin, a Gradle task.

### 3.5 What generated code runs against (superseded by 3.0)

Each language ships one runtime package, and the generated domain code calls
exactly two things in it: the **store** (§3.6) through `select`, `get`,
`exists`, `put`, `delete`, and **`ArkStd`**, the pinned standard library.
Everything else in the runtime — the peer, the protocol, views, live rooms —
sees the domain only through two generated entry points:

```
apply(fn_hash, ctx, autos, args, store) -> Result<Changes, Refusal>
fill_auto(fn_hash, auto_ctx) -> autos
```

`apply` dispatches on the function hash to the generated function of that
*version* (§3.12). A runtime knows nothing about any domain; harken's
generated code is a package the app links beside the runtime.

### 3.6 The store: layered, backend-agnostic

```
get(table, key) -> Option<Row>
scan(table, index, lo, hi, dir, limit, after) -> [Row]     -- sorted, seekable
put(table, row) -> Change | Refusal
delete(table, key) -> Option<Change>
commit(batch)                                              -- atomic
```

Petros's five methods with `fetch(plan)` replaced by `scan` over a declared
index: the generated code compiles a plan to scans, so a backend never sees
a filter, a `NULL` comparison or a collation. A backend is an ordered
key-value store with atomic batch commit — SQLite's B-tree, LMDB,
IndexedDB, a `BTreeMap`. **SQLite stays as the default durable backend on
every device, as a B-tree and not as a database**: rows keyed by `(table,
canonical key bytes)`, index entries keyed the same way, no SQL
above the two statements that read and write them. What leaves is the
dependency on SQL *semantics*, which is where the cross-language hazards
were.

**The rebase is an overlay.** Confirmed state lives in the base; pending
intents apply forward into an in-memory overlay; reads consult the
overlay first. A confirmed entry arriving drops the overlay, applies to the
base in one batch, and re-runs the still-pending intents into a fresh
overlay. Pending intents are themselves committed in the base (one fsync per
tap, as today), the optimistic state never is, a tap costs the same at
pending depth 400 as at 5, and a view is told `Rebuilt` after a rebase
because dropping an overlay reports nothing.

### 3.7 The log

A module is one append-only log: one `seq`, one authority, one snapshot
series, and one cursor per peer. A peer holds it in one of two ways:

- **whole**, in which case it replays intents and is an exact replica, may
  author into the log, and can verify its state hash against anyone;
- **as a projection** (§3.11), in which case it receives facts for the rows
  it asked for, may still author intents (its optimistic preview is
  approximate and the authority's answer wins), and does not claim exactness.

Any mutator may read and write any table, and one entry is one transaction
over all of them. The cost is that a whole peer holds everything, as in
Petros: there is no partial replication of an exact replica, no per-person
log and no second authority. `docs/scopes.md` records the design that had
those, and the questions a return to it must answer.

### 3.8 Intents and facts: one log, two ways to apply it

The authority applies each pushed intent in a transaction, and the generated
`put`/`delete` report exactly which rows changed. Petros computes those
`Change`s and discards them once views are settled. **Ark keeps them, beside
the entry:**

```
log: (seq, Entry { id, actor, session, fn: hash, args, autos }, facts: [Change])
```

A peer receiving `Batch` gets the entries. For each one it does one of two
things, and the state is the same either way, because the facts *are* what
exact replay produced:

- it knows `fn` — its generated code has that function version — so it
  **replays the intent**, exactly;
- it does not — an older build meeting a new verb, a build from before a fix
  shipped under a new hash — so it asks `Facts { seqs }` and
  **applies the rows**.

Both paths leave the peer an exact replica, because applying an entry's
facts is applying its effect. A peer that took facts for sequence 4127 can
still replay 4128 by intent, since it starts from the right base. So an old
client is never "update required": it applies by facts what it cannot apply
by intent, keeps authoring every verb it does know, and shows the new verb's
effects. The same mechanism is the cure for divergence: `Verify` finds it,
and the peer resyncs from the last snapshot plus facts rather than being
reinstalled.

**Frames**, canonical CBOR, tagged `"t"`:

```
Entry    { id, seq?, actor, session, fn: Bytes(32), args, autos }

client → Hello    { since: Seq, mode: whole | projection(plan), token?, spec: Int }
         Push     { entries: [Entry] }
         Facts    { seqs: [Seq] }                 -- entries I cannot replay
         Snapshot {}                              -- I am below the horizon; start me over
         Verify   { seq, hash }
         Say      { say: Bytes }

server → Batch    { entries: [Entry], has_more }
         Facts    { facts: [(Seq, [Change])] }
         Snapshot { seq, hash, rows: …, has_more }
         Ack      { ids, seqs } · Reject { id, reason } · Denied { reason }
         Agree    { seq, hash, ok }
         Heard    { hear: Bytes }
```

A `Hello` carries one subscription, and no other frame names anything
narrower than the log. Everything else is Petros: `seq = head + 1`, dedupe
by entry id, the authority applies before it appends so a `Reject` is a
verdict (its `reason` a sentence a screen can show beside the item), fan-out
is everything above a peer's cursor, `Heard` is taken before the rebase,
identity once at `Hello`, the server pings.

**What Replicache and Zero do here, and how this differs.** Both are the
facts-down model, and it is worth being exact about it because this design
borrows its rebase from them. In Replicache the client runs a *speculative*
mutator against a local store, pushes the mutation by name and arguments, and
the server runs its own implementation against the real database; `pull`
returns a patch of row-level `put`/`del` operations computed from the
server's *state* (the cookie or client-view-record strategies), plus which
mutation ids have been processed, and the client discards its speculative
state, applies the patch, and re-runs unacknowledged mutations on top. Zero's
custom mutators are the same shape with a query layer under them: a mutator
runs on the client optimistically and on the server in a database
transaction via the push endpoint, `zero-cache` replicates the database,
computes each client's active queries incrementally, and streams changed
rows to the client, which reconciles. Rocicorp call it server reconciliation
and are candid that the client's mutator need not match the server's: the
server's result wins, and that tolerance is what makes their permissions,
validation and "just change the server logic" story easy.

Three consequences of that model, and where Ark stands on each:

- **Their source of truth is the materialised database; the mutation is a
  transient request.** Nothing keeps intents once processed, so there is no
  log to compact, no history to replay, no per-entry versioning, and an old
  client is fine because it only ever receives rows. Ark's source of truth
  is the intent log, and facts are *derived* from it and retained only above
  the snapshot horizon. Above the horizon Ark pays what they never pay —
  retained entries, retained facts, retained function versions. Below it,
  Ark is in their position: a snapshot is state, and history under it is
  gone. The horizon is the dial between local-first exactness and their
  steady state.
- **Their clients are never exact, and it does not matter to them; Ark's
  whole peers are exact, and that is the point.** Exactness is what
  makes a peer with no server a first-class holder of a log whose history
  can later be adopted and *verified* by an authority — replay the intents,
  match the hash — and what makes two peers able to check they agree. A
  Replicache or Zero client cannot be an authority for anything, because
  the truth is the server's database and the client only ever had a
  speculation and a copy. That is the capability the user asked for, and it
  is the one their model structurally cannot offer.
- **Their partial sync is row-level and query-driven; Ark has none for
  exact peers, which hold the whole log, and is query-driven only for
  projections.** Zero's model is strictly more flexible about *which rows* a
  client holds, because facts-down does not care what the client can
  compute. Ark buys exactness by holding everything, and recovers Zero's
  grain in facts mode: a projection subscription *is* a query the authority
  maintains with the same view machinery the spec already has (§3.13),
  streaming facts for its rows.
  In that mode an Ark peer is a Zero client, and the two designs meet.

On the "two paths to test": Replicache and Zero also have two — the
speculative apply on the client and the authoritative apply on the server —
and they resolve the disagreement by fiat. Ark's two paths are held to
*agree*, and the test is cheap for a reason worth stating: the facts are the
recorded output of the intent path, so the `eval/` vectors assert that
applying an entry's facts to the prior state yields the same hash as
replaying it, which is one extra line per vector rather than a second
engine. The storage cost is comparable to theirs or smaller: Replicache's
client-view-record strategy keeps per-client, per-row version metadata, and
`zero-cache` keeps a full replica of the database plus each client's query
state; Ark keeps `Change`s above the horizon, which for harken is kilobytes
per day.

### 3.9 Snapshots and the horizon

A snapshot of the log at `seq` is the rows and their state hash. It is what
a new device starts from, what a peer below the horizon restarts from, and
what a schema rebuild replays forward from. Because every whole peer
replays exactly, a snapshot is **verifiable**: any peer with the log can
reproduce the hash, and an authority adopting a peer's log (§3.10) proves
the claimed snapshot by replaying to it.

The **horizon** is the oldest sequence the authority still serves. Below it
go entries, their facts, and every function version no retained entry names
— so "every version compiled in forever" becomes "every version above the
horizon", and `arkc gen` emits only those. How far back the horizon sits is
the authority's policy: far enough that every peer it expects to see can
catch up by tail, and no further. A peer that has been away longer takes a
snapshot and keeps its pending intents, which replay on top as they always
did. This is Petros's "no compaction" item closed, and it interacts with
§3.12 exactly as `decisions.md` predicted it would.

### 3.10 Authority is a role

Every peer runs the same log machine; **the authority of a log is the peer
that sequences it**. A server is a peer that does this for the logs it
hosts, for others. A peer with no server does it for its own log: it
sequences its own intents, keeps its own log, snapshots itself, and never
replays from zero on open. It is not offline, not in a mode, not "pending
forever"; it is a database with one replica.

When such a peer later meets a server, one of two things happens, both
already in the design:

- **A log the server has never seen is adopted whole.** The peer sends its
  log (or its snapshot and tail); the server replays every intent through
  its own generated code and checks the hash the peer claimed. A peer cannot
  smuggle rows it did not derive — this is the property only exact replicas
  have, and the reason intents rather than facts are the log. The server
  becomes the authority; the peer becomes a whole replica of it with
  nothing pending.
- **A log the server already holds takes the local entries as pending
  intents.** They rebase onto the server's log through the ordinary path at
  unusual depth, and refusals come back as verdicts, as they would for any
  offline edit.

Handing authority *back* — a server going away and a peer resuming
sequencing — is the same step in reverse and is deliberately not built
first. Authority transfer between two live peers is a protocol with a
fencing token, and nothing in harken needs it yet.

### 3.11 Authorization and partial sync

Two grains, and they line up with the two ways of holding the log:

- **Log grain.** Who may *receive* the log is one rule at its authority,
  over the whole log, checked at `Hello`. Who may *write* is inside the
  generated mutator via `ctx.user`, as today; every entry is held to the
  login that pushed it, and an authority may also accept one authored under
  an older login of the same user. harken: everyone signed in receives
  everything, and whose playlist a write may touch is the mutator's
  question. Read authorization finer than the whole log, for an exact
  replica, is what the removed design was for (`docs/scopes.md`).
- **Row grain, in facts mode.** A projection is a plan the authority
  maintains for that peer and streams facts for; the plan can carry a
  predicate the peer did not write (a permission rule), which is how
  row-level read authorization is expressed and the only place it can be —
  a peer holding a filtered subset cannot replay intents against it, so it
  does not.

This closes the "no authorisation" half of the "no authorisation, no
partial sync" item from `decisions.md` at the coarsest grain, the whole log,
and leaves partial sync to the mode that does not claim exactness.

### 3.12 Versioning: functions by hash, retained above the horizon

Every function's hash is SHA-256 of its verified, alpha-normalised canonical
form; an entry records the hash of the function that authored it; `apply`
dispatches on that hash to the generated function of that version. Editing
a function *is* adding a function, and the old one stays under its old hash
for the entries that name it. `tests/history.rs` — two builds replaying one
log to two states — cannot happen between peers that both hold the version,
and a peer that does not hold it takes facts (§3.8) rather than guessing.

What bounds it: an authority drops a version once no entry above the
horizon names it, and `arkc gen` emits only the versions the module's
retention list carries, so a phone compiles the functions it may meet and
not the history of the domain. What constrains the schema: additive-only,
retire rather than remove, and `arkc check old.ark new.ark` type-checks
every *retained* function against the proposed schema and refuses a change
that would strand an entry — `check-log` as a compiler pass over the log's
live vocabulary rather than a text diff.

And the quiet consequence: there is one list. A module's functions are what
can apply, the call surface is generated from them, and a verb that reaches
the surface but not the dispatch — `set_artwork`'s two commits — has no
place to exist.

### 3.13 Views

Petros's incremental views — source, filter, join, `Take` with a
per-partition bound refilled by seeking, `Tally` — move from a Rust crate to
a section of the spec, because a query's plan is data and a native app is
exactly the caller that wants to drive a `List` or `LazyColumn` from
`Insert{at}` / `Remove{at}` / `Update{at}` without crossing anything. Fed by
the peer after every applied entry and every local mutation; told `Rebuilt`
after a rebase; the query's *shape* re-run over the view's rows when they
move. The correctness contract (rows equal a re-run) is in the vectors; the
work contract (O(changed), refills are seeks) is stated and each runtime
measures it, because a vector cannot count another implementation's reads.
The same machinery, run at an authority over a projection's plan, is what
streams a projection its facts (§3.11).

### 3.14 Live rooms

Unchanged: a room per account in the authority's memory, the app's machine
deciding what a frame means, `keep` for the one row worth writing when a
room empties, frames dropped while unlinked, a second `Hello` is paging. The
module's live section **declares the frame types**, so `arkc gen` emits
them in every language and the hand-mirrored `Listener` and `Doing` in
`mobile/src/listening.ts` have no successor. The per-device state machine
(`elsewhere`, `output_here`, the 1100 ms drift) is app code and can be
written once as a pure Ark helper over a `Session` struct if an app wants
one definition — harken has already written it twice.

### 3.15 Sign-in

Kept from `petros-auth` and written into the spec's client section because
two native clients now implement it: open `{server}/auth/login?redirect=R`,
receive a single-use code, `POST /auth/exchange` for `Login { token,
session, user, expires_ms }`, put the token in every `Hello`. The server is
the only OpenID Connect client; dev mode remains; `Denied` is how a stale
token is learned about. A peer with no server has no sign-in and an `actor`
it chose, which is what Petros's `Trusting` already means.

### 3.16 The conformance suite

The vectors are what make the spec binding. They live in `spec/vectors/`,
are generated by the Rust reference and reviewed like code, and every
runtime carries a `conformance` target that walks the tree. **Generated
domain code is under test too**: each `eval/` vector's module is run through
`arkc gen` for the language under test, compiled, and executed — so the
suite checks the generator, `ArkStd` and the store together, which is the
only combination that ships.

| directory | one vector is | holds |
|---|---|---|
| `codec/` | a value, its canonical bytes; a non-canonical input to refuse | RFC 8949 determinism, tag 37, the type mapping |
| `order/` | values and their sorted order | the total order; UTF-8 vs UTF-16 vs canonical equivalence |
| `std/` | a call, its result or refusal | every library function; the pinned Unicode tables; checked overflow |
| `verify/` | a module, whether it verifies, the error | typing, router uses, totality, tie-break insertion, unbound symbols |
| `frontend/` | the same function authored in each builder | **one hash** — the proof that any language reaches any other |
| `eval/` | schema, functions, prior rows, an entry, expected changes and rows or refusal; `fill_auto` from a seed | the generated code; **and** that applying the recorded facts yields the same hash as replaying |
| `views/` | plan, rows, changes, expected patches and rows | the maintenance contract |
| `rebase/` | a seeded scripted session across peers with partitions, duplicates and drops; expected hashes at settle | Petros's simulation, portable; a Swift peer and a Rust authority in one run |
| `protocol/` | frames in, frames and state out | paging, dedupe re-acks, `Denied`, `Facts`, `Snapshot`, `Verify`/`Agree`, adoption of a local log |
| `hash/` | tables, expected state hash | `Verify` means one thing everywhere |

**Differential fuzzing** is the second half: `arkc vectors --fuzz` generates
random modules over random schemas and random sessions, evaluates them on
the reference, and emits vectors; a nightly job in each runtime pulls the
day's batch through `gen`, compile and run. Every vector directory carries a
falsification check — a deliberately wrong expected answer the runner must
reject — because this repository's history has three tests that passed by
not testing the thing.

### 3.17 Repository layout

One repository, because the vectors and the runtimes move together. Each
language keeps its native build tool — cabal, cargo, SwiftPM, gradle — and
nix drives every one of them: `nix flake check` is the spec's vectors, every
runtime against them, the Rust workspace's lint and tests, harken's module
against what its Rust emits, and its three domains' round trip through
`arkc` under each language's formatter; `nix build` is any
program, the Android APK included. Gradle's Maven graphs are recorded
(`kotlin/deps.json` and `harken/android/deps.json`, re-recorded by
`nix run .#kotlin-deps` and `.#harken-apk-deps`) and replayed offline, so no
build reaches the network but the ones that fetch sources.

```
apps/
  spec/           the specification as a Haskell program (see spec/README.md): one
                  module per section, the pinned Unicode tables and their generator,
                  arkc, and the vectors ark-vectors emits
  rust/           ark (the runtime: value, canon, store, eval, hash, verify, log, peer,
                  view, live, protocol, sim, and ark::authoring, the vocabulary)
  swift/          Package.swift: ArkDB (the runtime) · ArkAuthoring (the vocabulary) ·
                  ArkDBClient (a session over them: link, persistence, an in-process
                  authority) · ArkDBTests (the vectors, the demo, harken's phone domain)
  kotlin/         settings.gradle.kts: ark-runtime (with dev.arkdb.authoring) · ark-client
  harken/         the app, one directory per program (harken/README.md):
    domain/       the domain, written once in Rust (src/); harken.ark, its emit; and
                  gen/{swift,kotlin}, arkc's print of it for the phones
    server/       ark-server with harken's scanner, listening desk and house bridge
    iced/         the desktop and browser client on arkui and ark-client; its demo
                  build is published to GitHub Pages
    ios/          SwiftUI over ArkDBClient and the printed Swift domain (xcodegen)
    android/      Compose over ark-client and the printed Kotlin; nix build .#harken-apk
  flake.nix       all of the above, as packages, checks and shells
```

An app depends on one runtime package, and carries its domain as source in
the vocabulary of whichever language it is written in, plus the module that
source emits. The harken server and its desktop and browser client depend on
`rust/ark` (through `ark-server` and `ark-client`) and link the Rust domain; the two native phone apps depend on
`swift/` and `kotlin/` and compile `arkc`'s print of the same module, which
`nix flake check` holds to a byte-for-byte round trip.

### 3.18 The path for harken

1. **Spec, vectors and the Rust runtime**, vectors first: `wire.rs` becomes
   `codec/` and `protocol/`, `converge.rs` becomes `rebase/`, the ivm tests
   `views/`, and `history.rs` an `eval/` vector that passes because two
   bodies are two hashes. The runtime is Petros minus Diesel, SQL, wasm and
   UniFFI, plus retained facts, snapshots, the overlay store and the
   authority role. `arkc verify`, `print`, `check`, `gen rust` land with it.
2. **The Rust builder and harken's domain through it.** `functions.rs`
   rewritten against `ark-builder`; `schema.sql` as a schema program. One
   log, as today, so `add_to_playlist` keeps its `exists(media)` check.
   The server moves to `ark-server`, the desktop to `rust/ark` with
   generated Rust. **The log starts fresh**, for the reason the first
   revision gave.
3. **The Swift runtime and the iOS app**, in this repository: `swift/` to
   the vectors, `arkc gen swift` over harken's module, a SwiftUI app over a
   maintained view and the generated call surface. `Verify` goes in with it,
   so the first disagreement between two runtimes is a number rather than
   an empty picker.
4. **The Kotlin runtime and the Android app.** Same shape, Compose.
   `mobile/` retires, and with it `ubrn`, the gradle state layer, the Maven
   recording and the qemu wrapping.
5. **The Swift and Kotlin builders**, proved by `frontend/` vectors against
   harken's own functions: the same module from three sources.
6. **As they earn it:** projections and row-level rules; authority handoff;
   the browser runtime; whatever the profile says about generated code.

### 3.19 Risks, stated as risks

- **Three generated codebases from one IR is three chances to diverge, and
  divergence is silent.** The vectors, the fuzzer and `Verify` are the whole
  answer, and the design assumes they exist from the first commit. What is
  better than the first revision: divergence is now *recoverable* (facts,
  snapshot) rather than a reinstall.
- **The generator is a compiler with three backends, and it is the largest
  novel piece.** Idiomatic output, readable names, and error messages from
  the verifier decide whether authoring feels like the language or like a
  linter. Budget for it as the main cost.
- **Every exact peer holds everything.** One log means no partial
  replication, no per-person log and no read rule finer than the whole log
  for a peer that replays; a large library, or data one person should not
  receive, is where that shows first. `docs/scopes.md` records the design
  that answered it and the questions a return to it must settle.
- **The IR's ceiling still exists**, though compilation makes it cheap to
  raise: a new `std` function is a mapping per generator and an
  implementation per `ArkStd`, with vectors. It should still be raised
  slowly; everything in it is in the log forever.
- **Retention above the horizon is a real cost** — entries, facts, function
  versions — and the horizon policy is a knob somebody has to set. It is
  smaller than what Replicache's CVR or `zero-cache`'s replica keep, and it
  is bounded, but it is not nothing.
- **Authority transfer is deferred**, and a serverless peer that later wants
  a server *and* wants to stay able to work alone will want it. The
  adoption step is enough for harken; the handoff is a protocol to design
  when a domain needs it.
- **Performance of generated code is not the risk it was**: it is native.
  The floor is one fsync per tap, and the numbers Petros keeps at pending
  depth 5 and 400 come back as the first Swift milestone's table.
- **Not verified here: any of it.** The first `eval/` vector that runs
  through generated Swift will move something in this document, and should.

### 3.20 Decisions, one paragraph each

- **The domain is an IR authored through builders and compiled to native
  source; nobody runs the IR.** Sameness across languages is the
  requirement; a compiler from one description to three targets, held to
  one suite, is the tool for it, and native code is what a native app runs.
- **Builders, not macros, and the three are symmetric.** A builder is an
  ordinary typed library in each language; authorship leaves no trace in
  the module; the `frontend/` vectors hold one hash across all three. Kotlin
  authors as well as anyone, because builder DSLs are what Kotlin does well.
- **Intents are the log, because only intents can be adopted, verified and
  rebased later.** A peer with no server is a database with one replica,
  not a client in a mode.
- **Facts are retained beside the log and are the fallback, not the
  truth.** A peer applies by intent when it can and by facts when it
  cannot, and ends in the same state. Old clients age gracefully;
  divergence heals.
- **One log per module.** One sequence, one authority, every reference
  checked, any function over any table. The partitioned design, and what it
  would take to bring back, is in `docs/scopes.md`.
- **Snapshots are verifiable and the horizon bounds everything.** Above it,
  local-first exactness costs retention; below it, the design is in
  Replicache's and Zero's steady state.
- **Authority is a role.** A server is a peer that sequences for others.
- **Functions are content-addressed and entries name them.** The log
  freezes meaning; versions live above the horizon and nowhere else.
- **The store is an ordered KV with batch commit, layered under an
  overlay; SQLite is a backend and not a dependency.**
- **The standard library is the only escape hatch and Unicode is pinned as
  data.** Compilation makes admission cheap; permanence makes it slow.
- **Views and live frame types are in the spec**, so the native app gets
  what the desktop has, from one declaration.
- **One repository, native builds per language, vectors pinned by
  revision.**
- **harken's log starts fresh.**
- **Everything Petros got right is kept by name.** The point of a successor
  is to keep the decisions and change the substrate they were paying for.
