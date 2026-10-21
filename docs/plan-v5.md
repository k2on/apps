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

> *Superseded (history).* §1.16 replaced this codec with serde: a value
> is its bytes in the value format (`ark::format`, postcard or JSON by a
> feature), a key the same value memcomparable, and there is no `Ty`, no
> `parse` and no hand-written encoding; refinements are an input's
> `#[derive(Input)]` checks. The order-preserving key and the distinct
> hash spaces survive.


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

> *Superseded (history).* §1.16: the domain is plain Rust, so there is no
> `Expr`, checker or interpreter; effects are the borrow checker's (`&Db`
> reads, `&mut Db` writes) and a view is a typed `Plan`.


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

> *Superseded (history).* §1.16: an entry names its function by name, a
> `Domain` maps names to Rust functions, and the CAS, `FnHash` and the
> module are gone; the schema stays data, hashed and prefix-checked.


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

Sans-io: no sockets, clocks or files inside. One driver feeds `step` and
routes `Output`: `ark-peer`'s `Peer`, the two machines (the log's and the
volatile space's) over one disk, in either role — a device that **dials**
(the link with redial, its pending intents on disk, the relay, the views
and the functions an app calls by name) or a server that **listens** (the
connections with their identity and feed, `Send`/`Fanout`/`To` routed,
written before delivered, the journal beside the log). The simulator and
the trace player drive the same `Peer`. Rebase, sequencing, dedupe, judging, compaction, going
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

One sans-io driver, `ark-peer`'s `Peer` (§1.7): a server is a `Peer` in
`Role::Authority` that listens, a device one that dials. The server
evaluates each connection's scopes at `Hello` and serves it its union
(§1.8). Sign-in is as v4 (`ark-auth`): the server is
the OpenID Connect client; a session is what `Hello` carries.

### 1.12 Authoring

> *Superseded (history).* §1.16: the attributes only register plain Rust
> functions by name; nothing is lowered. `spec/AUTHORING.md` is "Writing a
> domain".


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

### 1.15 Authoring: the domain is written in ArkLang

> *Superseded (history).* §1.16: ArkLang, `arkc`, `ark::gen`'s module
> bindings and the binary module are gone; a domain is a crate.


**The module** (§1.5) is what runtimes load, check, hash and call by name.
**ArkLang** (`ark::sexp`, `spec/AUTHORING.md`) is the language a domain is
written in: S-expressions, one reader rule, one spelling. The decisions:

1. **A domain is a directory of `.ark` files** (`ark::sexp::build_dir`).
   Every `**/*.ark` under it is read in sorted path order, all forms first,
   then names resolved, so order and layout carry no meaning; files make no
   namespaces, dotted names do; `(schema …)` forms merge; a name declared
   twice is refused, both files and lines said.
2. **The text is the source of truth; the binary module is derived.** No
   module file is checked in. The server builds its module from
   `HARKEN_MODULE` — a directory, an ArkLang file, or a module file for the
   old modules it holds (`ark::sexp::load`) — and serves it by hash as
   before; the flake's `harken-domain` is the directory, built and checked.
3. **`arkc`** is a small CLI over library calls: `check MODULE…`, `print
   MODULE [-o DIR]` (a file per dotted prefix, `schema.ark`, `types.ark`),
   `compat OLD NEW`, `build MODULE -o FILE`.
4. **Bindings are generated from the module, types only** (`ark::gen`): a
   struct per row and record, an enum per sum, an alias per refinement, a
   stub per function calling it by name through `Call`. An app's `build.rs`
   writes them to `OUT_DIR` from the domain's directory. At load a module is
   held to them (`fits`): its schema a prefix-extension of theirs, every
   type and function they name of their structure, a row grown by appended
   columns read by its prefix. No function body is ever generated.
5. **No builders**: `ark::author` and the `Row`/`Input`/`Sum`/`Schema`
   derives are gone (`Node` stays). harken's domain crate is its directory,
   its tests, and `host.rs`, the host's Rust each app includes beside its
   bindings (the session read model; the keys, by the domain's helpers).
6. **The spec is readable**: `eval/`, `verify/`, `views/` vectors carry
   their module as ArkLang beside the bytes, the text informative.

**The language.** Top level: `(schema (table …)…)`, `(type N TY)`,
`(mutator|live N ((P TY)…) BODY…)`, `(query|helper|guard N ((P TY)…) RET
BODY…)`, `(scope N ((ROW TY)) BODY)`. Expressions: `let`, `if`, `match`,
`record`, `tuple`, `list`, `some`/`none`, `as`, `(?? o.f d)` and `(?. o f)`
(the matches through an option they stand for), the operators, `refuse`,
`fn`, reads and writes by table, `ctx.*`, `new-id`, `has-role`, the
standard functions, `(NAME ARG…)`. A place types what it wants: a `none`,
a `(record …)`, a `map`'s element; `(as T e)` where none does.

**Names are in the module**: a declaration keeps its fields and variants
(A3), a function its binders' names (`Function::names`, in walk order), so
the printer writes the author's. A name is in a function's hash: renaming a
local is a new function, as renaming a field is; the body stays De Bruijn,
and `port.rs` holds every harken function to its pre-A3 pin, names aside.
**The layout**: one line when a form ends by column 100, its closing `)`s
counted; else head and first argument (an atom, or one that fits) on the
first line, the rest one per line two spaces in. `print(build(t)) == t`
for every file of `harken/domain`, comments aside.

### 1.16 Native: one runtime, the domain in Rust

The owner's decision, which supersedes §1.4, §1.5, §1.12 and §1.15: **there
is one runtime, Rust, and a domain is plain Rust.** Phones embed the Rust
kernel through FFI (N3); ArkLang, the checker and the interpreter go.

1. **A domain is a crate.** Rows are structs with `#[derive(Row)]`
   (`#[ark(key)]`, `#[ark(index(..))]`, `#[ark(unique(..))]`,
   `#[ark(refs(T))]`, `#[ark(volatile)]`); inputs `#[derive(Input)]`, whose
   field attributes (`#[ark(trim, len = "1..=80", why = "…")]`, `range`,
   `non_empty`, `check`) generate the check that is the form's validation;
   sums are ordinary enums (`#[derive(Describe)]`); `#[derive(Schema)]` on
   the struct of tables. A **mutator** is `fn(&Ctx, &mut Db<S>, I) ->
   Result<(), Refusal>`; a helper or guard takes `&Db<S>`, so it cannot
   write — effects are the borrow checker's; a **scope** is `fn(&Ctx, &R)
   -> bool` per table; a **live transition** is a mutator over volatile
   tables; a **view** returns a `Plan<R>` built by `plan::scan::<R>()
   .filter(..).sort(..).take(n)`, `group`, `join`, because maintenance must
   see the plan. `#[mutator("playlists.add_to_playlist")]` (and `live`,
   `query`, `view`, `scope`) only registers the function by name in a
   `Domain`: name → function and its input's type description.
