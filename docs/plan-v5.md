# Spec v5: bytes under types, one language, one machine

The design for the fifth revision of ArkDB, and the plan for landing it
beside v4 and measuring the two against each other. It follows from one
principle and the decisions it forces:

> **If the type system is trusted, no code is needed to manage states it
> rules out.** Every place v4 keeps the same fact twice — a runtime tag
> beside a static type, five sub-languages each encoded and checked on
> their own, the peer's lifecycle copied into five places, a `log_id`
> beside a state hash — is a place v5 keeps it once.

Every core feature stays: the intent log with facts beside it; the
rebase; snapshots and the horizon; authority as a role, a peer alone
included; partial sync; versioning by hash; maintained views with
patches; live state; sign-in; input checks that are a form's validation;
a canonical encoding; the conformance suite; peers in other languages.
What changes is how many concepts carry them.

Part 1 is the design. Part 2 is the work, as pieces for implementation
agents. Part 3 is how the result is measured — size and speed — against
v4, which stays in the tree untouched until the numbers are in.
`docs/arkdb.md` remains the architecture's history; where it and this
document disagree, this one is newer.

---

## Part 1 — Design

### 1.1 Types, and bytes under them

There is no runtime value type. A value is bytes, and a **type** says how
to read them. The type grammar has five constructors:

```
Ty = Int                 -- 8 bytes: the i64 with its sign bit flipped, big-endian
   | Bytes               -- escaped: each 00 byte becomes 00 FF; terminated by 00 00
   | List Ty             -- each element preceded by 01; terminated by 00
   | Tuple [Ty]          -- the fields' encodings, concatenated
   | Sum [Tuple [Ty]]    -- one byte, the variant's index; then that variant's tuple
```

