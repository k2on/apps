# ArkDB

A design for the successor to Petros. Three parts: what Petros is and what
harken does with it, what the compiled-domain-over-FFI approach costs now that
the phone is becoming two native apps, and the architecture of a system in
which the domain is *data* rather than a binary — authored in Rust, Swift or
Kotlin, carried in one specified encoding, and executed by a runtime in each
language that is held to one conformance suite.

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

**The domain is data.** A mutation or a query is not compiled code that a
peer links or loads; it is a value in a small, total, statically typed
language — Ark IR — carried in one canonical encoding, verified by a checker,
and executed by a runtime in whichever language the peer is written in. The
host language is a *frontend*: a Rust function decorated `#[ark::mutation]`
does not run on a phone, it *emits* an Ark function, and the Swift and Kotlin
runtimes execute that. A Swift function decorated `@ArkMutation` emits the
same kind of thing, and a Rust server can run it.

Everything else follows from that, and most of Petros survives it:

| kept from Petros, unchanged in meaning | dropped | changed |
|---|---|---|
| intents, not facts; `apply` reads to decide | SQLite as a requirement; Diesel; SQL anywhere | the store is an interface with an ordered-KV shape, and SQLite is one backend of it |
| append-only totally ordered log; `replay(confirmed) then replay(pending)` | the savepoint rebase | the rebase is an in-memory overlay the store spec defines |
| all non-determinism in `fill_auto`, once, at origin | wasm, wasmi, the guest ABI | the module a peer receives is Ark IR, not machine code |
| `Ctx { user, session }`, actor held to the login that pushed it | UniFFI, `ubrn`, Expo, `petros-js` | one runtime per language, no bridge |
| typed writes that report `Change`; queries as data; trees via declared relationships | `tables!` reading `schema.sql` through SQLite | the schema is declared in the same IR and codegen emits row types for every language |
| incremental views: source/filter/join/take/tally, `Rebuilt` after a rebase | `Changes::Rebuilt` as the only answer after a rollback | still the answer; but views are spec'd so every runtime maintains them natively |
| sans-io client and server state machines; deterministic simulation | | the simulation's vectors are part of the spec and run against every runtime |
| three lifetimes: log, live room, frame; rooms per account; snapshot on empty | | live frame types are declared in the module, so they are generated everywhere |
| server pings; token in `Hello`; `Denied`; server as the only OIDC client | | |
| "the log freezes arguments, not meaning" as a hazard | the discipline as the only defence | **entries name the function that authored them by hash, and the server keeps every function it ever accepted** |

### The layers

```
  ┌──────────────────────────────────────────────────────────────────────────┐
  │  frontends       Rust #[ark::mutation]   Swift @ArkMutation   Kotlin ark { }   │
  │                  each emits ───────────────────────────────┐             │
  ├────────────────────────────────────────────────────────────▼─────────────┤
  │  Ark module  (canonical CBOR, content-addressed)                          │
  │     schema · functions (mutators, queries) · live types · min spec        │
  ├──────────────────────────────────────────────────────────────────────────┤
  │  arkc            verify · hash · check (log compat) · gen {rust,swift,kotlin,ts} · vectors   │
  ├──────────────────────────────────────────────────────────────────────────┤
  │  runtime, one per language, all held to ark-spec/vectors                 │
  │     codec · verifier · evaluator · stdlib · store (layered) · views       │
  │     · peer (client state machine) · server core · live rooms · protocol  │
  ├──────────────────────────────────────────────────────────────────────────┤
  │  backends        sqlite-as-btree · memory · (later) lmdb, indexeddb       │
  │  transports      URLSession / OkHttp / tungstenite / axum — thin           │
  └──────────────────────────────────────────────────────────────────────────┘
```

The spec is the middle three rows. A backend and a transport are each
implementation's business, provided the store behaves as specified.

### 3.1 Values and the canonical encoding

The value model is small on purpose, because every type is a type three
evaluators have to agree about to the byte.

| type | notes |
|---|---|
| `Null` | only as the absent case of `Option<T>` |
| `Bool` | |
| `Int` | signed 64-bit. Arithmetic is *checked*; overflow is a refusal, not a wrap. `wrapping_*` is not offered; a hash is a stdlib function |
| `Text` | valid UTF-8. **Compared by code point** (= UTF-8 byte order), never by locale, never by canonical equivalence. Swift's `String ==` treats `é` and `e◌́` as equal; Kotlin compares UTF-16 units, which misorders astral characters. Both runtimes compare bytes explicitly, and the vectors include a canonically-equivalent pair and a U+1F3B5 |
| `Bytes` | |
| `Id<table>` | 16 bytes; CBOR tag 37 (UUID). The table is a *type*, not a runtime tag: the encoding is the same 16 bytes Petros writes today |
| `List<T>` | |
| `Struct` | named, ordered fields; a row type or a query result type |
| `Option<T>` | `Null` or `T`; a nullable column is one |
| `Enum` | fieldless, for live frames and arguments (`Kind::Phone`); encoded as its name |