2. **Reads are typed handles.** `db.playlist.get(id)`, `db.playlist_item
   .by(PlaylistItem::PLAYLIST_POS).from(id).last()`, `scan`, `range`: the
   author names the index, a wrong column is a compile error, and nothing
   routes a mutator's reads at run time. `insert`/`update`/`put`/`delete`
   judge the store's constraints and report `Change`s, typed, stored as
   bytes. A run reads its own writes; a refusal undoes them.
3. **Determinism is an API rule.** `ctx.now` and `ctx.new_id()` (the n-th
   draw of the entry's id, §1.6) are a function's only non-determinism;
   nothing else a mutator may consult varies between runs. A replica that
   disagrees anyway is healed as now: the authority's facts win.
4. **serde everywhere.** Rows, inputs, entries, `Logged`, frames, snapshots
   and journal records are serde values written in one **value format**
   (`ark::format`): **postcard** by default since N4 (a struct its fields
   in declaration order, integers varints, an enum its variant's index),
   or compact **JSON** with the `values-json` feature — no floats, maps as
   `BTreeMap`, one encoding per value, the canonical form hashed. JSON is
   the human form under both: what the explorer, the phones' bridge and
   the vectors read, a value of a `Type` read back from either
   (`ark::json`). Keys — a row's, an index entry's — are `memcomparable`
   bytes, so the store stays one `memcmp`-ordered map. A grown row is read
   under this schema by `Table::fit`: completed with its appended columns'
   defaults, or cut to its prefix (§1.2's rule). The state hash is
   `sha256(table ‖ row bytes)` summed per table; the chain hash is over the
   entry's bytes. `codec.rs`, `ty.rs`, `Node` go.
5. **Functions by name.** `Entry.func` is the function's name; `Hello`
   carries the domain's schema hash and version string; an unknown name is
   `Held`. The CAS, closures by hash and the module go. The schema stays
   data, derived from the structs, hashed, prefix-compat-checked, and what
   the explorer reads; forms come from each input's type description.
6. **Views are generic.** `view.rs` is `impl<R>` operators over typed rows
   and deltas (derived `Ord`/`Hash`, order keys the sort keys' key bytes),
   monomorphised per declared view; a closure's reads through `&Db` are
   recorded and a change to what it read re-runs that element only. One
   `DynRow` (a JSON row under its table) puts the explorer and the
   `views/` vectors — plans carried as JSON — on the same engine.

Kept: the store, the chain, the one machine, the rebase, partial sync as a
view, live state as a second machine, every `rebase/`, `protocol/`,
`trace/`, `views/` and `hash/` family (re-derived). Gone: `eval/`,
`verify/`, `module/`, `text/`, `codec/`, `arkc`, the source pane.

---

## Part 2 — The work

V5 is built **beside** v4, as new crates, so that v4 keeps compiling,
its vectors keep passing, and Part 3 can run both. Nothing under
`rust/ark`, `rust/ark-client`, `rust/ark-server` or `spec/vectors` is
edited until R1. The crates:

The crates as they are now (the list as first planned, `ark5*` beside v4, is
in the history):

```
rust/ark/           the kernel: §1.1–§1.11 and §1.16 — format, json, hash, schema,
                    store, log, peer, frames, row, db, domain, plan, dynamic
rust/ark-macros/    the derives (Row, Input, Describe, Schema) and the attributes
                    that register a function by name
rust/ark-peer/      the one peer that dials and listens: storage, link, hub, the
                    app kit, the server's socket, admin listener and web build;
                    local instances and the peer's own schema (`local`)
rust/ark-ffi/       ark-peer's app kit through UniFFI, for the phones
rust/ark-dev/       the demo, simulator, trace player, fuzzer, vector writer, and
                    `gen kotlin|swift` (a domain's types for the phones)
rust/ark-auth/      dev sign-in and OpenID Connect
rust/ark-explorer/  the admin page and the explorer over a replica
rust/arkui/         the iced components
harken/domain/      harken's domain, a crate of plain Rust
harken/server/, harken/iced/, harken/ffi/ (with android/ and ios/)
spec/               README.md, RUNNERS.md, AUTHORING.md and vectors/
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

**Size** (`tools/loc.sh`: non-blank, non-comment Rust lines; Kotlin and Swift
counted the same way; Unicode tables excluded)

| | v4 | v5 | native (N4) | N4 / v4 |
|---|---:|---:|---:|---:|
| library source (`ark` + `ark-macros` + `ark-ffi`) | 25,537 | 6,526 | 6,125 | 0.24 |
| peer (`ark-peer`; v4 and v5 client + server) | 6,843 | 1,465 | 2,041 | 0.30 |
| tooling (`ark-dev`, `ark-explorer`, `arkui`, `ark-auth`) | 19,498† | 7,515† | 7,907 | 0.41 |
| of it `ark-auth` (S1: 1,051, carrying the sign-in tables and the hub's side) | 1,422 | 1,410 | 999 | 0.70 |
| harken: domain | 3,711 | 2,583 | 2,373 | 0.64 |
| harken: server (+ bins) | 4,923 (+1,720) | 4,808† (+1,618) | 4,735 (+1,360) | 0.96 |
| harken: iced (+ bins) | 8,838 (+1,659) | 8,171† (+1,660) | 8,161 (+1,660) | 0.92 |
| harken: android, ios (the apps' own) | 10,998, 821 | 10,998, 821 | 14,992, 779 | 1.36 |
| phones: the runtime under them | 40,224 (`kotlin/` 24,674, `swift/` 15,550) | 40,224 | 2,394 | 0.06 |
| tests (every crate) | 18,916 | 11,257 | 20,169 | 1.07 |

**Speed** (release, `--test-threads=1`; v4 and v5 alternating; N4 within
one hour, postcard alternating with JSON: `rust/ark`'s shapes the median of
three runs each, `ark-peer`'s and harken's one run each)

| shape | v4 | v5 | v5 / v4 | v5 after P1 | after / v4 | native (N4) | N4 / v4 |
|---|---:|---:|---:|---:|---:|---:|---:|
| a: local mutation, linked (µs, 8,000 entries, playlists of 10) | 28.2 | 37.6 | 1.33 | 39.0 | 1.42 | 29.8 | 1.06 |
| a: local mutation, alone (µs) | 28.2 | 16.2 | 0.57 | 17.2 | 0.62 | 10.3 | 0.37 |
| b: linked round trip (µs, 2,000, playlists of 10, no log file) | 192.5 | 5,520.1 | 29 | 182.2 | 0.85 | 140.9 | 0.73 |
| c: offline mutate at 8,000 pending (µs, Memory) | 30.4 | 1,990.8 | 65 | 28.0 | 0.89 | 18.4 | 0.60 |
| d: initial sync, time (µs / entry, 8,010 entries) | 26.4 | 9.6 | 0.36 | 9.7 | 0.39 | 7.5 | 0.28 |
| d: initial sync, size (bytes / entry) | 161.7 | 250.0 | 1.55 | 250.0 | 1.55 | 200.6 | 1.24 |
| e: rebase, one page under 400 pending (µs / entry) | 62.0 | 3,974.9 | 64 | 48.8 | 0.76 | 29.3 | 0.47 |
| e: rebase, a pump per entry under 400 pending (µs / entry) | 8,498.6 | 3,779.5 | 0.44 | 4,104.8 | 0.45 | 2,898.3 | 0.34 |
| g: fan-out (µs / connection, 160 connections, log of 8,000) | 4.62 | 0.41 | 0.09 | 0.13 | 0.02 | 0.10 | 0.02 |
| h: decoding a `Batch` of 256 with facts (allocations) | 12,559 | 2,567 | 0.20 | 2,567 | 0.20 | 2,817 | 0.22 |
| h: decoding a `Batch` of 256 with facts (µs) | 1,091 | 381 | 0.35 | 236 | 0.22 | 141.1 | 0.13 |
| harken `add_song` (µs, library of 8,000) | 326.7 | 31,440 | 96 | 225.7 | 0.67 | 80.5 | 0.25 |
| harken `add_song`, rows read | 1 | 8,000 | — | 1 | 1 | 1 | 1 |
| harken `add_to_playlist` onto N−1, rows read | 1 | 7,999 | — | 1 | 1 | 1 | 1 |
| harken `create_playlist`, the 102nd "Favorites", rows read | 102 | 3,000 | — | 102 | 1 | 102 | 1 |
| views: a change on `playlist-with-items` at 16k (µs) | 11.06 | 16.10 | 1.46 | 10.57 | 0.88 | 10.58 | 0.96 |
| views: a change on `top-two-by-pos` at 16k (µs) | 77 | 5.6 | 0.07 | 4.5 | 0.07 | 5.66 | 0.07 |
| storage: live bytes per media row | 726 | 426 | 0.59 | 426 | 0.59 | 485 | 0.67 |

**The value format, measured** (N4, the same runs; JSON is `--features
ark/values-json`)

| shape | JSON | postcard | postcard / JSON |
|---|---:|---:|---:|
| d: bytes per entry on the wire | 557.3 | 200.6 | 0.36 |
| d: receive by facts (µs / entry, 8,010) | 16.3 | 7.5 | 0.46 |
| h: a `Batch` of 256: bytes; encode µs; decode µs; decode allocations | 140,784; 182; 784; 5,895 | 50,314; 18; 141; 2,817 | 0.36; 0.10; 0.18; 0.48 |
| a: linked (µs, playlists of 10); alone (µs, one playlist) | 46.5; 20.7 | 29.8; 10.3 | 0.64; 0.50 |
| b: round trip, no log file / log on disk (µs, 2,000) | 161.3 / 566.3 | 140.9 / 461.4 | 0.87 / 0.81 |
| c: offline at 8,000 pending, Memory / Dir (µs) | 34.9 / 359.9 | 18.4 / 346.5 | 0.53 / 0.96 |
| e: one page / a pump per entry under 400 (µs / entry) | 32.3 / 3,754.0 | 29.3 / 2,898.3 | 0.91 / 0.77 |
| g: sequence (µs); fan-out per connection, 160 (µs) | 21.5; 0.15 | 12.9; 0.10 | 0.60; 0.67 |
| i: scoped fan-out per connection, 160, log of 8,000 (µs) | 13.9 | 11.5 | 0.83 |
| open: client of 100,000; server of 400,000; alone chain of 400,000 (ms) | 1,057; 5,368; 10,345 | 898; 3,602; 5,841 | 0.85; 0.67; 0.56 |
| on disk: client of 100,000; alone chain of 400,000 (MB) | 20.0; 255.9 | 8.3; 100.8 | 0.42; 0.39 |
| storage: rows alone; with indexes (live B / media row, 100,000) | 280; 602 | 160; 485 | 0.57; 0.81 |
| views at 16k: `playlist-with-items`, `top-two-by-pos`, `group-source` (µs) | 12.2, 6.3, 7.8 | 10.6, 5.7, 6.2 | 0.87, 0.90, 0.79 |
| harken `add_song` (µs, 8,000), whole; alone | 144.5; 63.2 | 80.5; 36.9 | 0.56; 0.58 |

N4 notes. † from the tree at `8ee5380` (K3b, the last v5 interpreted), the
v5 column's other rows are the M1 table's. *Library* N4 is `ark` 5,154,
`ark-macros` 502, `ark-ffi` 469 (v4's `ark` held its simulator, trace
player and binaries, 11,769, counted under tooling). *Phones* N4 is
`ark-ffi`'s source and its `Typed.kt`/`Typed.swift` (153), `harken/ffi`
(123) and the types `ark-dev gen` writes (688 Kotlin, 958 Swift), against
the v4 runtimes the phones ran on; the apps' own lines grew (android
10,998 → 14,992) as onboarding and the look moved into them (N3). *Tests*
N4 counts every crate's, harken's included. *Storage* is `perf_d_bytes`'
live bytes per row with harken's two indexes at 100,000. *e, one page*
and the views are within run-to-run noise of each other between the
formats (each moved by 20–50% across the three runs).

**Measured, native.** Smaller: the library is 0.24× v4's (6,125 lines
with the FFI bridge in it, against 25,537) and the phones' runtime 0.06×
(2,394 lines of bridge and generated types against 40,224 of Kotlin and
Swift); the peer is 0.30× v4's client and server, the tooling 0.41×,
harken's domain 0.64×. Larger: tests (1.07×, harken's and the bridge's
included) and the phone apps' own code (1.36×, what moved into them).
Faster than v4: everything Part 3 times but the linked mutation, which is
at par (1.06×) because it encodes and decodes the frames; a peer alone
0.37×, `add_song` 0.25×, a rebase page 0.47×, a `Batch` decoded in 0.13×,
initial sync 0.28× per entry. Postcard is why N4 beats N1: against JSON
it carries 0.36× the bytes, decodes a page in 0.18× the time with half the
allocations, opens a 400,000-entry chain in 0.56× (5.8 s; N1 measured
32 s, before each entry was read once), and halves `add_song` and a local
mutation. Slower or larger than v4: initial sync still carries 1.24× the
bytes (facts always, §1.6), and a `Batch` decode allocates more than v5
interpreted did (2,817 against 2,567: a row is a `Vec` per fact). Not
verified: the runs are one machine's, noisy at 20–50% for the smallest
shapes; the browser and phones run the same kernel but none was timed;
harken's views, scanner and search shapes are not measured; v4 and v5 are
the recorded numbers, not re-run this hour.

**Size at M1**, v4 against v5 interpreted (`loc-v5.txt`, after P1 `loc-p1.txt`; non-blank, non-comment; Unicode tables excluded)

| | v4 | v5 | v5 / v4 |
|---|---:|---:|---:|
| library source | 25,537 | 6,526 | 0.26 |
| client + server source | 6,843 | 1,465 | 0.21 |
| harken's domain | 3,711 | 2,583 | 0.70 |
| all-in source (library, client, server, auth, domain) | 37,501 | 11,984 | 0.32 |
| dev tooling (sim, trace, fuzz, vectors) | in `ark` | 1,441 | — |
| tests | 18,916 | 11,257 | 0.60 |
| concepts a port carries | 5 sub-languages, 2 execution paths, the lifecycle in 5 places | 1 language, 1 path, 1 machine | — |

(The M1 speed table is the one above without its N4 columns.)

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

**S3a** (`dd26e71`…): **a peer is three instances wired; an effect is a
row.** *Instances.* `ark_peer::Instance` (`instance.rs`) is one machine and
what keeps it — a disk or none, the open from it (`stored`), every
`Persist` written in order, a failed write kept with its failure
(`flush`), and one fold rule, `Fold { keep, slack, rows }`: once the
entries above the snapshot outnumber `keep + slack` (+ the rows held, when
`rows`), all but `keep` are folded — a server `Fold::keep(k)` (keep `k`,
fold at `2k`), a local instance `{0, 64, rows}`. It is over the machine,
not a schema; typed access is layered on (`read::<S>`, `write::<S, P>`),
so a `Peer` is three of one type — `log`, `live`, `local` — and the
wiring: the link, the hub, frames, what it was told, and the routing of
`Send`/`Fanout`/`To`/`Changes`/`Refused`/`Confirmed`, matched on which
instance spoke. A closure's write is the kernel's `Input::Write { facts,
now }` (an authority's only): an entry naming no function, whose facts a
replay applies and nothing runs again — the fake `write` procedure,
`admin::Raw` and `Handle::author`'s one-entry function go, and the admin
page's raw write and `ark_auth::server::Hub::write` are the same input
(`Peer::write`, `Handle::write`). `Local`'s columns are typed (`Origin`
and `EntryId` describe as `Id`s, `Entry`, `Logged`, `(Seq, EntryHash)`):
`ark_peer::Bytes` and its `read()?` chains go, `ark::row::Bytes` being a
row's encoding as a column (the volatile space's); the volatile space and
what waits are written whole. *Effects* (`ark::effect`, §1.16): a
mutation may only record facts; acting on the world outside is a **job**,
a row whose `state` is

```
Job<T> = Pending { due_ms } | Claimed { by, at_ms } | Done(T) | Failed { why, retry_ms } | Cancelled

Pending ──claim, now ≥ due──► Claimed ──finish Ok──► Done(T)
   │                           │  ▲  └──finish Err──► Failed ──claim, now ≥ retry──► Claimed
   └──cancel──► Cancelled      └──┘ claim, now ≥ at + lease (another's claim)
```

`#[derive(Effect)]` (`#[effect(lease_ms, retry_ms, role, owner = field)]`,
a minute each by default; one key column) registers, through
`Effect::register::<S>`, `<table>.claim {id, by}` (refused unless due, and
without the role), `<table>.finish {id, by, outcome}` (refused unless
claimed by `by`), `<table>.cancel {id}` (refused unless pending, and but
by the owner — or, with no owner, the role) and the query `<table>.due`;
the authority's log order decides a claim against a cancel. A done job
carries its outcome; a variant carrying one value is a record of it
(`{"Done":{"value":…}}`), so the phones' generated types read it. Two
registrations on a peer: **a handler**, `peer.handle::<R>(|peer, row|
-> Result<R::Outcome, String>)`, a plain function on the driver's thread
(ark-peer is pumped, not async: `Peer::jobs` runs at each `pump`, and
`deadline` wakes for the nearest due): it keeps `R`'s rows not over from
every durable change, claims each due one by a name drawn at
registration, runs once the claim is confirmed with the row (its key the
idempotency key) and the peer, and finishes with its answer — never in a
replay, never on a peer that did not register it, and a claim from before
a restart is due again only when its lease runs out; **a reconciler**,
`peer.reconcile::<T>(space, view, input, |rows: Vec<T>|)`, told a view's
typed rows at the start and on every change, no claim. Through the bridge
a kind's due rows are `Peer::due(kind)` (rows as JSON, and when the next
falls due), a view's rows stream through `view` as before, and the claim
and the finish go through `mutate`. **The reference job** is the
scanner's look: harken's `look` table (`Look { id, paths, state:
Job<Looked> }`, `library.look`), asked at boot, on a rescan and per
settled watch, done by the scanner's own handler with what it read and
authored — no feature added (harken's schema `748e53c5…`, its pins
re-derived in both formats; the phones' types regenerated). Not added: a
simulator scenario (the simulator's domain is the demo, which the vectors
pin). Source: `ark` 5,175 → 5,232 (Part A: `Input::Write`, the log's
values as columns) → 5,458 (`domain/effect.rs`, 216), `ark-macros` 503 →
570, `ark-peer` 2,518 → 2,414 (Part A) → 2,550 (`jobs.rs`, 121),
`ark-ffi` 473 → 487, harken's domain 2,399 → 2,429, server 4,767 →
4,787; the default `tools/loc.sh` set 19,013 → 18,966 (Part A) → 19,439.
Part A came in at 2,414, not the ≈2,250 asked: the instance is one now,
but the rows, `Shared`, `Logins` and `preset` stay. Tests 431 passed (10
new, each falsified once). Not verified: a handler's lease against a
look longer than ten minutes (another process's claim could take it,
and the first's finish is then refused); a Kotlin or Swift handler (the
bridge's `due` is tested from Rust; the phones' types regenerated, not
compiled here); the browser's `localStorage` local instance (built for
wasm32, not run).

**S2** (`338333a`…`d9ec095`): **what a peer keeps beside its log is rows,
by two concepts.** A table is `Durable` or `Volatile` (§1.3), nothing new; an
instance is wired — a `Peer`, linked or serving — or not: an
`ark_peer::Instance<S, P>`, a machine in the authority role never offered a
link. So four combinations: durable synced (the app's log), volatile synced
(its live state), **durable local** (`Instance::open(disk)`: the log's own
`kv`/`log` format, one entry per write, folded into the rows whole once the
entries outnumber the rows) and **ephemeral local** (`Instance::ephemeral()`:
volatile tables, no journal). A write is a typed run over `Db<S, P>` whose
changes are the one entry the machine sequences; a local table is a table of
another `#[derive(Schema)]` struct, no attribute. `ark_peer::Local` holds the
hub's journal (`origin` {origin, actor, last}, `verdict` {entry, why},
`waiting` {origin, local, who, entry} — new: what waits behind a held intent
now survives a restart), its volatile space as it closed (`volatile`, the
space's rows and its log's row), a device's `replica` row (origin, last
`local`, who last signed in, fork, joining) and `pending` {local, entry},
under the `local.` names of the peer's disk (`Shared`); `ark_auth::remember`
is the `login` table {server, user, name, email, roles, session, token,
expiry} that `ark_peer::Logins` opens per app (or `Logins::at(dir)`, which
harken-peer uses); harken-server's `HarkenLocal` (`print` {path, size, mtime,
artist, covers}, `art` {name}: the pictures stay files) is the server's, not
the domain's — the domain is what every peer links, phones too, and none of
these rows reaches a log. Gone: the `journal`, `live`, `pending` (and its
record format), `origin`, `fork`, `joining` and `who` blobs (a disk holding
any is refused), `logins.json` and the per-server `localStorage` keys,
`library-files.json`, harken-peer's `login.json`. **A run writes its own
space's tables, and rustc says so**: `Row::Space` is `ark::db::Durable` or
`Volatile` (the derive writes it from `#[ark(volatile)]`), `Db<S, P>` and
`Tab<R, P>` carry the run's (`P = Durable` by default, `Domain::live` takes
`Db<S, Volatile>`), and `insert`/`update`/`put`/`delete`/`remove` exist only
where `R::Space = P` — a `compile_fail` doc-test in `db.rs` holds it; the
machine's refusal stays for a function run on the other space's machine and
for `Dynamic` rows. Source: `ark` 5,154 → 5,175, `ark-macros` 502 → 503,
`ark-peer` 2,123 → 2,518 (`local.rs`, the instance, the schema, `Shared`,
`Logins`, `preset`), `ark-auth` 1,051 → 1,037, harken's domain 2,399 (its 32
live transitions take `Db<Harken, Volatile>`), server 4,742 → 4,767 and bins
1,361 → 1,352; the default `tools/loc.sh` set 18,610 → 19,013 source — **up
403, not the 700 down asked**: the nine formats were small (about 230 lines
of codecs, file handling and blob names went), and rows cost a schema, a
diff against what is kept and the instance. Tests 421 passed (10 new, each
falsified once), no vector moved (the side blobs were never in one). Not
verified: an old data directory is refused, not migrated (a device's pending
intents, a server's refusals, a login, the prints — the last only a full
read); the browser's `localStorage` path built, not run; the perf numbers
not re-run (a mutate's local write is one entry, under 512 bytes at 8,000
pending, as `a_mutate_writes_one_pending_record` holds).