This encoding is **canonical** (one value, one byte string), **prefix-free**
(no value's bytes are a proper prefix of another's of the same type) and
**order-preserving** (for two values of one type, bytewise order of their
encodings is the spec's order of the values). Consequences, each used:

- `compare(a, b)` is `memcmp`. An index key is column bytes concatenated.
  The state hash is SHA-256 over row bytes with no re-encoding. There is
  one codec and no separate order.
- Tuples order by their first field, then their second — a row orders by
  its key columns when they come first, which the schema makes so.
- `Sum` orders by variant index: `None < Some`, `false < true`, an enum by
  declaration.
- There are **no mixed-type comparisons**, because the checker (§1.4)
  guarantees both sides of every comparison and every row of every index
  have one type. So there is no rank byte and no "total order over all
  values": there is an order per type, and it is the bytes.

Everything else is a **declaration** over the five, by name, in a
module's type section:

```
Bool         = Sum [(), ()]
Option<T>    = Sum [(), (T)]
Enum         = Sum [(), (), …]              -- orders by declaration; append-only under compat
Text         = Bytes   where utf8
Id<t>        = Bytes   where len 16         -- "the row exists" is a store constraint, not a type (§1.3)
NonEmpty<T>  = List<T> where len ≥ 1
Name         = Text    where trim, len 1..=80, "a playlist needs a name"
Row<t>       = Tuple of t's columns, in schema order; names live in the schema
```

A `where` clause is a **refinement**: a predicate on the values of a
structural type. It is checked exactly once, at the boundary where bytes
enter from outside — an input, a snapshot, a frame, a file — by one
function, `parse(ty, bytes) -> Result<Typed, Refusal>`, whose refusal
message is the one a form shows. Inside, the checker tracks refinements as
types: `concat : Text, Text -> Text` keeps UTF-8 by its signature;
`filter : NonEmpty<T>, … -> List<T>` loses non-emptiness by its
signature; a literal in a module is checked by the verifier once and then
covered by the module's hash. In the host language a refined type is a
newtype with a private field whose only constructor is `parse`, so an
unchecked one cannot exist.

**Hash spaces are distinct types**: `FnHash`, `StateHash`, `ModuleHash`,
`TypeHash`, `EntryHash`, each a newtype over 32 bytes. Comparing across
them does not compile.

**JSON** (`json.rs`'s dialect) stays as the human form of vectors and
traces, printed under a type. It is the only self-describing form, and
nothing on the data path reads it.

### 1.2 The schema

Tables, columns (each a named type from §1.1), keys, declared indexes,
references. Unchanged in substance; it is now the **only** place that
knows what a value is.

**Append-only evolution.** `compat`'s additive-only rule becomes: a new
schema appends columns to a table and appends tables; it never removes,
reorders or retypes. With rows as tuples in schema order, a row written
under schema `v+1` is readable under schema `v` **by prefix**: an older
runtime reads the columns it knows and never sees the rest. There is no
projection code. A column appended must be an `Option` or have a
declared default, so that a row written under `v` is complete under
`v+1` when the prefix ends. The compat check is one function: is the new
schema a prefix-extension of the old.

### 1.3 The store

One ordered key-value map:

```
(table, key_bytes)                          -> row_bytes
(table, index, column_bytes, key_bytes)     -> ()
```

`get`, `scan(lo, hi, dir, limit, after)`, `put`, `delete`. Writes judge
constraints — a unique index, a reference (`Id<t>` must name a row of
`t` at the moment of the write), a non-nullable column — and refuse.
Every write reports a `Change`:

```
Change = Add { table, row } | Remove { table, row } | Edit { table, old, new }
```

A change **inverts exactly**: an `Add` by deleting its key, a `Remove` by
putting the row back, an `Edit` by putting `old`. That is all the rebase
(§1.6) needs. There is no overlay store: the optimistic state is the
confirmed store with the pending change lists applied in order, and the
rebase undoes them newest-first.

The state hash is kept incrementally: per table, the sum mod 2^256 of
`sha256(table_name ‖ row_bytes)` over its rows, moved by one leaf in and
one out per write; the state hash is `sha256(schema_hash ‖ digests in
schema order)`. **The schema hash is in it**, so two stores are equal only
when their bytes mean the same thing.

Tables carry a **space**: `Durable` or `Volatile`. A volatile table is
not journaled, not in snapshots, not in the state hash, and lives in the
authority's memory. §1.9 is what it is for.

### 1.4 One language

The domain — every mutator, query, helper, guard, provider, scope and
live transition — is one typed expression language. The relational
subset is what a query is.

```
Expr = Lit(bytes)                     -- a value of the expression's type
     | Var(sym) | Let(sym, Expr, Expr) | If | Match(Expr, [(variant, [sym], Expr)])
     | Field(Expr, i) | Tuple([Expr]) | Variant(i, [Expr]) | List([Expr])
     | Op(op, Expr, Expr) | Un(op, Expr)              -- checked arithmetic, comparison, bool
     | Std(fn, [Expr])                                 -- the standard library, by name
     | Call(FnHash, [Expr])                            -- a helper, by hash
     | Ctx(field)                                      -- user, session, roles, now, device
     | Lambda([sym], Expr)                             -- only as an operator's argument
     | Scan(table) | Lookup(table, key: Expr)          -- Read
     | Filter(Expr, Lambda) | Map(Expr, Lambda)
     | Join(Expr, Expr, on: Lambda) | Group(Expr, by: Lambda, agg: Lambda)
     | Sort(Expr, keys: Lambda, dirs) | Take(Expr, n)
     | Insert(table, Expr) | Update(table, key: Expr, Lambda) | Delete(table, key: Expr)   -- Write
     | Refuse(Expr)                                    -- the message
     | Seq([Expr])
```

Every expression has a type and an **effect**, `Pure < Read < Write`.
What v4 calls kinds are types and effect bounds:

| v4 | v5 |
|---|---|
| mutator body (`Stmt`) | `(Ctx, Input) → [Change]`, effect `Write` |
| query (`Plan`) | the relational subset, effect `Read`, result `List<T>` |
| scope (`Pred`) | `(Ctx, Row<t>) → Bool`, effect `Pure` |
| input `Check` | a refinement of the input type (§1.1) |
| live transition | `(State, Input) → State`, effect `Pure` |
| guard / provide / helper | ordinary functions with an effect bound |
| router / middleware | a namespace prefix in the binding map; a call at the top of a body |

**Effects are values.** A mutator evaluates to a list of changes; the
interpreter does not mutate the store. The list is what the log keeps
beside the intent, so "run it" and "record what it did" are one
operation, and rebase re-evaluates pending intents and re-applies their
lists. (A body that reads what it wrote earlier in the same body reads
through its own list so far.)

**Total by construction.** A function calls another only by `FnHash`,
and a hash cannot contain itself, so the call graph is a DAG and every
function terminates; the checker needs no termination rule. No runtime
eval. Non-determinism enters at one door: `Ctx(now)` is drawn once at
origin and frozen in the entry (§1.5). Arithmetic is checked and overflow
is a refusal.

**One checker** (types, effects, refinements by signature) and **one
interpreter** replace v4's `verify.rs`, `eval.rs`, `view::pull`,
`scope::admits`, `Eval.check` and the transition runner.

**The IR is a value.** `Expr`, `Ty`, the schema and every frame are
`Sum`s of `Tuple`s, so §1.1's codec encodes them, and `#[derive(Node)]`
on their Rust definitions generates the `Ty` of each. There is no
hand-written encoder, decoder, normaliser or hash walk for the IR.
Alpha-normalisation is De Bruijn indices for `sym`, so a function's bytes
do not depend on names.

### 1.5 Functions by hash; a module is a binding map

`FnHash = sha256(enc(function))`, where the function's encoding names the
hashes of what it calls. Function bodies live in a **content store**
keyed by hash. A module is:

```
Module { spec: Int, schema: Schema, types: [(name, Ty)], bindings: [(name, FnHash)] }
ModuleHash = sha256(enc(Module))
```

Two modules binding the same names to the same hashes are the same
module. "Did this authority run this function?" is "is it in the store."
Retiring old versions is garbage collection by reachability from retained
log entries. Compatibility between versions is §1.2's prefix check on the
schema plus one `verify` per new hash. Sending a function a peer lacks is
a store fetch. `ran`, `retire`, `check_retained`, `need_closures` and the
monolithic module codec are gone as separate things.

A binding's name is a path: `playlists.create`. That is what a router is.

### 1.6 The log, chained

```
Entry { id: EntryId, actor, session, roles, fn: FnHash, input: bytes, now: Int }
Logged { seq, entry, facts: [Change], state: StateHash, hash: EntryHash }
EntryHash = sha256(prev.hash ‖ enc(seq, entry, facts, state))
```

Each logged entry carries the state hash after it and the hash of the
chain to it. **The chain is the log's identity.** There is no `log_id`,
no `verify` frame, no `agree`, no `unknown`, no gap check: a `hello` says
`(cursor, EntryHash)`; the authority either has that hash at that
sequence and pages from it, or does not and serves a snapshot. Divergence
is detected per entry, for free, because the hash is already kept.

**Deterministic ids.** `EntryId = sha256(session ‖ local_seq)[..16]`; a
row id drawn inside an entry is `sha256(entry.id ‖ n)[..16]` for the
`n`-th draw. The only frozen non-determinism is `now`. Dedupe is
`(session, local_seq)`, which also detects a gap in a session's pushes.

**Snapshots and the horizon.** A snapshot is `(seq, EntryHash, StateHash,
rows in key order)`; the chain starts from one. Entries below the horizon
are dropped; a `hello` below it is answered with the snapshot.

**Confirmed entries apply by facts.** A replica runs code for its own
pending intents (that is optimistic UI) and for nothing else: every
confirmed entry arrives with its facts and is applied by them. So a
replica needs the bodies of the functions it authors, and the authority
needs the bodies of every function it sequences. The v4 inbox (an entry
awaiting its facts), `Pending::Known` vs `Ran`, `need_closures` on
receive, `held` on receive and the behind-on-intent path do not exist.
`batch` always carries facts. The cost is bandwidth on a bulk mutator
whose facts outweigh its intent; the chain still catches every
divergence.

### 1.7 One peer machine

```rust
struct Peer {
    log: Log, store: Store, cas: Cas,
    pending: Vec<(Entry, [Change])>,        // its own, previewed
    role: Role, feed: Feed,
}
enum Role { Replica, Authority { journal: Journal } }   // "alone" = Authority with a local journal
enum Feed { Whole, Scoped(FnHash) }                     // partial sync: the scope's function (§1.8)

enum Input  { Frame(bytes), Mutate { fn, input, now }, Tick(now), Persisted(seq), Open(Durable) }
enum Output { Send(bytes), Changes([Change]), Persist(bytes), Refused { id, why }, Confirmed(id) }

impl Peer { fn step(&mut self, input: Input) -> Vec<Output>; }
```

Sans-io: no sockets, clocks or files inside. The server hub, the client,
the simulator and the trace player are **drivers** that feed `step` and
route `Output`. Rebase, sequencing, dedupe, judging, compaction, going
alone, joining again, and partial feeds are in this one machine, once.

**The rebase.** A confirmed entry arriving under pending intents: undo
the pending lists newest-first (§1.3 inversion), apply the confirmed
facts, re-evaluate the still-pending intents over the new store and
record their new lists. The view is told what happened as changes like
any other.

**Authority stamps.** The authority writes the roles it judged an entry
under into the entry before sequencing; a replay anywhere uses what the
log says. The server half of a mutator and the authority's raw writes are
entries the authority itself authors, through the same `Mutate` input;
there is no second door.

**Going alone and joining.** An `Authority { journal }` sequences its own
intents; `join` hands its chain to a server, which adopts it by applying
its facts (a chain whose facts disagree with its own run is refused, as
v4's `FactsDiffer` is) and answers with the sequences it assigned; the
peer rebases its chain onto them. Both are the machine's own transitions.

### 1.8 Partial sync is a view

A scope is a `Pure` function `(Ctx, Row<t>) → Bool`. "Serve this
connection its union" is: keep a maintained view (§1.10) of
`Filter(Scan(t), scope)` per table per connection over the authority's
store, and send its patches as the facts of the entries that caused them.
A partial replica is `Feed::Scoped` and, since §1.6, applies facts like
every replica; its own intents are previewed over what it holds and
confirmed by the authority's facts, a difference being the authority's
answer and never a divergence. Whole replicas are the case where the
view is everything. `scope.rs`, the `held`/`partial`/`upto` fields and
the projection of facts are gone.

### 1.9 Live state is a second machine over volatile tables

Live state — presence, playback, who is listening — is tables marked
`Volatile` (§1.3), a second `Peer` over them on the same socket, in the
same frames with a `space` tag. A transition is a mutator over a table
keyed by instance; an ephemeral per-device slot is a row keyed by
`(instance, device)`; `on_join`/`on_leave` are entries the authority
authors on connect and disconnect; `applied` is `batch`; `resync` is a
`hello` with a hash the authority lacks, answered with a snapshot of the
volatile space. Ephemeral calls are the volatile machine's pending
intents, and relaying them at most every N ms, coalesced to the newest
per transition, is a transport policy in the driver. `live/state.rs`,
`ordering.rs`, `PerDevice`, `Ephemeral` and the live frames are gone.

### 1.10 Views, incremental per operator

A query is an expression in the relational subset. A maintained view
keeps, per operator, what it needs to update itself from a change delta:

| operator | keeps | on a delta |
|---|---|---|
| `Scan` | nothing | the table's changes, as is |
| `Filter`, `Map` | nothing | apply the lambda to each change |
| `Join` | an index on each side's join key | for a change on one side, probe the other |
| `Group` | the members per key | recompute the affected group's aggregate |
| `Sort`, `Take` | an ordered index; the window | reposition; refill the window from the index |
| `Lookup` | the key it read | re-read when the looked-up row changes |

Composition handles the tree. After each settle the view consumes the
change lists from §1.7 and emits `Insert{at}` / `Remove{at}` /
`Update{at}` against the list the UI holds. A rebase is changes like any
other. The correctness contract — the answer equals a fresh evaluation,
and the patches splice the old list into the new — is a test over every
`views/` vector, and the work contract — a change costs the entries it
hits — is measured (Part 3).

### 1.11 The protocol

Frames are one `Sum` type, encoded by §1.1, derived:

```
Client: Hello { space, cursor, hash: Option<EntryHash>, device? }
        Push  { space, entry, body: Option<bytes> }      -- the body when the authority asked
        Fetch { hashes }
Server: Ack   { id, seq, facts: Option<[Change]> }       -- facts when they differ from the preview
        Reject{ id, why } | Held { id, fn }              -- Held: send the body
        Batch { space, from, logged: [Logged] }
        Snapshot { space, seq, hash, state, rows }
        Bodies { [(FnHash, bytes)] }
```

Two sans-io state machines, client and server, each a driver of a
`Peer`. The server evaluates each connection's scopes at `Hello` and
serves it its union (§1.8). Sign-in is as v4 (`ark-auth`): the server is
the OpenID Connect client; a session is what `Hello` carries.

### 1.12 Authoring

Attribute macros over restricted Rust, producing §1.4's `Expr`:

```rust
#[ark::mutator]
fn create(ctx: Ctx, db: Db, input: CreatePlaylist) -> Result<(), Refuse> {
    if db.playlist.filter(|p| p.user_id == ctx.user && p.name == input.name).first().is_some() {
        refuse!("a playlist with this name exists");
    }
    db.playlist.insert(Playlist { id: ctx.new_id(), user_id: ctx.user, name: input.name, added_ms: ctx.now() });
    Ok(())
}
```

The macro accepts `let`, `if`, `match` on declared sums, field access,
the operators, calls to other `#[ark::*]` functions and the standard
library, the relational combinators on `db.<table>`, and the writes; it
refuses anything else with a compile error naming the construct. Field
names are read from the struct, never from declaration order. There is
one execution path, the interpreter. `spec5/AUTHORING.md` is the subset
the macro accepts.

Why a macro rather than v4's builders: the domain must become data
(§1.4) either way, and every v4 authoring hazard the survey found —
fields bound by position, variants named by string, `guard::<I>` untied
from its transition — comes from a builder reconstructing by hand what
the Rust source already says. A macro reads the source. The cost is
that it is a parser; §1.14 says how that is kept small.

### 1.13 What leaves, and why nothing is lost

| v4 | v5 | the feature it carried |
|---|---|---|
| `Value` enum, `Null`, `compare_value`, CBOR | bytes under types (§1.1) | canonical encoding, total order |
| fourteen `Ty` variants, `conforms`, `TypeError` arms | five constructors + refinements | typed inputs, form validation |
| `Stmt`/`Plan`/`Pred`/`Check`/transitions | one `Expr` with effects | mutators, queries, scopes, checks, live |
| `verify.rs` (296 complaints), `eval.rs`, `view::pull`, `scope.rs` | one checker, one interpreter | the same |
| encode/decode/normalize/hash walks | `#[derive(Node)]` | the wire form, the hash |
| monolithic module, `ran`, `retire`, `check_retained` | bindings over a CAS | versioning by hash |
| `log_id`, `verify`/`agree`, gap check, `adopt` | the chain | the log's identity, divergence |
| `Auto::NewId`, frozen ids | deterministic ids | exact replay |
| inbox, `Known`/`Ran`, `need_closures` on receive | facts-only confirmed | old clients never "update required" |
| `Replica` + `Authority` + client `Peer` + `Hub` + sim state | one machine, drivers | rebase, authority as a role, alone |
| `partial`/`through`, `held` frames, `scope` folding | a view per connection | partial sync |
| `live/state.rs`, `ordering.rs`, live frames, `PerDevice` | a second machine over volatile tables | live state |
| per-plan-shape maintenance | per-operator deltas | maintained views with patches |
| builders, native path, agreement tests | attribute macros, one path | authoring |
| `journal.rs` pages/merge, `persist.rs` rehome | the chain on disk | durability, alone |

### 1.14 Risks, stated as risks

- **`Sort`/`Take` under a limit and `Group` with ordered members** are
  where v4's hand-casing earned its lines. V1 prototypes them first and
  measures against v4's `views` paths before the rest of the engine is
  built on them.
- **The macro is a parser.** Kept small and loud; the accepted subset is
  a list, and anything off it is a compile error, never a guess.
- **Interpreting everything.** No native path for mutators. Part 3's
  `perf_a` and harken's `perf_add_song` are the numbers; if a hot path is
  more than 3× v4's, the plan is to make the interpreter faster (typed
  bytes make a bytecode cheap), not to bring the native path back.
- **Bandwidth of facts-only.** Measured by `perf_d` and `perf_e` bytes
  per entry. harken's bulk mutator is `add_song`; its facts are its rows.
- **The browser.** The iced web build runs the Rust store in wasm today;
  nothing here needs anything wasm lacks. Not verified until D1.

---

## Part 2 — The work

V5 is built **beside** v4, as new crates, so that v4 keeps compiling,
its vectors keep passing, and Part 3 can run both. Nothing under
`rust/ark`, `rust/ark-client`, `rust/ark-server` or `spec/vectors` is
edited until R1. The crates:

```
rust/ark5/          the library: §1.1–§1.11, one crate, no dependencies but sha2
rust/ark5-macros/   §1.12, a proc-macro crate; also the Node derive (L1)
rust/ark5-dev/      the simulator, the trace player, the fuzzer, the vector writer (dev only)
rust/ark5-server/   the hub, persistence, the web build, auth glue (thin; drivers)
rust/ark5-client/   the client driver: link, storage, views for a UI (thin)
harken/domain5/     harken's domain on §1.12
spec5/              AUTHORING.md, README.md, RUNNERS.md and vectors/ for v5
```

All code is written by implementation agents; the coordinator writes this
document, reviews, runs the checks, commits others' work where noted, and
pushes. Each piece names what it owns; nobody edits outside their paths.
The order is forced by APIs:

```
K1 ──► L1 ──► V1 ──► D1 ──► M1 ──► R1
         └──► A1 ──┘
```

### Rules every agent keeps

- **Git**: commit your own paths only — `git add -- <paths>` then
  `git commit -m "…" -- <paths>` — and never push, reset, checkout,
  stash, rebase or clean; other agents' uncommitted work shares the tree.
  Retry on `index.lock`. Every commit message ends with exactly:

      Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
      Claude-Session: https://claude.ai/code/session_01AE6tmNmaVTjG7EGfeLfKBG

  No model names anywhere else; no personal names or emails anywhere.
- **Build**: `cargo` from `/home/user/apps/rust` works directly;
  `rust/target` is shared. Add new crates to `rust/Cargo.toml`'s members.
  The disk is a fixed allowance: when it fills, delete stale test binaries
  under `rust/target/debug/deps` (keep the newest per name) and say so.
- **Size is a deliverable.** `tools/loc.sh` is the count. Every piece
  reports its count when it lands, and a piece that comes in over its
  budget says why before it lands.
- **Falsify every new test once** by breaking what it checks, and say in
  the test's doc comment what broke it.
- **Say what is not verified.**
- **Doc comments are the spec**: each module's comment says what it
  defines and which §1 section it is, as v4's do; no prose elsewhere.
- When this document does not answer a question, ask the coordinator with
  a concrete proposal; the answer is added here.

### K1 — the kernel: types, codec, store, chain, machine

Owns: `rust/ark5/src/{lib.rs, ty.rs, codec.rs, hash.rs, schema.rs,
store.rs, log.rs, peer.rs, frames.rs}`, `rust/ark5/tests/**`,
`rust/ark5/Cargo.toml`, the member line in `rust/Cargo.toml`.

Builds §1.1, §1.2, §1.3, §1.6, §1.7 (with `Feed::Whole` only) and §1.11
(the frames, derived by hand for now — `#[derive(Node)]` is L1's; K1
writes the `Ty` of each frame type once, in a table, and the derive
replaces it). Functions are opaque to K1: `trait Procedure { fn run(&self,
store: &dyn Read, ctx, input) -> Result<Vec<Change>, Refuse> }` and a
`Cas` that maps `FnHash` to one; L1 supplies the interpreter as a
`Procedure`.

Held to:

- **codec**: a property test that for every `Ty` the encoding is
  canonical, prefix-free and order-preserving over generated values, and
  that `parse` refuses every non-canonical byte string it generates
  (unescaped `00`, a missing terminator, a variant index out of range).
- **hash**: the v4 `hash/` vectors re-derived — the same demo rows, under
  the same schema, now as tuples; the vector writer in K1's tests prints
  them in `json.rs`'s dialect into `spec5/vectors/hash/`.
- **rebase**: `three-peers` steps 1–4 and `fleet-seed-7`, re-derived,
  with the demo's four mutators written by hand as `Procedure`s in
  `rust/ark5/tests/demo.rs` (the interpreter replaces them in L1 and the
  vectors must not move). `fleet-seed-7` needs the simulator's scheduler:
  K1 ports `sim.rs`'s network (MMIX, duplicate one in eight, drop one in
  sixteen) as a test support module under `rust/ark5/tests/support/`,
  which D1 moves to `ark5-dev`.
- Going alone, joining with a chain the server adopts, and a chain the
  server refuses because its facts differ.

Budget: **≤ 3,000** non-blank lines of `src/`. Not in K1: scopes, views,
the language, live, persistence beyond `Output::Persist`.

### L1 — the language: types with refinements, `Expr`, checker, interpreter, derive

Owns: `rust/ark5/src/{ir.rs, check.rs, interp.rs, std.rs, module.rs}`,
`rust/ark5-macros/src/derive.rs` (the `Node` derive only), `spec5/README.md`
§1–§6, and the `Ty` table in `frames.rs` replaced by the derive.

Builds §1.4, §1.5, the refinement half of §1.1 (`parse` with messages),
and `#[derive(Node)]`. The interpreter is a `Procedure` (K1's trait).
Standard library: v4's `stdlib.rs` names, with signatures stating the
refinements they preserve; the Unicode tables come across unchanged
(generated data, not counted against the budget).

Held to: `eval/` and `verify/` vectors re-derived over the v5 demo —
the demo's mutators now written in the IR by a small builder in
`tests/`, and the K1 hand-written `Procedure`s deleted once the
interpreter reproduces every `rebase` vector byte for byte (the vectors
must not move). The form validator (`eval/form-check`, `eval/checks`) is
`parse`. `DeBruijn` normalisation is pinned by a `module/` vector: two
spellings, one hash.

Budget: **≤ 2,200** (`ir` 300, `check` 600, `interp` 600, `std` 300,
`module` 200, derive 200).

### V1 — views

Owns: `rust/ark5/src/view.rs`, `spec5/vectors/views/`, and in `peer.rs`
the `Feed::Scoped` arm (§1.8), coordinated with D1.

Builds §1.10 and §1.8. The operators are generic over the delta; the
contract test runs every `views/` vector re-derived and asserts the
answer equals a fresh `interp` evaluation after every settle, and that
the patches splice. The work contract is a counting store (v4
`tests/support/counting.rs` ported): a change costs reads proportional to
the entries it hits, asserted on `playlist-with-items`, `top-two-by-pos`
and `group-source`.

Prototype first, before anything else in V1: `Sort`+`Take` with refill,
and `Group` with a `Sort`ed aggregate, each against v4's `view.rs` on the
same vector, timed. Report the ratio to the coordinator before building
the rest.

Budget: **≤ 1,300**.

### A1 — authoring macros, harken's domain

Owns: `rust/ark5-macros/src/{lib.rs, mutator.rs, query.rs, scope.rs,
live.rs, lower.rs}`, `harken/domain5/**`, `spec5/AUTHORING.md`.

Builds §1.12. The lowering from `syn` to `Expr` is one module; each
attribute is a thin entry. The accepted subset is a table in
`AUTHORING.md`, and every row has a test that it lowers and a `trybuild`
test that the nearest thing outside it is refused with a message naming
it. harken's domain is ported function for function; `harken/domain/tests/
{converge,read_model,views,listening}.rs` are ported as its tests, so the
domain is held to what it was.

Budget: macros **≤ 1,500**; `harken/domain5` reported, not budgeted
(it is harken's, and its line count beside v4's 3,711 is the
ergonomics result).

### D1 — drivers: client, server, dev crate, the volatile space

Owns: `rust/ark5-client/**`, `rust/ark5-server/**`, `rust/ark5-dev/**`,
`spec5/RUNNERS.md`, `spec5/vectors/{protocol,trace,rebase}/` (the fleet
and fuzz vectors move here from K1's tests), and in `peer.rs` the
`space` arm (§1.9).

Builds §1.9, §1.11's two machines, the journal (the chain on disk: append
`Logged` bytes; on open, truncate at the first entry whose hash does not
check; the snapshot file is rows in key order), the client's link with
reconnect and the ephemeral relay policy, the hub over tokio with the
in-process transport the perf tests use, persistence, the web build's
needs, auth glue (`ark-auth` is reused as is), the simulator and trace
player and fuzzer in `ark5-dev`. The fuzzer's convergence, replay and
every-frame's-bytes checks are as v4's.

Held to: the `trace` corpus re-derived (a client as a pure function of
its inputs), `fleet-seed-7` through the real hub, the fuzzer finding
nothing in 10,000 sessions, and the browser build drawn once in headless
Chromium as v4's is.

Budget: client **≤ 1,200**, server **≤ 1,200**, dev **≤ 1,500**.

### M1 — measurement

Owns: `rust/ark5/tests/perf.rs`, `rust/ark5-server/tests/perf.rs`,
`harken/domain5/tests/perf.rs`, `docs/perf/**`, and Part 3's table in
this document.

Builds the harnesses Part 3 names, runs both versions, writes the
numbers. Nothing else.

### R1 — retirement

After M1's numbers are in and the coordinator has decided: `rust/ark5*`
becomes `rust/ark*`, `spec5` becomes `spec`, v4's crates and vectors are
deleted, the flake and the READMEs are updated, `kotlin/FROZEN.md` and
`swift/FROZEN.md` say what a port now carries.

**Landed** (`8bda5c3`…): see R1 under Landed.

---

## Part 3 — Measurement

Every number was recorded by the command that produced it, in release,
on the same machine within the same hour, v4 and v5 alternating. The raw
outputs were kept under `docs/perf/` while the work was measured and
removed from the tree once the tables below were filled; they are in
this branch's history up to the commit that removed them.

### Size

`tools/loc.sh`: non-blank, non-comment Rust lines, split into shipped
source, binaries and tests, per crate. The comparison is **shipped source
of the library** (`ark` vs `ark5` + `ark5-macros`), **shipped source
all-in** (plus client, server, auth, and harken's domain), and tests,
separately. Generated Unicode tables are excluded from both.

### Speed

The same shapes, the same sizes (four and sixteen times apart), the same
columns (mean, first hundred, last hundred), reported per operation:

| shape | v4 harness | what it measures |
|---|---|---|
| a | `ark/tests/perf.rs::perf_a_mutate_alone` | a local mutation, alone, pending depth growing |
| d | `perf_d_initial_sync` | a client catching up from empty; bytes and time per entry |
| e | `perf_e_rebase` | a confirmed entry under N pending; per entry and per page |
| g | `perf_g_authority_and_fanout` | the authority sequencing and fanning out to C connections |
| h | `perf_h_frames` | encode/decode of each frame; allocations |
| i | `perf_i_partial_fanout` | the authority serving scoped connections |
| b, c | `ark-server/tests/perf.rs` | a round trip through the hub; the pump alone; offline pending; storage |
| harken | `harken/domain/tests/perf.rs` | `add_song`, `playlists`, `playlist_name`, rows examined, allocations |
| views | new in both | a change against `playlist-with-items`, `top-two-by-pos`, `group-source` at 1k/4k/16k rows: time, entries touched |

The v5 harnesses are written to print the same lines so the two outputs
diff. Where v5 has no counterpart to a v4 shape (the inbox, the
`verify` round trip), the line says `n/a` and why.

### The report

Numbers are from the same-hour runs described above. Notes under the
table say what each row counts.

**Size** (`loc-v5.txt`, after P1 `loc-p1.txt`; non-blank, non-comment; Unicode tables excluded)

| | v4 | v5 | v5 / v4 |
|---|---:|---:|---:|
| library source | 25,537 | 6,526 | 0.26 |
| client + server source | 6,843 | 1,465 | 0.21 |
| harken's domain | 3,711 | 2,583 | 0.70 |
| all-in source (library, client, server, auth, domain) | 37,501 | 11,984 | 0.32 |
| dev tooling (sim, trace, fuzz, vectors) | in `ark` | 1,441 | — |
| tests | 18,916 | 11,257 | 0.60 |
| concepts a port carries | 5 sub-languages, 2 execution paths, the lifecycle in 5 places | 1 language, 1 path, 1 machine | — |

**Speed** (release, `--test-threads=1`, v4 and v5 alternating)

| shape | v4 | v5 | v5 / v4 | v5 after P1 | after / v4 |
|---|---:|---:|---:|---:|---:|
| a: local mutation, linked (µs, 8,000 entries, playlists of 10) | 28.2 | 37.6 | 1.33 | 39.0 | 1.42 |
| a: local mutation, alone (µs) | 28.2 | 16.2 | 0.57 | 17.2 | 0.62 |
| b: linked round trip (µs, 2,000, playlists of 10, no log file) | 192.5 | 5,520.1 | 29 | 182.2 | 0.85 |
| c: offline mutate at 8,000 pending (µs, Memory) | 30.4 | 1,990.8 | 65 | 28.0 | 0.89 |
| d: initial sync, time (µs / entry, 8,010 entries) | 26.4 | 9.6 | 0.36 | 9.7 | 0.39 |
| d: initial sync, size (bytes / entry) | 161.7 | 250.0 | 1.55 | 250.0 | 1.55 |
| e: rebase, one page under 400 pending (µs / entry) | 62.0 | 3,974.9 | 64 | 48.8 | 0.76 |
| e: rebase, a pump per entry under 400 pending (µs / entry) | 8,498.6 | 3,779.5 | 0.44 | 4,104.8 | 0.45 |
| g: fan-out (µs / connection, 160 connections, log of 8,000) | 4.62 | 0.41 | 0.09 | 0.13 | 0.02 |
| h: decoding a `Batch` of 256 with facts (allocations) | 12,559 | 2,567 | 0.20 | 2,567 | 0.20 |
| h: decoding a `Batch` of 256 with facts (µs) | 1,091 | 381 | 0.35 | 236 | 0.22 |
| harken `add_song` (µs, library of 8,000) | 326.7 | 31,440 | 96 | 225.7 | 0.67 |
| harken `add_song`, rows read | 1 | 8,000 | — | 1 | 1 |
| harken `add_to_playlist` onto N−1, rows read | 1 | 7,999 | — | 1 | 1 |
| harken `create_playlist`, the 102nd "Favorites", rows read | 102 | 3,000 | — | 102 | 1 |
| views: a change on `playlist-with-items` at 16k (µs) | 11.06 | 16.10 | 1.46 | 10.57 | 0.88 |
| views: a change on `top-two-by-pos` at 16k (µs) | 77 | 5.6 | 0.07 | 4.5 | 0.07 |
| storage: live bytes per media row | 726 | 426 | 0.59 | 426 | 0.59 |

Notes. *Library source* is `ark` (27,880 less 2,343 Unicode) against
`ark5` (7,522 less 2,343) plus `ark5-macros` (1,238). *All-in* adds
client, server, `ark-auth` (1,410, reused unchanged) and harken's domain.
v4's simulator, trace player, fuzzer and vector writer sit inside `ark`
(`sim.rs` 1,272, `trace.rs` 739, `bin/` 9,758); v5's are `ark5-dev`.
*Tests* include M1's harnesses on both sides (475 lines added to v4,
2,421 to v5). *a, linked* in v5 includes encoding and decoding the
frames; *alone* is one machine in the authority's role. *d* in v4 is its
default, by intent; by facts v4 is 5.6 µs and 234 B. *e* at depth 400
is from `v4-views.txt` and `v5-views.txt`; v4's own harness stops at
100. *g* in v5 is through `Hub::frame`; the machine alone is 0.14. v4's
inbox, `Verify` and native-path lines print `n/a` in the v5 outputs,
with why. Not measured: harken's views, scanner and search shapes; the
storage shapes run over the `media` table alone. *v5 after P1* is from
the `*-p1.txt` outputs and *after / v4* divides it by the v4 run of the
same hour (`*-rerun2.txt`: a 27.56, b 215.11, c 31.26, d 24.67, e 64.6
and 9,215.2, g 5.43, h 1,094.7, `add_song` 338.39, views 11.95 and
68.81); *b* and *c* are `ark5-server`'s; rows read are the counting
store's (`perf_rows_read_per_mutation`).

**Measured.** Faster in v5: a peer alone mutates in 0.57× v4's time; the
authority sequences at par (16–17 µs); fan-out costs 0.09× per connection
because the `Batch` is encoded once, not per connection; a `Batch` decodes
in 381 µs against 1,091 with a fifth of the allocations; initial sync is
0.36× v4's default by intent; `top-two-by-pos` at 16k is 0.07×; stores hold 426 live B/row
against 726 and open in 0.2–0.8× the time. Slower: initial sync carries
1.55× the bytes (facts always); a rebase under 400 pending is 64× a v4 page
because `Peer::confirm` undoes and re-runs every pending intent once per
confirmed entry where v4 settles once per page; a client offline pays
2.0 ms per mutate at 8,000 pending (v4 30 µs) because `Client::step`
rewrites the whole `pending` blob on every change; a linked round trip is
5.5 ms (v4 0.2) because the link thread waits up to 5 ms for a frame
before sending — 0.17 ms without it. harken's `add_song` is 96× and grows
with the library: its reads return all 8,000 media rows where v4's
returned one (`add_to_playlist` onto a playlist of N−1: 7,999 rows, v4 1).

**P1.** The four causes, fixed (`*-p1.txt` against `*-rerun2.txt`). Routing:
`first` of a routed filter or sort reads through the store with a limit, a
descending sort over a non-unique index is served with its ties put back in
key order, and bounds past the equalities narrow the read: `add_song` reads
1 row (was 8,000) and costs 225.7 µs against v4's 338.4, `add_to_playlist`
onto N−1 1 row, the 102nd "Favorites" 102 (was 3,000). Rebase: a page undoes
and re-runs the pending once, 48.8 µs/entry under 400 against v4's 64.6 (was
3,974.9). Pending: one journal record per mutate, 28.0 µs at 8,000 pending
(was 1,990.8; v4 31.3). Link: frames go out from the caller's thread, a
linked round trip 182.2 µs (was 5,520.1; v4 215.1). Source +186 lines.

## Landed

**R1** (`8bda5c3`…): v5 has the canonical names and v4 is gone.
`rust/ark5*` → `rust/ark`, `ark-macros`, `ark-client`, `ark-server`,
`ark-dev`; `harken/domain5` → `harken/domain` (crate `harken-domain`);
`spec5` → `spec`; v4's `rust/ark`, `ark-client`, `ark-server`,
`harken/domain` and `spec` deleted. The attribute macros and `Domain` are
re-exported at `ark`'s root, since `use ark::author::{self as ark}` would
now name itself. The join test that timed out in debug is a debug-build
timing: in release, alternating, start-with-server to converged took
11.8/10.6/11.3 s before K2 (`0179f02`) and 9.5/10.0/10.2 s after, so K2
did not slow it (0.88×); a debug build waits ten times `PATIENCE`
(about 99 s needed). One kernel-adjacent fault found by the renamed
suite: the server's process exited while the hub's thread was writing
the volatile space, leaving `log/live.tmp`
(`a_server_stopped_mid_stream_loses_nothing` failed every run);
`Running::stop` now waits for `Handle::stop`. `harken-domain` writes the module file `HARKEN_MODULE`
reads, `(Module, [body])`, as `harken.ark`, committed again; `ark-dev`
gains `arkc` (`verify`, `compat`), `ark-fuzz … traces DIR` writes a
corpus and `tests/vectors.rs::trace_corpus` replays `ARK_TRACE_CORPUS`;
the vectors written over an empty `spec/vectors` are the tree byte for
byte. The flake: `ark` (with `arkc`, `ark-fuzz`), `vectors` (the
writer's output), `fuzz`, `trace-corpus`, `harken-domain`, the harken
packages and `ark-admin`; checks `rust`, `vectors`, `traces`,
`fuzz-smoke`, `versions`, `harken-domain`, `harken-module`; Kotlin's,
Swift's and the phone's outputs removed (`kotlin/FROZEN.md`,
`swift/FROZEN.md`: frozen at v4, last recorded green `a983b3c`); the CI
change is `docs/build-workflow-v5.patch`, to be applied by hand. The
flake's toolchain (1.98) found one lint 1.97 does not
(`result_large_err` in `ark-server::admin`), fixed. Sizes after
(`docs/perf/loc-r1.txt`): `ark` 7,790 source (5,447 less the Unicode
tables), `ark-macros` 1,236, `ark-client` 793, `ark-server` 1,031,
`ark-dev` 1,462 + 86 bin, `harken/domain` 2,583; 36,388 source,
3,492 bin and 21,126 test lines across every crate. Not verified: the
browser build drawn on v5; the CI workflow as patched; the phones (frozen).

**K2** (`3062a3c`…): four rules, each a witness flipped and a kernel test.
**No body crosses the wire**: an authority runs exactly the functions of
the modules its operator loaded (at start, or `/admin/api/load` over the
same schema); `Push` has no body, `Fetch`/`Bodies` are deleted (nothing
fetched them); `Held` means keep it pending and push again; a joined chain
naming an unknown function is refused. **A held intent keeps its
`local`**: those behind it wait on the authority in order and are judged
when it is (pushed again, or `Input::Loaded`). **A grown schema keeps the
log**: `open` reads rows and facts under the new schema (`Table::fit`),
checks the chain by its hashes and never derives a state again; digests
are the new schema's; `Peer::foreign` marks entries not derived (a `Hello`
below is served the snapshot); a replica on another schema takes states on
faith (`Peer::faith`) and asks for the snapshot (cursor -1) on a second
divergence at one entry. **Ids from an origin**: `EntryId::of(origin,
local)`, 16 bytes drawn once and kept; sign-in keeps ids; dedupe per
`(origin, local)`; the authority stamps the connection's actor, session and
roles and binds an origin to its first actor (another actor's sign-in gets
a derived origin). Vectors: protocol/ renumbered (eight frames) and
re-derived; trace/ and rebase/ re-derived (FINAL `bb0abc1a…`, seed-7
`ff04f56d…`); hash/, eval/, verify/, module/, views/ unchanged.

**D1** (`d223d71`…`1b8384e`): `ark5-client` 680, `ark5-server` 708,
`ark5-dev` 1,441 (+22 bin) source lines, each within budget; the
volatile arm in `peer.rs` +79 net, `store.rs` +36 (the still-referenced
refusal, stores by space). 179 tests across the six v5 crates.
`fleet-seed-7` through the real hub and client reaches K1's pin; 123
traces of 41 fleet sessions replay; the fuzzer ran 10,000 sessions
with traces replayed (746,771 ops, 141,787 entries, 0 failed) and a
second 10,000 without; `harken/domain5` is green with no `#[ignore]`
for D1. The fuzzer found and D1 fixed five kernel faults, each now a
test and a `rebase/fleet-fuzz-seed-N` vector: a refused intent used a
`local` while alone; a gap inside a joined chain held its tail; a
`Join` lost in flight; a re-push below a compacted horizon unanswered;
a volatile push overtaken by a later one. Decisions now part of the
design: `space` on `Ack`/`Reject`/`Held`; `Hello.device` is bytes an
app function turns into the `on_join` input with the verified login;
the volatile chain is a real chain and ephemeral calls are sequenced in
it, the relay client-side and which functions are ephemeral client
config; a volatile authority takes a push past a gap and refuses one
behind as overtaken; the hub keeps the volatile snapshot across
restarts and authors `on_restore`; the token is the connection's first
message, close 4401 for nobody; the hub is a sans-io struct on its own
thread so the simulator drives the same one; a store has a space; a
trace is per client at `Peer::step`; the hub's journal is a side blob
of each session's last `local` and refused verdicts. Not verified: the
browser build (out of scope); `wss`; keepalive and backoff timing;
`Fetch`/`Bodies`/`Held` only by hand-built vectors; fuzz modules use
`Int`/`Text` columns and no `Group`/`Join` queries; a scoped connection
is served afresh after a restart; `converge.rs` keeps A1's local
simulator copy.

**A1** (`5053c19`…`62965d7`): the attribute macros (`rust/ark5-macros`:
`lower`, `mutator`, `shape`, `query`, `scope`, `live`, `module!`) at
1,164 source lines, plus a 678-line runtime `ark5::author` the generated
code calls (a proc-macro crate exports nothing but macros, and resolving
fields by name needs types in a normal crate): **1,842 against the
budget of 1,500**, accepted — the runtime is the price of field names
being rustc's to resolve, and nothing in it is a second execution
path. The demo written with `#[ark::mutator]` hashes byte for byte as
`tests/demo.rs` builds it. 22 lowering tests, one per row of
`spec5/AUTHORING.md`'s subset; 33 `trybuild` compile-fail cases.
`harken/domain5` is **2,583 source lines against v4's 3,711** (70%),
every function through the checker, with `read_model`, `listening`,
`views` and `converge` ported (144 tests across the three crates).
Decisions now part of the design: an expression is an `Ex<T>` of its
Rust type with refinements dropped, so a misspelt field or an unmatched
variant is rustc's own error; each function expands to a `mod` with a
cached `built()` and a typed `fn` other bodies call, lowering to
`Call(hash)`; a helper or guard gets the least effect its body has;
`Option<T>` is a named declaration; a one-field tuple struct is a
refinement; `#[ark(key)]` marks key columns; `derive(Schema)` writes
the `Sum` over tables a scope takes; live state is three volatile
tables (`listening` by user, `listening_device` by (user, device),
`listening_scrub` ephemeral), and a live function touching a durable
table is refused at assembly. Not verified: the live transitions run on
the interpreter over a store, not the volatile machine — four listening
tests wait on D1 (`#[ignore]`, each saying so); `slug` cannot collapse a
run of separators (v5 has no fold over neighbouring characters; one
std function would fix it); the views tests churn by the domain's own
mutations rather than v4's raw rows; `converge` uses a local copy of
the simulator with per-client logins; two text indexes on `media` are
not ported; a cycle of helpers panics at module build rather than at
compile time.

**V1** (`32ac411`, `3d09aa2`): `view.rs` (624) plus index routing in
`interp.rs`, the `Feed::Scoped` arm in `peer.rs` and `Std::Sum`: 922
net source lines against 1,300; 49 tests. Prototype ratios, release,
v5/v4 per push: `top-two-by-pos` 0.7 at 1k, 0.2 at 4k, 0.07 at 16k (v4
grows with n, v5 does not); `group-source` under a sort 1.3–1.7, flat.
12 `views/` vectors re-derived; after every batch the view equals a
fresh evaluation and the patches splice; a change costs 3 rows on
`playlist-with-items` and 0 on the other two at every size; the
interpreter reads through indexes (`add_to_playlist` 1 row, 3 gets); a
scoped replica holds exactly its union after every op over three
seeds. Decisions now part of the design: a scope is one `Pure` function
over a `Sum` of the schema's tables; the authority learns a connection's
feed by `Input::Feed { who, feed }` and answers it by `Output::To {
session, bytes }`, `Fanout` meaning the connections fed whole; a union
must hold what its rows reference, else it is answered with nothing;
`Filter` and `Map` keep their output; kept aggregates are `len`, `sum`,
`sum(map)` pure of the member, or tuples of those; routing takes the
longest run of equalities leading the key or an index, a sort only when
the path gives the stable order exactly, a literal `Take` only over a
pure-equality filter; `None` indexes as a value so a unique index over
an `Option` column is non-unique. Not verified: hydration cost; a
refused push leaves a view stale; `Join`/`Group` keys that read the
store rebuild whole; the authority keeps a `Store` per scoped
connection, unmeasured and not persisted across restart; `Output::To`
is D1's to route.

**L1** (`04ef9bb`…`9ec23ef`): the language — `ir`, `check`, `interp`,
`std`, `module` and the `Node` derive (`rust/ark5-macros`): 1,378 net
source lines against a budget of 2,200 (1,607 in its modules; K1's files
shrank by 229 as the derive replaced their hand-written walks). 42 tests.
The demo's mutators are IR built by plain functions in `tests/demo.rs`,
run by the interpreter, and every K1 `rebase`, `alone` and `fleet`
pinned state hash held with the hand-written `Procedure`s deleted.
`eval/`, `verify/` (18 refused cases with the checker's words) and
`module/` (De Bruijn: two spellings, one hash) vectors re-derived under
`spec5/vectors/`. Decisions now part of the design: `Ty::Named`
appended; a function carries the type declarations its refinements name,
so its hash covers their meaning; `Lit`, `Variant` and `List` carry
their types; `CtxField::NewId(table)` has effect `Write` and draws count
from 0; binding is in order with the last bound as `Var(0)`; the
interpreter evaluates over the codec's parse tree (ints, bytes, tuples,
sums, lists — no tags, no null, compared by encoded bytes), which is the
"typed cursor" §1.1 means; `trim` normalises at `parse` and the entry
keeps the raw input; form messages carry a path of field positions; a
refinement's predicate is `Pure` and reads no context; a lambda may
carry any effect, so `Map` with writes is the loop; `Insert` refuses an
existing key, `Update` a missing one, `Delete` of a missing key is a
no-op; a standard function is encoded by index, so new ones append;
`Group` yields `(key, aggregate)` in key order. Open, for later pieces:
`Store::apply` checks references only on writes, so a delete can leave
rows dangling (v4 `StillReferenced`) — **D1 fixes this in `store.rs`**;
`Filter(Scan)` is O(rows) with no index use — **V1 routes constant
equalities and orders to the store's indexes**; live vectors and the
server half are D1's; no `Ctx(device)` yet; checker soundness has no
fuzz; no performance measured.

**K1** (`1e739f4`…`72b93ac`): `rust/ark5` — 2,030 source lines against
a budget of 3,000; 28 tests, each falsified once. Held to the `hash/`
vectors re-derived (`spec5/vectors/hash/`), `three-peers` steps 1–4,
`fleet-seed-7` through the ported scheduler plus 40 other seeds, and
alone/join adopt-or-refuse. Decisions it took, now part of the design:
`Input::Frame { from: Option<Who>, bytes }` (an authority judges by the
connection's verified identity); `Output::Fanout` beside `Send`;
lifecycle inputs `Connected`, `Alone`, `Join`, `Compact(seq)`; a `Join`
frame carrying a chain; confirmation through `Batch` only, `Ack` carries
no facts; `Entry.local` and `id == EntryId::of(session, local)`;
`SchemaHash`; a column carries its declared type's name; `scan` takes
`{index, lo, hi, dir, limit}` with "after" an exclusive bound; a
snapshot is served at the head. Not verified: `Fetch`/`Bodies`
untested; `Tick`/`Persisted` are no-ops; pending intents are not
persisted (`Open` takes them); the volatile space is excluded from hash
and snapshot but has no second machine yet; no performance measured.
`Ty` is recursive and hand-encoded; L1's derive must handle recursion
through `Named`.