**No floats.** Petros already forbids them in control flow; Ark forbids them
in the model. A gain is an `Int` in millibels, a position is milliseconds.
If a float type is ever needed it is a spec version bump with a defined
canonical NaN and no equality in the IR, and that is a decision for the day a
domain needs one rather than a slot left open.

**One total order over all values**, needed for `ORDER BY`, index keys and
the state hash: by type rank first (`Null < Bool < Int < Text < Bytes < Id <
Enum < List < Struct`), then within a type as above; lists and structs
lexicographically. Every runtime implements the one comparison and the
vectors pin it.

**Encoding: RFC 8949 §4.2.1 core deterministic CBOR**, plus the type mapping
above. Petros chose CBOR for compactness and self-description; Ark keeps it
and adds *determinism*, which is what makes a hash of a value meaningful:
shortest-form integers, definite lengths, map keys sorted by their encoded
bytes, no indefinite strings, no duplicate keys. Libraries exist in every
language; the spec only forbids what the RFC leaves optional. A runtime that
emits non-canonical bytes fails the codec vectors before it does anything
else. Internally tagged maps with `"t"` stay, for the reason `decisions.md`
gives: adding a field is compatible and a tag is a name rather than a
position.

**A state hash is specified**, not a testkit convenience: SHA-256 over the
canonical encoding of `[table_name, [rows in primary-key order]]` for every
table in name order. That is what lets a Swift phone and a Rust server say
whether they agree, in production, which Petros cannot do today (§3.6,
`Verify`).

### 3.2 The schema

A schema is a value in the module, not SQL text. It says what `tables!` used
to ask SQLite:

```
Table { name, columns: [Column { name, ty, nullable }], key: [column],
        indexes: [Index { columns, unique }], refs: [Ref { column, table }] }
```

From it every runtime derives what Petros derives from `PRAGMA
foreign_key_list`: a primary key is `Id<self>` if it is 16 bytes; a `Ref`
column is `Id<that table>`; both directions of every reference are
relationships (`Song.playlist_item` down, `PlaylistItem.song` up), and
reading down keeps a childless parent while reading up drops an orphan. The
names are the same, because the rule producing them is in the spec rather
than in a proc macro.

What the *evaluator* enforces, identically everywhere, as a deterministic
refusal: not-null, unique indexes, and references on `put`. Petros leans on
SQLite's `foreign_keys = ON` for the last of those and treats the result as a
verdict; Ark cannot lean on a backend it does not require, so the check is in
the spec, and a backend is never asked to know what a reference is.

A schema is additive-only across versions, and a column can be *retired*
(reads as its default, writes are dropped) rather than removed, because
functions already in the log may name it — see §3.7. An app's tables are still
a function of the log, so a change to a live column's type is still a rebuild
and a replay, exactly as `SCHEMA_VERSION` does today.

### 3.3 Ark IR: functions

A module carries two kinds of function. Both are typed, both are verified,
and the verifier is part of the spec: a runtime executes only a module that
passed it, and the module's hash is of the verified form.

**A mutator** is `(ctx, autos, args) -> ()` with an effect list. Its body is
a statement list in a deliberately small imperative core:

```
stmt ::= let x = expr
       | if expr { stmts } else { stmts }
       | for x in expr { stmts }          -- over a List; finite by construction
       | put(table, expr)                 -- reports Add | Edit{old,new}; refuses on constraint
       | delete(table, key-expr)          -- reports Remove; a missing row is a no-op
       | refuse(expr: Text)               -- the deterministic verdict, ends the mutator
       | return
```

There is no `while`, no recursion and no user-defined function that can call
itself; every mutator terminates, which is what lets a server run untrusted
modules from an app with no timeout to tune. Autos are declared in the
signature by type exactly as Petros classifies parameters — `NewId<T>`, `Now`
— and a mutator may declare more than one `NewId`, since the one-uuid limit
was `fill_auto`'s and not the log's. The natural-key guidance harken arrived
at (an album is keyed by its name because two offline peers agree on a name
without being told) is unchanged and still the better design; it is simply no
longer forced.

**A query** is `(args) -> T` and has two stages, named because they are
maintained differently:

- the **plan**: `from(table)`, `filter(pred)`, `order(cols)`, `take(n)`,
  `related(rel, plan)` — Petros's `Plan` and `Pipeline`, unchanged. This is
  what a view maintains incrementally.