**S1** (`17fe168`…): **who is signed in is rows of the app's log.**
`ark-auth` owns them: `SignIn` and `Role`, `Revoke`/`Grant`, the
functions `session.revoke` (its owner's only), `session.current`,
`session.mine`, `role.grant`/`role.revoke` (an `ADMIN`'s only), written
once over any schema that is `AuthTables` (`ark_auth::install!(App)` for a
schema whose `sign_in` and `role` fields are the two tables;
`.with(ark_auth::register::<App>)`), and `feed(scope)` holding a
connection's own person's sign-ins and roles. The token is the row's id
and the server's HMAC of it, the key `session.key` in the data directory
`Auth::open` is handed; `Auth::authenticate` judges a token by the HMAC and
the row (live, its roles merged with the role table's and the
configuration's), `Auth::watcher` names the connections the hub closes (a
session revoked, gone or expired at the clock's tick, a person whose roles
changed), `Auth::issue` writes the row through `server::Hub` (which
`ark-peer`'s `Handle` is), `Auth::own` keeps a server's own peer's session
across restarts. `ark-peer` plugs them in (`Server::auth`,
`Handle::watched`) and reads them on a device (`Peer::current`,
`Peer::sign_out`, the preview's roles joined by the session's); `ark-ffi`
says `signed_in`, and `logout` revokes. Gone: `sessions.json` and
`SessionStore`, `/auth/whoami`, `/auth/logout`, `issue/redeem/whoami/owns/
revoke/on_revoke` of the store, `HARKEN_SESSION_KEY_FILE`. harken keeps
two fields, the install line, the registration and its scope's table list
(`session.feed`, fed with `HARKEN_SCOPED=1`). The dependency stays
`ark-peer` → `ark-auth`: `ark-explorer`'s page links `ark-auth`, so the
other way is a cycle; `ark-auth` speaks to the hub through `server::Hub`
and the kernel's `Store`. Source: `ark-auth` 999 → 1,051 (the domain
fragment 120, the hub's side; the 550–700 aimed at would mean cutting
OIDC or the clients), `ark-peer` 2,041 → 2,123, harken's domain 2,373 →
2,399, server 4,735 → 4,742, `ark-ffi` 469 → 473; `ark-auth`'s tests over
a schema of their own (`tests/rows.rs`, 12, each falsified once). Not
verified: a real OpenID Connect provider (the callback's row is written in
`tests/oidc.rs` against the test's provider only); the phones (the
generated types unmoved, the JVM and iOS builds not run here); the web
client signing out in a browser.

**N4** (`9907e0f`…): **the value format is postcard; the docs are the
tree's.** `ark::format` writes rows, inputs, entries, frames, journal
records and the hashed bytes in postcard by default (`values-postcard`) or
JSON (`values-json`, which wins when on); keys stay memcomparable; JSON is
the human form under both, `ark::json::Seed` reading a value of a `Type`
back from either, and the FFI, `harken-peer` and the explorer cross JSON
through the function's types. Measured the same hour, alternating
(Part 3, *The value format*): postcard carries 0.36× the bytes, receives
in 0.46× the time, decodes a page in 0.18×, and halves a local mutation
and `add_song`, with nothing slower outside noise — so it is the default.
Open reads each entry once (rows fitted only when not this schema's, the
foreign entry found by the digests): an alone chain of 400,000 opens in
5.8 s (N1 32 s; JSON now 10.3 s). Re-derived: `hash/`, `protocol/` (each
frame its bytes and its JSON), `trace/`, `rebase/` (`fleet-seed-7`
`54e6f160…`, the scenario `bde6d903…`, harken's schema `02372c9b…`),
the JSON pins kept beside them; `views/` unmoved. Docs: `spec/README.md`
indexes every module (a test holds it), `RUNNERS.md` trimmed to
`ark-dev`'s runner, `AUTHORING.md` is "Writing a domain" (145 lines, each
example held to its source by a test), the root README, `docs/arkdb.md`'s
note, Part 1's superseded sections noted. Source: `ark` 4,752 → 5,154
(`format.rs`, `Seed`, the JSON boundary), `ark-ffi` 453 → 469; tests
472 in `checks.rust`, 0 failed (`ark-ffi`'s denied-login test, which raced its
own driver in the sandbox, pumps by hand); fuzz 1,000 sessions with traces,
0 failed. Not verified: a build
of `values-json` is not held to the vectors (its tests skip them, saying
so); a non-canonical postcard frame decodes (only rows are held to their
one encoding, by re-encoding); the phones' JSON crossing on a device
(the JVM tests and the iOS type-check only); the perf numbers are one
machine's, noisy at 20–50% on the smallest shapes.

**N1** (`1e60dda`…`b3da7f4`): **one runtime; the domain is Rust** (§1.16).
ArkLang, its checker, interpreter, std and Unicode tables, the CAS and the
`eval/` `verify/` `module/` `text/` vectors are gone; an entry names its
function, values are serde JSON and keys memcomparable; JSON plans
(`ark::dynamic`) carry the `views/` vectors. Source: `ark` 9,480 → 4,752,
`ark-macros` 74 → 502; harken's 1,834 lines of ArkLang (and the demo's 69)
are 2,373 of Rust; all-in 58,585 → 53,045. Vectors: `views/` patches
byte-identical; `hash/`, `protocol/`, `trace/`, `rebase/` re-derived
(`fleet-seed-7` `8316d1f4…`, was `ff04f56d…`). 394 tests (iced demo 69),
fuzz 1,000 with traces, 0 failed. Release, before → after, µs: (a) whole
38 → 48, mutate 10.9 → 9.2, alone 15.8 → 16.7 (2,000); (g) sequence 17 → 20;
`add_song` 232 → 144, alone 109 → 59; (e) acks of 1,000 pending 215 → 39;
views hydrate (16,000) 51 → 13 ms; but JSON costs bytes and decoding: (d)
267 → 557 B/entry, receive 8.6 → 21 µs/entry; (h) a page 55/247 → 159/730
µs; a 400k chain opens in 32 s against 9; 2,000 intents join in 14 s
against 7. Not verified: the Kotlin and Swift runtimes (frozen) read none
of the new vectors; harken-peer's command JSON keeps its `$id` dialect;
open re-checks every row's encoding; FFI (N3) not begun.

**K3b** (`e60c1b2`…): **one peer crate.** `ark-client` and `ark-server` are
`rust/ark-peer`: one `Peer` over one disk that dials (`link`: a `Wire` a
`Dial` makes, redial with backoff, close 4401 a denial that stops it, the
waker) and listens (`hub`: identity and feed per connection, `Send`/`Fanout`/
`To`, written before delivered, the journal; `serve`: the thread, the socket,
the admin listener, web, auth glue); the storage format is written once over
named blobs (`Files`, `Memory`, a page's `Local`). The app kit (`app`):
`mutate`/`call`/`query`/`view` by name, the stubs through `gen::Call`,
standing, `Update`s; iced's `client.rs` 971 → 119. `ark-auth` is one build.
Converge and the fleet's seeder run on `ark-dev`'s simulator. Source:
`ark-peer` 2,064 against 1,826 for the two and 1,300 asked — over: it holds
the app kit (236) and the browser's socket and disk; iced 8,860 → 8,171,
server 4,874 → 4,808; all-in 32,854 → 32,320. No vector moved; fuzz 1,000, 0
failed; a/b/c/`add_song` ≤1.04×. A refused device write is retried, not lost.
Not verified: the browser build drawn (it compiles again); wss.

**A4** (`dc97b35`…): **the domain is written in ArkLang; the builders go.**
`harken/domain` is eleven hand-written files, 1,834 lines of ArkLang (311
of comments) against the builders' 3,149 of Rust; all 127 functions hold
their pre-A3 pins, names aside (`port.rs`: none moved), and every file is
its forms printed (`text.rs`). A function keeps its binders' names, so a
renamed local is a new hash. Added: `(?? o.f d)`, `(?. o f)`, `(as (row t)
(record …))`, a `map`'s lambda wanting its element, the Lisp layout,
`build_dir`/`load`, `arkc` (74). `ark::gen` (395: generator ~190, `Id`,
`Call`, `fits`, prefix reading) writes types and stubs; both apps generate
theirs in `build.rs`. Deleted `ark::author`, four derives, harken's domain
Rust: `ark` 9,766→9,487, `ark-macros` 453→74, `harken/domain` 3,149→237.
`sexp.rs` is 1,629, not ≤900: the declarations' spelling holds the pins.
Every vector family but `hash/` re-derived, every state and answer held;
fuzz 1,000, 0 failed. Not verified: the browser build; a grown row in an
iced view; pre-A4 module files no longer decode (`Function` grew).

**K3c** (`0918028`…`acf0447`). **A fresh read keeps nothing**: `view.rs`
defines each operator once (`each`, `order`, `count`, `tally`/`aggregate`,
`join_key`/`joined`); a view's nodes run them over deltas, a fresh read
over lists in hydration's order — a sorted `Take` orders its window only,
a keyed `Join` hashes its smaller side once; rows read unchanged.
`add_song` 25.19G instructions (step 4 24.90G, 1.012×; step 5 28.10G);
whole, release medians at 500/2,000/8,000: 233/228/247 µs (step 4
226/232/241); views push at par. **Only the log's rows are journaled**:
`Record::Logged` (`writes` → `log`; a directory with `writes` refused),
open appends them (`Store::append`): 620 → 364 bytes per entry, open of
8,000 entries unchanged. trace/: 88 persist records re-derived. Fuzz
1,000 sessions, 0 failed, 2,997 traces replayed. Source +93.

**A3** (`8857083`…): ArkLang is S-expressions, `ark::sexp` (reader,
layout, lowering through the checker, printer): 1,400 source lines against
600 — over, because printing recovers what the bytes leave out (binder
names, wanted types, `(as …)`, kinds) and lowering must rebuild the
builders' declarations byte for byte; `text.rs` (1,811) deleted, `ark` −335.
**Names are in the module** (`Decl::names`, checked; the derives write
them): every function declaring a type moved, and `port.rs` holds harken's
127 to the macros' pins with names set aside. Re-derived: eval/, verify/,
module/, views/, trace/, protocol/, `fleet-seed-7`'s script; no state hash
moved; hash/ and `fleet-fuzz-*` held (their functions declare nothing).
`domain.ark` builds to `harken.ark`, `demo.ark` to the builders' demo, 200
fuzz modules to themselves; fuzz 1,000 sessions, 2,997 traces replayed.
Added to the grammar: `volatile`, `refs`, `default`, `as`, `lit`,
`variant`, `.`, `asc`, `(len LO ..)`, `ctx.roles`. Not verified: `ctx.device`
(no IR) and a field's `exists` are refused; the explorer's pane undrawn.

**K3a** (`791b3d0`…`17322ae`). **Four frames**: `Hello`, `Page`,
`Snapshot`, `Verdict { Ok | Refused | Held }`; a peer's page is its
intents (`Logged::intent`, sequence 0; a push is a page of one) or its
chain made alone, adopted when it links onto the head; an authority's, its
log. **The log is a table**: the store's `_log`, by sequence, a `Logged`
per row, its invariant the chain (`Store::apply_kv`: ends only, linking,
never edited), outside the schema, the state hash and sent snapshots. One
durable format: `Record::Writes` (facts and the row, appended) and
`Record::Rows` (the store whole, folding them); the horizon deletes below;
open replays writes to the first that fails, then writes the rows.
`ark-client::storage`, the hub's too, is `kv` + `writes` and refuses the
old `snapshot`/`chain`. **One evaluator**: `interp.rs` has no relational
arms; `view::read` hydrates a spine once and routes as P1 did. Vectors:
protocol/ renamed, re-derived; trace/ twice (frames; persist and open
bytes); `fleet-fuzz-seed-7144` (pushes paged together: other network
draws); the rest unchanged. Fuzz: 2,000 sessions, 0 failed, 5,975 traces
replayed. `add_song` 1.13× its instructions (callgrind), a and views at
par; source +125. Not verified: an old data directory is refused, not
migrated; facts are on disk twice per entry, unmeasured; perf b–i not
re-run; a fresh `Join` calls `on` for key partners only.

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
change is applied by hand to `.github/workflows/build.yaml` (the owner's credentials; this session cannot push workflow files). The
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

**T1** (`189ea8f`…`702215e`): over budget, both. `ark-auth` on
`openidconnect` 4.0.1 over its `ureq` agent, 1,422 → 1,000 source: OIDC
264 → 128, the rest the sessions, routes and two clients `ark-server` and
harken use unchanged; the ID token's signature is now checked (keys fetched
per exchange), its issuer exactly, extra audiences refused, the secret sent
as Basic (README). `tests/oidc.rs` signs in through a provider in the test;
the JWT/base64 tests went with the parsing. `ark-explorer` 3,361 → 2,268
(the browser 2,722 → 1,630; `data`, `wire`, `admin` kept as the hosts'
wire): sortable grids, a call screen of typed text fields (`[a, b]`,
`(a, b)`), the log; dropped the console, recent calls, nested inputs, the
picker's labels, gone rows and lit answers (3 tests). harken's pass
unchanged; `ark-admin` builds for wasm32. Not verified: a real provider.