- the **shape**: a pure expression over the plan's rows — `map`, `filter`,
  `group_by`, `lookup` (an in-memory index of a second plan, which is what
  harken's `track_details` builds by hand with `BTreeMap`s), `sort_by`,
  `first`, `count`, struct construction. Re-run over the maintained rows on
  every change; cheap because it never touches the store.

harken's sixteen queries all fit that split today: `library()` is a plan
with one `related`; `album()` is a plan plus a `lookup` and a `sort_by`;
`track_details()` is a plan plus three `lookup`s. The split is also the
honest statement of what is maintained: the plan is O(changed), the shape is
O(rows in view), and a query author can see which stage a line is in.

**Expressions** are the usual pure core — literals, arguments, locals, field
access, comparison and boolean operators, checked `Int` arithmetic, `Option`
handling (`is_some`, `unwrap_or`, `match`), list operations, and calls into
the standard library. A `select(plan)` expression is how a mutator reads;
`get(table, key)` and `exists(table, key)` are the point lookups.

**The standard library is the whole of the escape hatch, and there is no
other.** Anything a mutator can compute is in the spec with an exact
definition and vectors. From reading `functions.rs`, v1 needs:

- text: `trim`, `is_empty`, `concat`, `len` (code points), `starts_with`,
  `split_once`, `lower`, `is_alnum`, `chars`, `text_of(int)`, `hex(bytes)`
- int: `min`, `max`, `clamp`, `abs`
- hash: `fnv1a64(bytes) -> Int` (harken's `key_part` fallback) and
  `sha256(bytes) -> Bytes`
- id: `id_of(text)`, `text_of(id)`, `nil`
- list: `map`, `filter`, `first`, `last`, `len`, `any`, `all`, `sort_by`,
  `fold`, `group_by`, `lookup`, `contains`

**And two of those are where three runtimes would silently disagree.**
`lower` and `is_alnum` are Unicode operations; Rust's `char::to_lowercase`,
Swift's `lowercased()` (ICU) and Kotlin's `lowercase()` (ICU, a different
version per OS release) do not agree on every code point, and harken's
`slug` — which keys every work, movement and recording in the log — is built
on both. SQLite's own answer is instructive: its `lower()` is ASCII-only
unless compiled with ICU, precisely to be the same everywhere. Ark takes the
other route with the same goal: **the spec pins a Unicode version and ships
the three tables it needs** (White_Space for `trim`, Alphabetic ∪ Numeric for
`is_alnum`, the simple lowercase mapping for `lower`) as data in the spec
repository; every runtime embeds them and none calls the platform. The
tables are conformance-tested like everything else, and moving the Unicode
version is a spec version bump. Anything a runtime cannot promise to compute
identically is not in the library; that is the test for admission.

**Determinism rules**, restated for the IR and now *checkable* by the
verifier rather than kept by convention: no clock, no randomness, no I/O,
no floats, every `select` in a mutator has a total order (the verifier adds
the primary key as a tie-break when the author did not), iteration is only
over lists, and integer arithmetic is checked. `history.rs`'s hazard is
addressed separately (§3.7); these rules address everything else.

#### A worked example

`add_to_playlist`, authored in Rust with the frontend (§3.4). This is
harken's function almost line for line — the frontend accepts a subset of
Rust and this is inside it:

```rust
#[ark::mutation]
pub fn add_to_playlist(db: &Db, ctx: &Ctx, added_ms: Now,
                       playlist_id: Id<Playlist>, media_id: Id<Media>) {
    if !db.exists(Playlist::key(playlist_id)) { return; }
    if !db.exists(Media::key(media_id)) { return; }
    if db.exists(PlaylistItem::key(playlist_id, media_id)) { return; }
    let last = db.select(PlaylistItem::all()
                         .filter(PlaylistItem::playlist_id.eq(playlist_id))
                         .order_by(PlaylistItem::pos.desc()).limit(1))
                 .first().map(|r| r.pos).unwrap_or(0);
    db.put(PlaylistItem { playlist_id, media_id, pos: last + 1, added_ms, user_id: ctx.user.id });
}
```

What it emits, in the printable form `arkc` uses for diffs and for the
successor of `mutations.txt` (the canonical form is CBOR; this is its
diagnostic rendering):

```
mutation add_to_playlist(added_ms: Now, playlist_id: Id(playlist), media_id: Id(media))
  if not exists(playlist, [playlist_id]) { return }
  if not exists(media, [media_id]) { return }
  if exists(playlist_item, [playlist_id, media_id]) { return }
  let last = unwrap_or(map(first(select(from playlist_item
                                          where playlist_id = $playlist_id
                                          order pos desc, media_id asc limit 1)), .pos), 0)
  put(playlist_item, { playlist_id: $playlist_id, media_id: $media_id,
                       pos: checked_add(last, 1), added_ms: $added_ms, user_id: ctx.user })
```

Note the verifier added `media_id asc` to the order: a `LIMIT 1` over a
non-total order is exactly the kind of thing two backends answer differently.

What a Swift app calls, generated by `arkc gen swift`:

```swift
extension Harken {
    /// Put a track on a playlist. A no-op if either is missing or it is already there.
    public func addToPlaylist(playlist: Id<Playlist>, media: Id<Media>) throws
}
```

The body is not in the Swift; the Swift runtime evaluates the IR the module
carries. The doc comment travels, the argument types are branded, and there
is no JSON, no `bigint` and no string-typed id anywhere in the path.

`create_playlist`'s per-person uniqueness refusal is the same shape with a
`select … where user_id = ctx.user and name = $name` and a `return` if it is
non-empty; `add_song`'s `art_to_write` is a `let` with a `match` over an
`Option<Text>`; `add_all_to_playlist` is a `for` over a `select`. Nothing in
harken's ten mutators needs more than the statement list above and the
library listed. That was checked against `functions.rs` rather than assumed,
and it is the reason the library is the size it is.

### 3.4 Authoring in each language, and `arkc`

"Queries and mutators can be defined in whatever language you want" means
each language has a **frontend** that produces Ark IR, and the IR is the
program. Three frontends, and they are not equally easy, which is worth
saying plainly:

- **Rust: a proc macro over a Rust subset.** `#[ark::mutation]` and
  `#[ark::query]` parse the function body with `syn` and translate `let`,
  `if`/`else`, `for`, `match` over `Option`, `return`, method calls on the
  generated row and query types, and the operators, into IR. Anything
  outside the subset is a compile error naming the construct ("`while` is
  not Ark; iterate a list"). Row types, column constants and relationships
  come from `ark::schema!`, which is `tables!` without SQLite: the schema is
  written in the same DSL, and the macro generates the Rust from it. This is
  the frontend harken's domain is ported to, and it is the reference.
- **Swift: a macro over a Swift subset.** Swift 5.9 macros see the syntax
  tree, so `@ArkMutation` is the same translation over the same subset;
  `@ArkSchema` generates the row structs. Result builders were considered
  and rejected: a body written as `If(cond) { … }` is a second dialect to
  learn, and the point is that a domain author writes the language they know.
- **Kotlin: a builder DSL, with KSP generating the typed surface.** Kotlin
  has no stable macro system; a compiler plugin is the only way to translate
  a body and it breaks across compiler versions. So the Kotlin frontend is
  `mutation("add_to_playlist") { … }` with lambdas-with-receivers, and it is
  more verbose than the other two. Kotlin *runs* Ark as well as anything;
  *authoring* in it is the weakest of the three, and a team should know that
  before choosing where the domain lives.

**The emit step is a build step.** A frontend produces a `.ark` module when
its crate, package or module is built: `cargo run --bin emit`, a SwiftPM
plugin, a Gradle task. What comes out is identical whichever frontend made
it, which is the property the conformance suite holds for frontends too:
the same function authored in each language must hash to the same bytes.
That is a *stronger* statement than "they behave the same", and it is
checkable by a file compare.

**`arkc`** is one CLI, in Rust, that does everything that is about a module
rather than about running one:

| command | what |
|---|---|
| `arkc verify m.ark` | type-check, determinism rules, totality; prints the module hash |
| `arkc print m.ark` | the diagnostic text form; what a diff shows |
| `arkc check old.ark new.ark` | log compatibility (§3.7): every function in the old surface still present, arguments never retyped or retabled, schema additive |
| `arkc gen rust\|swift\|kotlin\|ts m.ark out/` | row types, branded ids, typed call surface with doc comments, query result types, live frame types, and the module embedded as a resource |
| `arkc vectors` | runs the conformance vectors against the Rust reference and, with `--fuzz`, generates new ones (§3.11) |

`arkc gen` is the successor of `petros-codegen`, and the difference in kind
is that it reads a typed module rather than a text section: it can generate
a Swift `enum Kind { case computer, phone, speaker }` from the live section
and a `struct Item` from a query's result type, where today those are
mirrored by hand.

**Transpiling bodies to native source is deliberately not v1.** `arkc gen`
could emit a Swift body per function and let a runtime skip the interpreter.
It is not done first because the interpreter is what the versioning story
needs anyway (a peer must be able to run a function it received over the
wire), because an interpreted mutator at harken's scale is well under a
millisecond (§3.14), and because a transpiler is a fourth implementation to
hold to the vectors. It is the right optimisation later and the vectors are
what would prove it.

### 3.5 The store: layered, backend-agnostic, no SQL in the spec

The store the evaluator runs against is an interface, and the spec defines
its semantics rather than its file format:

```
get(table, key) -> Option<Row>
scan(table, index, lo, hi, dir, limit, after) -> [Row]     -- sorted by the index, seekable
put(table, row) -> Change | Refusal                         -- enforces not-null, unique, refs
delete(table, key) -> Option<Change>
```

That is Petros's five-method `Store` with `fetch(plan)` replaced by `scan`
over a declared index — the evaluator compiles a plan to index scans itself,
so a backend never sees a filter. A backend is an **ordered key-value store
with atomic batch commit**, which is what every candidate already is:
SQLite's B-tree, LMDB, LevelDB, IndexedDB, a `BTreeMap`.

**SQLite stays, as a B-tree and not as a database.** The recommended durable
backend on iOS, Android and the desktop is SQLite with two tables — rows
keyed by `(table_id, canonical key bytes)` holding the canonical row, and
index entries keyed by `(index_id, canonical index key bytes)` — because it
is on every device already, its durability story is understood, and its
`synchronous`/WAL knobs are the ones Petros already measured. What changes
is that the domain never sees SQL, the spec never mentions it, and swapping
it for LMDB on a server is one file. "We can ditch SQL" is answered by
ditching the *dependency on SQL semantics* — collation, `NULL` comparison,
the planner — which is where the cross-language hazards were, while keeping
the storage engine that is not the problem.

**The rebase without a savepoint.** A client's store is *layered*:

```
   reads:  overlay (pending)  →  base (confirmed, durable)
   confirmed entry arrives:   drop the overlay; apply confirmed into base in one batch;
                              re-run every still-pending intent into a fresh overlay
   ack:                       remove the intent from the pending queue; if the queue is
                              empty, drop the overlay (the base already has it)
```

The overlay is an in-memory map of `(table, key) → Option<Row>` plus shadow
index entries; a read consults it first and falls through. It is exactly what
`SAVEPOINT pending … ROLLBACK TO pending` did, expressed as a data structure
any backend can sit under, and it keeps Petros's two properties: pending
intents are committed durably (a system table in the base, written in the
same batch as nothing else, so a tap is one fsync), and the optimistic state
never is. A mutation still costs the same at pending depth 400 as at 5
because a local write applies forward into the overlay and does not replay.
`Rebuilt` is still what a view is told after a rebase, for the same reason:
dropping an overlay reports nothing.

The server's store has no overlay; it applies each pushed intent into a
transaction on its base and commits or discards it with the verdict.

### 3.6 The log and the wire

The protocol is Petros's, written down as a specification instead of a Rust
enum, with three additions. Frames are canonical CBOR maps tagged `"t"`.

```
Entry     { id: Id, seq?: Int, actor: Text, session: Text, fn: Bytes(32), args: Struct, autos: Struct }

client →  Hello   { since: Seq, token?: Text, spec: Int, module?: Bytes(32) }
          Push    { entries: [Entry] }
          Say     { say: Bytes }                      -- live, opaque to the engine
          Need    { fns: [Bytes(32)] }                -- functions this peer lacks
          Verify  { seq: Seq, hash: Bytes(32) }       -- "this is my state at seq"

server →  Batch   { entries: [Entry], has_more: Bool }
          Ack     { ids: [Id], seqs: [Seq] }
          Reject  { id: Id, reason: Text }
          Denied  { reason: Text }                    -- nothing follows; socket closes
          Heard   { hear: Bytes }
          Module  { hash: Bytes(32), fns: [Function], schema: Schema }   -- answer to Need, or offered on Hello
          Agree   { seq: Seq, hash: Bytes(32), ok: Bool }
```

The additions:

- **`fn` on the entry is a hash of the function that authored it**, not a
  verb name. `args` and `autos` are the two halves Petros already keeps
  apart. Replay looks the function up by hash (§3.7).
- **`Need`/`Module`** is how a function body reaches a peer that has never
  seen it: a phone that installed last month replays an entry authored by a
  verb added yesterday by asking for it. The server's `Hello` reply offers
  the current module hash, and a peer that lacks any function in a `Batch`
  asks before applying. This is the "server hands the module out" step
  `decisions.md` names as the one that closes the older-client problem, and
  it is cheap because a module is kilobytes of IR rather than a megabyte of
  wasm.
- **`Verify`/`Agree`** is the state hash (§3.1) exchanged on request. A peer
  that has caught up to `seq` may say what it holds; a server that disagrees
  says so. Petros can only find divergence in a test. Ark can find it in a
  house, name the sequence number it appeared at, and — because every
  function is content-addressed — say which function two runtimes evaluate
  differently. That is the production half of the conformance suite.

Everything else is as it was: `seq = head + 1`, dedupe by entry id, the
server applies before it appends so a `Reject` is a verdict, fan-out is
"everything above your cursor, 256 at a time", `Heard` is taken before the
rebase, identity is asked once at `Hello` and every pushed entry is held to
it, the server pings and no client has to. `spec` in `Hello` is the runtime's
spec version, and a server whose module needs a newer one answers `Denied`
with a reason a client shows as "update required" — the handshake Petros
deferred, now about the runtime alone rather than the app.

Batching, `APPLY_CHUNK`, and the cursor semantics are carried over and are
part of the protocol vectors.

### 3.7 Versioning: the log freezes meaning too

This is the one place Ark is not a translation of Petros but a repair.

**Every function is content-addressed.** Its hash is SHA-256 of its
canonical, verified encoding. A module is a map from name to hash plus the
bodies. An entry records the hash of the function that authored it.

**The server keeps every function it has ever accepted**, by hash, in a
system table beside the log. A module install adds bodies and never removes
one — the same permanence rule the log has, now applied to what the log
*means*. Bodies are small; harken's whole domain is a few tens of kilobytes
of IR.

**Replay runs the function that authored the entry.** A peer replaying
sequence 1 today runs the `add_song` of the day sequence 1 was written, not
today's. `tests/history.rs` — two peers on one log ending at `[1,2,3]` and
`[10,20,30]` — cannot happen, because both peers run the same body for the
same entry by construction, and neither has to have been built at any
particular time. The discipline "a verb's meaning is immutable; add a verb"
becomes a fact rather than a rule: editing a function *is* adding a
function, and the old one is still there under its old hash for the entries
that name it.

What that costs, and how it is bounded:

- **The schema must stay readable by every historical body.** A function
  from last year names columns; if today's schema dropped one, that body
  cannot run. So the schema is additive-only, a column is *retired* rather
  than removed (it reads as its default and a write to it is dropped), and
  `arkc check` type-checks *every accepted body* against a proposed schema
  before the server will install it. `check-log` compared a text surface;
  this is a compiler pass over the log's whole vocabulary, and it refuses a
  deploy that would strand an entry.
- **A schema change that changes a live column's meaning is still a
  rebuild.** Tables are a function of the log; `schema_version` moves and
  every peer replays into fresh tables. Unchanged from Petros, and every
  peer does it — including the phone, whose runtime is not a different kind
  of thing any more.
- **Two runtimes disagreeing about one body is the remaining hazard**, and
  it is the one the conformance suite and `Verify` exist for. It is a
  smaller hazard than today's: it can only come from a runtime bug in a
  spec'd operation, never from a domain edit.

There is a second, quieter consequence. A verb missing from `peer!` was
invisible because the schema line and the dispatch were two lists. In Ark
there is one list: a module's functions *are* what can apply, the call
surface is generated from them, and there is no way to declare a function
the evaluator cannot find.

### 3.8 Views, in every language

Petros's incremental views are kept whole — `Source`, `Filter`, `Join`,
`Take` with a per-partition bound that refills by seeking, `Tally` — and
moved from "a Rust crate" to "a section of the spec", because a query's plan
is IR and a native app is exactly the caller that wants to hold a
maintained list and drive a `List`/`LazyColumn` from `Insert{at}`,
`Remove{at}`, `Update{at}` patches without crossing anything. The phone
gets what the desktop has today, natively, and `library_update()` as a
bridge type disappears.

Two things the spec says that the crate only tested:

- **The correctness contract**: after any sequence of changes, a view's
  rows equal a re-run of its plan. The view vectors are exactly that —
  plan, initial rows, change list, expected patches, expected final rows.
- **The work contract**: a push costs O(changed), and a `Take` refill is a
  seek, not a scan. `docs/ivm.md` found three bugs that only a pull counter
  could see; the spec states the bound and each runtime's own tests measure
  it, because a vector cannot count another implementation's reads.

A view is fed by the peer after every applied entry and every local
mutation, and told `Rebuilt` after a rebase. The `shape` stage of a query
(§3.3) is re-run over the view's rows when they change, so a screen that
wants harken's `Item` — a row plus its playlist membership plus a derived
`on_playlist` — subscribes to one thing.

### 3.9 Live rooms

Unchanged in design: a room per account, held in the server's memory, the
app's machine deciding what a frame means, `keep` for the one row worth
writing when a room empties, frames dropped while unlinked, a second `Hello`
on a connection is paging and not a departure. What changes is that a
module's **live section declares the frame types** (`Say`, `Hear`, and the
structs and enums inside them), so `arkc gen` emits them for every language
and `mobile/src/listening.ts`'s hand-written `Listener` and `Doing` do not
have a successor. The state machine each device runs (`elsewhere`,
`output_here`, the 1100 ms drift rule) is app code and stays app code — but
it can be written *once* as a pure Ark query over a `Session` struct if an
app wants one definition, which is how harken should do it given that it
has already written it twice.

The engine still never looks inside a frame, so none of the log's
permanence rules bind one.

### 3.10 Sign-in

`petros-auth`'s design is kept and its client half becomes a section of the
spec, because a Swift and a Kotlin client each have to implement it: open
`{server}/auth/login?redirect=R`, receive a single-use `code`, `POST
/auth/exchange` for `Login { token, session, user, expires_ms }`, put the
token in every `Hello`. Three HTTP calls and a URL scheme. The server
remains the only OpenID Connect client, the dev mode remains, and `Denied`
remains how a stale token is learned about.

### 3.11 The conformance suite

The spec is a document; the vectors are what make it binding. They live in
`spec/vectors/`, are generated by the Rust reference and reviewed like code,
and every runtime carries a `conformance` test target that walks the tree.
A runtime's version states the spec version it passes, and CI in every
runtime pins the vectors by revision.

| directory | one vector is | what it holds |
|---|---|---|
| `codec/` | a value in diagnostic JSON and its canonical bytes as hex, both ways | RFC 8949 determinism, tag 37, the type mapping; a non-canonical input that must be *refused* |
| `order/` | a list of values and their sorted order | the total order; UTF-8 vs UTF-16 vs canonical-equivalence traps |
| `stdlib/` | a call, its arguments, its result or its refusal | every library function; the Unicode tables at their pinned version; checked-arithmetic overflow |
| `verify/` | a module and whether it verifies, with the error | typing, determinism rules, totality, order tie-break insertion |
| `eval/` | schema, functions, initial rows, an entry with ctx and autos, expected changes and final rows or the refusal text | the evaluator; harken's ten mutators are the first real ones; `fill_auto` from a fixed seed |
| `views/` | a plan, initial rows, a change list, the expected patch list and final rows | §3.8's correctness contract, including per-partition `Take` refills and `Child` placement |
| `rebase/` | a seeded scripted session: mutations, partitions, heals, deliveries with duplicates and drops, expected state hash at settle | Petros's simulation tests, made portable; a Swift peer and a Rust server in one run |
| `protocol/` | a frame sequence into a peer or server and the frames and state out | `Hello`/`Batch` paging, dedupe re-acks, `Denied`, `Need`/`Module`, `Verify`/`Agree` |
| `hash/` | a set of tables and the expected state hash | §3.1, so `Verify` means one thing everywhere |
| `frontend/` | the same function in each language's source and the one hash it must emit | §3.4's property, per frontend |

Vectors are JSON with `{"$bytes": hex}`, `{"$id": uuid}` and `{"$int": "…"}`
wrappers where JSON cannot say the thing, and every vector carries the
expected canonical bytes so a runtime is tested on encoding as well as on
meaning.

**Differential fuzzing is the second half.** `arkc vectors --fuzz` generates
random modules over random schemas and random entry sequences, evaluates
them on the reference, and emits vectors; a nightly job in each runtime
pulls the day's batch. Three hand-written evaluators agreeing on a curated
suite is a start; three evaluators agreeing on ten thousand programs nobody
wrote is the thing that justifies putting a mutator authored on a phone into
a log a server replays. The lesson from `docs/ivm.md` and the three vacuous
tests in `decisions.md` applies to the vectors themselves: every vector
directory has a *falsification* check — a deliberately wrong reference
answer that the runner must reject — so a runner that passes by not reading
the expected value is caught.

### 3.12 Repository layout

One repository, because the vectors and the runtimes move together and a
spec change that is not accompanied by a runtime change is a spec change
nobody tested. Each language keeps its native build; nix wraps the Rust
half and CI.

```
arkdb/
  spec/
    SPEC.md                  numbered sections: values, encoding, order, schema, IR, stdlib,
                             store semantics, log and protocol, views, live, sign-in, hashing
    unicode/                 the three pinned tables, generated from UCD, with the generator
    vectors/                 §3.11
  rust/
    ark/                     the runtime: codec, ir, verify, eval, stdlib, store (layered),
                             views, peer, server core, live, protocol
    ark-store-sqlite/        SQLite as a B-tree
    ark-store-mem/           BTreeMap, for tests and the browser
    ark-server/              axum: the handler, the hub, `exchange`, the module store, auth
    ark-macros/              the Rust frontend and `ark::schema!`
    arkc/                    the CLI
    ark-testkit/             `Sim<A>` over the runtime; emits `rebase/` vectors
  swift/
    Package.swift            ArkDB (codec, eval, store, views, peer), ArkMacros, ArkConformance
  kotlin/
    settings.gradle.kts      ark-runtime (jvm + android), ark-ksp, ark-conformance
  ts/                        later: the browser runtime, same shape, for a web client
```

An app — the two native harken apps this repository (`k2on/apps`) is for —
depends on `swift/` or `kotlin/` as a package, on `arkc` as a build tool,
and carries its domain as a Rust crate that emits a module, or authors it in
the app's language. The desktop and the server depend on `rust/ark`.

### 3.13 The path for harken

In order, each step leaving something that runs:

1. **Spec and Rust runtime, with vectors from day one.** Port Petros's
   tests as vectors before porting its code: `wire.rs` becomes a `codec/`
   and `protocol/` vector, `converge.rs` becomes `rebase/`, the ivm tests
   become `views/`, `history.rs` becomes an `eval/` vector that *passes*
   because two bodies with two hashes are two functions. The Rust runtime is
   Petros minus Diesel, SQL, wasm and UniFFI, plus the evaluator and the
   overlay store; roughly the size of Petros today. `arkc verify`,
   `print`, `check` and `gen rust` land with it.
2. **harken's domain as Ark-in-Rust.** `functions.rs` under
   `#[ark::mutation]`/`#[ark::query]` and `schema.sql` as `ark::schema!`.
   The port is the test of the Rust frontend's subset and of the standard
   library's coverage; anything that does not fit is either a library
   addition with vectors or a sign the function was doing something a
   mutator should not. The server moves to `ark-server`, the desktop to
   `rust/ark`. **The log starts fresh.** harken is "still alpha" by its own
   CLAUDE.md, and a Petros log can in principle be imported (entries are
   CBOR maps; a verb name maps to the ported body's hash) but the replay
   would have to be proven identical, and that proof costs more than the
   library it would save.
3. **The Swift runtime and the iOS app.** `swift/` to the vectors, then
   `arkc gen swift` over harken's module, then a SwiftUI app in `k2on/apps`
   over a maintained view and the generated call surface. No bridge, no
   `.so`, no Expo. This is the first moment two runtimes share a log, and
   `Verify` goes in at the same time so that any disagreement is a number
   on a debug screen rather than a picker that is mysteriously empty.
4. **The Kotlin runtime and the Android app.** Same shape, Compose.
   `mobile/` retires; with it go `ubrn`, the gradle state layer, the Maven
   recording and the qemu wrapping, because none of them were about the
   app.
5. **Afterwards, as they earn it:** a Swift frontend so a domain can be
   authored where an app is; transpiled bodies where a profile says the
   interpreter is the cost; checkpoints (a signed state hash and a snapshot
   at `seq`, which is the compaction Petros never built and which `Verify`
   already gives the vocabulary for); the browser runtime.

### 3.14 Risks, stated as risks

- **Three evaluators are three chances to diverge, and divergence is
  silent.** The whole design rests on the vectors, the fuzzer and `Verify`
  being taken seriously from the first commit rather than added. A runtime
  that ships before it passes the suite is a Petros with more languages.
- **The IR's ceiling is real.** A mutator that needs date arithmetic, a
  regular expression, or a Unicode operation outside the three pinned
  tables cannot have it until the spec does, and the spec should be slow to
  grant it. That is the correct trade for the log's sake; it will still
  feel like a wall the first time somebody hits it. harken's current
  nineteen functions do not.
- **Interpreted performance on a phone is estimated, not measured.** Petros
  measured 0.027 ms for a linked mutation and 0.39 ms through wasmi, with
  the interpreter a minority of the second number. A tree-walking evaluator
  over an in-memory overlay with SQLite underneath should land between
  those; the number that actually matters is one fsync, which nothing here
  changes. The first Swift milestone includes the latency table Petros
  keeps, at pending depth 5 and 400.
- **The Rust frontend is a language subset with a proc macro as its
  compiler**, and the error messages for "that is not Ark" decide whether
  authoring feels like Rust or like fighting a linter. Budget for them.
- **Kotlin authoring is second-class** (§3.4). If the domain will ever be
  written on the Android side, that is the moment to fund a compiler plugin
  and accept its maintenance.
- **A content-addressed function store grows forever**, like the log. It
  grows by kilobytes per deploy, and it is what makes the log replayable,
  so it is the right thing to grow; but a body can never be deleted, and
  `arkc check` refusing a schema change because of a body from two years
  ago will happen and should be understood in advance as the mechanism
  working.
- **Static typing in the IR must be sound enough that `arkc gen` never
  lies.** A generated Swift signature that disagrees with what the evaluator
  accepts is the `bigint` problem back in a suit. The verifier is part of
  the spec for this reason and gets its own vector directory.
- **Not verified here: any of it.** This is a design written against two
  codebases read closely and one set of measurements taken by their
  authors. The first prototype that runs an `eval/` vector through a Swift
  evaluator will move something in this document, and should.

### 3.15 Decisions, one paragraph each

- **The domain is an IR, not a binary, because sameness across languages is
  the requirement and compilation is the wrong tool for it.** Two `apply`s
  in two languages diverge silently; one `apply` compiled twice diverges
  less often and just as silently; one IR with three evaluators held to one
  suite diverges only by a runtime bug, which a suite can find and a hash
  can name.
- **The IR is total and typed so the server can run it from anyone.** No
  loops that do not terminate, no I/O, no floats, checked arithmetic, every
  order total. A verifier is in the spec and a module's hash is of its
  verified form.
- **The standard library is the only escape hatch, admission requires an
  exact definition and vectors, and Unicode is pinned as data.** Because
  `slug` keys the log and three platforms' ICUs do not agree.
- **Functions are content-addressed and entries name them, so the log
  freezes meaning.** `history.rs` becomes impossible rather than
  discouraged. The server keeps every body forever; the schema is
  additive-only and type-checked against all of them.
- **The module travels over the wire.** `Need`/`Module` is what lets a
  phone replay an entry authored by a verb it has never seen, and it is why
  the interpreter comes before the transpiler.
- **The state hash is specified and exchanged.** `Verify`/`Agree` turns
  "the picker is empty" into "we disagree since seq 4127 about
  `add_song@3f9c…`".
- **The store is an ordered KV with batch commit, layered for the rebase;
  SQLite is a backend and not a dependency.** SQL semantics leave the spec;
  the storage engine that was never the problem stays on the devices that
  have it.
- **Views are in the spec because the native app is the caller that most
  wants them.** The phone gets what the desktop has, without a bridge.
- **Queries have a plan and a shape, named, because they are maintained
  differently and an author should see which is which.**
- **The Rust frontend is a Rust subset under a proc macro; Swift the same
  under a macro; Kotlin a builder DSL.** The IR is symmetric; the
  ergonomics are not, and saying so is cheaper than discovering it.
- **The same function in every frontend must hash to the same bytes.**
  A file compare is a stronger test than a behavioural one and costs
  nothing.
- **One repository, native builds per language, vectors pinned by
  revision.** A spec change without a runtime change is untested by
  definition.
- **harken's log starts fresh.** By its own account it is alpha, and an
  import is a proof nobody needs yet.
- **Everything Petros got right is kept by name**: intents, the total
  order, `fill_auto` at origin, `Ctx`, typed writes that report changes,
  trees from declared references, `Rebuilt`, sans-io machines, the
  simulation, three lifetimes, one socket, the server as the only OIDC
  client, the server pinging. The point of a successor is to keep the
  decisions and change the substrate they were paying for.
