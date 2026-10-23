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

> *Superseded (history, in git before T1).* §1.16: a value is its serde bytes in postcard, a key the same value memcomparable; the distinct hash spaces survive.

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

> *Superseded (history, in git before T1).* §1.16: the domain is plain Rust; a view is a typed `Plan`.

### 1.5 Functions by hash; a module is a binding map

> *Superseded (history, in git before T1).* §1.16: an entry names its function by name; the schema stays data, hashed and prefix-checked.

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
intents; `join` turns its chain above the fork into pending intents,
pushed as after any connection, and the server judges each as the push it
is — one refused loses that entry, not the chain; the peer rebases onto
what it answers. Both are the machine's own transitions.

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

> *Superseded (history, in git before T1).* §1.16: the attributes only register plain Rust functions by name (`spec/AUTHORING.md`).

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

> *Superseded (history, in git before T1).* §1.16: ArkLang, `arkc` and the binary module are gone; a domain is a crate.

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
   (`ark::format`): **postcard** since N4 (a struct its fields in
   declaration order, integers varints, an enum its variant's index) — no
   floats, maps as `BTreeMap`, one encoding per value, the canonical form
   hashed (the `values-json` build is gone, T1). JSON is the human form:
   what the explorer, the phones' bridge and
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
   recorded and a change to what it read re-runs that element only. A
   row without a Rust type is JSON under its table (`dynamic`): what the
   vectors, a host speaking JSON and the explorer hold. Its one query is
   `Browse` — equal columns, a key range, a sort by a column, the first
   `n` — a plan of the same operators, so a grid of it is kept by patches;
   anything more is a view the domain names. No plan is written as JSON:
   a `views/` vector is a named view of a reference domain (`ark-dev`'s
   `library`), and another implementation runs the view of that name.

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
rust/ark-peer/      the one peer that dials and listens: storage, link, hub (on a
                    thread, `handle`), the app kit; local instances and the
                    peer's own schema (`local`); the server's socket and web
                    build behind its `server` feature (axum, tokio)
rust/ark-ffi/       ark-peer's app kit through UniFFI, for the phones
rust/ark-dev/       the demo, simulator, trace player, fuzzer, vector writer (with
                    the `views/` vectors' reference domains, `library`), and
                    `gen kotlin|swift` (a domain's types for the phones)
rust/ark-auth/      dev sign-in and OpenID Connect
rust/ark-explorer/  the admin page (its routes behind `server`) and the explorer
                    over a replica
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

The tables below were recorded by harnesses that printed every shape and
asserted nothing (`tests/perf.rs` in `rust/ark`, `rust/ark-peer` — once
`ark-server` — and `harken/domain`, each shape a v4 one: a local mutation,
the round trip, offline pending, initial sync, the rebase, fan-out, frames,
harken's mutations, views at 1k/4k/16k rows). T3 replaced them, and they
are in the history up to its commit. What is measured now is held: each
crate's `tests/costs.rs` asserts the counts that stay put as the data
grows, and prints its timings from one ignored test, `timings`, run as
`cargo test -p CRATE --release --test costs -- --ignored --nocapture`:

| crate | held (asserted, each falsified once) | printed by `timings` |
|---|---|---|
| `ark` | a `Batch` of 256 decoded in ≤ 12 allocations an entry; a peer alone's step allocating the same over a log of 500 and of 8,000; one change on `playlist-with-items`, `top-two-by-pos`, `group-source` reading and allocating the same at 1k and 16k rows | a: a peer alone's add, log of 8,000; h: a `Batch` of 256 decoded; views: a change on each at 16k |
| `ark-peer` | an offline device's intents writing the same bytes at 100 and 4,000 pending; a fan-out ≤ one allocation per connection; ≤ 400 live bytes per harken `media` row with its two indexes | b: a linked round trip through the hub in process; c: an offline intent at 4,000 pending |
| `harken-domain` | the rows a mutation reads (`reads_are_bounded_by_what_they_need`); `add_to_playlist` allocating the same at a library of 500 and 8,000; `library` read whole in ≤ 32 allocations an entry | harken `add_song`, a library growing to 8,000 |

`ark-peer/tests/live_memory.rs` holds a live space's memory as before
(Landed: live fold). The numbers of the tables are not those of the new
benchmarks' shapes in every case (`a` is now a peer alone over playlists
of ten): a comparison runs both trees' `timings`.

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

### Landed: crane

The Rust builds in `flake.nix` are crane's, in layers per target: on the
host the dependencies (over a dummy of the manifests), the workspace once
over them, its test targets once over that; the checks only run tests, and
`clippy` and `fmt` are checks of their own. Features resolve once for the
whole workspace on the host (cargo's `feature-unification`), so `-p` runs
reuse the layers. The wasm pages share one dependency layer; each Android
ABI has its own, and the iOS targets one. Measured on this machine with
`--max-jobs 1`, `nix build` of `checks.{rust,ffi,traces,versions}` after a
comment added to `harken/domain/src/lib.rs`, dependencies already built:
`Compiling ark ` 8 → 2 lines (`Compiling` of any crate 2,142 → 27), 1,731 s
→ 399 s. From nothing but the fetched crates the new layers take 1,006 s.

### Landed: kernel layer

Each target's Rust is now three layers rather than two: its dependencies
(over the manifests' dummy, as before), **the kernel** over them, and
harken's crates over that. The kernel layer builds the workspace members
under `rust/` from a source that is `rust/` as it is with harken's crates
still the dependency layer's dummies, so it is keyed on `rust/**` and the
manifests, and it runs the same cargo commands as the layer above it (the
same units, the same features): over it cargo finds every kernel crate
fresh and compiles only harken's. Host: `arkdb-deps` → `arkdb-kernel`
(the kernel's libraries, binaries and test targets) → `arkdb-workspace` →
`arkdb-tests`; wasm: `arkdb-wasm-deps` → `arkdb-wasm-kernel` → the pages;
each Android ABI `harken-android-<abi>-deps` → `-kernel` → the library;
iOS `harken-ios-deps` → `harken-ios-kernel` → the xcframework. The APK's
host part (the Kotlin bindings and the host library its JVM tests load)
is a chain of its own, `harken-android-host-deps` → `-kernel` →
`harken-android-host`, built as an ABI is (`-p harken-ffi`, without the
workspace's feature unification, under which ark-explorer's `ui` — and so
iced — is on for harken-ffi too); UniFFI's bindgen binary is built over
it. `harken-android-lib` no longer has `arkdb-deps` or `arkdb-workspace`
in its closure, and compiling its host part compiles no `wgpu`, `winit`
or `iced` crate (the old `arkdb-deps` it waited on compiled 22).

Measured with `--max-jobs 1`, a comment added to
`harken/domain/src/schema.rs`, the layers below already built:

| | before | after |
|---|---:|---:|
| `checks.rust` (workspace, tests, the run): `Compiling ark `; any `Compiling`; wall | 2; 27; 729 s | 0; 11; 288 s |
| `harken-web` (the page): `Compiling ark `; any `Compiling`; wall | 1; 8; 119 s | 0; 2; 113 s |

The page's time is mostly wasm-bindgen and `wasm-opt`, not the compile.
The kernel layer itself, after a kernel edit: 16 `Compiling` on the host
(each crate's library and its tests), 13 for wasm.

Not done: splitting the host dependency layer around harken-iced. iced
is not harken-iced's alone — `arkui` (a kernel crate) depends on it, and
ark-explorer's `ui` is on workspace-wide — so `--workspace --exclude
harken-iced` still compiled `wgpu` and its stack, and a layer on top
re-runs whenever the one below moves, so it could not keep `wgpu` across
a kernel or domain feature change either. Not built here: the Android
and iOS layers (evaluated; CI builds Android), and nothing is through
Cachix yet.

### Landed: server layer

`harken-server` (the server and harken-peer; also `harken-serve`, the
fleet VM and the NixOS module's default package) was `crate` over the
host's workspace layer, so a server rebuild built `arkdb-deps` and
`arkdb-workspace` — iced's stack (`wgpu`, `winit`, `iced`) for a GUI the
server never links. It is now a chain of its own, built as the APK's host
part is (without the workspace's feature unification, so ark-explorer's
`ui` and with it arkui's iced stay off): `harken-server-deps` (the
manifests' dummy and the lockfile) → `harken-server-kernel` (rust/ as it
is, harken's crates dummies: keyed on `rust/**` and the manifests) →
`harken-server`, every layer `cargo build -p harken-server`, so the
kernel crates compile under the server's features and are fresh above.
`-p` of each kernel crate would not do: selected by itself ark-explorer
takes its default features, `ui` among them. The closure of
`harken-server` no longer has `arkdb-deps`, `arkdb-kernel` or
`arkdb-workspace`; the wasm pages (`harken-web-server`, `ark-admin`) were
already on the wasm layers alone. The checks, and every Rust layer and
package but the server's, are the same derivations as before.

Measured with `--max-jobs 1`, the fetched crates in the store: from
nothing, 275 s (232 `Compiling` in the dependency layer, 9 in the kernel
layer, 3 in the crate; none of them `wgpu`, `winit` or `iced`). After a
comment added to `harken/server/src/lib.rs`: 38 s, only the crate
layer rebuilt, `Compiling ark ` 0 and 3 `Compiling` in all
(harken-domain, harken-player, harken-server).

### Landed: live fold

The one-machine experiment's finding, fixed with the one machine: a
device's live `Instance` was never folded (`Compact` was an authority's
only), so every replica held every live entry for its life, and the
authority's live verdicts grew by one per entry. **A replica moves its own
horizon**: `Compact(n)` is taken in either role, clamped to the head (a
device alone, to its fork as before), the store made durable whole as
confirmed (`confirmed_rows`, its pending intents' changes undone; nothing
in the volatile space). A replica needs no entry below its head — the
rebase undoes and re-runs its pending intents by their own changes, a
`Hello` says the head, and going alone forks at the head — so a device's
live instance folds as `Fold::REPLICA` (`{0, 64, no rows}`): none kept,
folded once 64 are held. **Volatile verdicts are a window**: a volatile
authority keeps the verdicts of each origin's last 64 `local`s
(`peer::WINDOW`), forgetting those the window leaves as it moves (at most
64 per verdict, even past a gap); a push again inside it is answered as it
was, one below it is refused as overtaken by the rule that was already
there — so a live call from more than 64 calls ago, pushed again, is
refused though it was taken, as in the experiment. Durable verdicts are
unchanged. The server's live log was already folded at harken-server's
`keep` 256 (at most 512 held).

Measured by `ark-peer/tests/live_memory.rs` (`live_held_measured`,
debug profile with the kernel at `opt-level 2`; 10,000 `room.point` calls
from `C` devices in turn, every frame carried by hand; counted, not RSS):

| | before | after |
|---|---:|---:|
| `C` = 2: entries a device holds; the server's; live verdicts | 10,002; 493; 10,002 | 57; 493; 130 |
| `C` = 10: the same | 10,010; 501; 10,010 | 0; 501; 650 |
| a call's round trip, `C` = 2; `C` = 10 (µs) | 68.3; 171.9 | 68.5; 167.3 |

(A device holds 0 to 64, folding every 65th; the experiment measured
~575 B an entry.) The per-call time is unchanged: a fold is a split of the
log's map, and the chain is still hashed per live entry, which is the
speed the experiment's second machine bought and this does not.

Still growing, not changed: **a device's durable log** is never folded
either (its `Instance` has no `Fold`; its disk's `log` blob grows until a
snapshot writes `kv`) — the same `Fold::REPLICA` with `rows` would fold
it, once `reset`'s check for a late snapshot (`log().has(seq)`) also
takes one below the horizon as late; **the authority's durable verdicts**
keep an `Ok` per entry for the server's life (the refusals are rows of
the journal, rewritten whole at each refusal) — an `Ok` at or below the
horizon could be forgotten as the log folds, since a push there is
already answered `Ok { seq: base }`, as after a restart; a device's
`Standing` per durable intent, and its live refusals until the app takes
them. A device's relay and its frames to send are bounded; the hub's
deliveries are taken at each release. Source: `ark` 4,956 → 4,975,
`ark-peer` 2,550 → 2,555; tests `ark` 3,961 → 4,038 (three in
`tests/volatile.rs`), `ark-peer` 1,620 → 1,742 (`live_memory.rs`), each
falsified once. Protocol, vectors and traces unchanged. Tests 456 passed
(each crate's, one target at a time for the disk), clippy and fmt clean,
the fuzzer 4,000 sessions (seeds from 1, and from 2026 with every trace
replayed), none failed. Not verified: a fold under a pending live intent
in a fuzzed session (a session's live log reaches 26 entries at most over
seeds 1–300, below the 64 a fold waits for; `tests/volatile.rs` folds
under one by hand); the
release profile; a phone or a browser.

### Landed: speakers agree

In the server-split experiment, over sockets, each speaker read its
session off its own machine, and two speakers of one session acted on two
moments of it: a group joined, unjoined and joined again, one run in two.
**It does not reproduce on main**, and cannot in that form: the house
reads every session in one `hub.with` job off the hub's own live store
(`assistant.rs`, `ha::sessions`) and hands that one reading to every
speaker's reconciler (`Speakers::wants`); a speaker's machine is read only
to preview what it says. A bridge test
(`two_speakers_of_one_session_act_on_one_moment_of_it`: the study joined
and unjoined four times while another account presses pause and play as
fast as it can) passes on main, 20 runs of the bridge's tests in a row;
deciding for the study on the reading before fails it with join, unjoin,
join, two runs in three. What main had by job order and not by rule: a
speaker is never behind the reading after `Wires::receive` (a job's frames
go out before the next job, the read among them), but can be ahead, and
two speakers apart — `Handle::with` answers from inside its job
(`ark-peer/src/serve.rs`), so the next job's frames can reach one
speaker's channel and not yet another's before they are taken. Now a rule:
**a session is acted on only when every speaker standing in it has heard
the volatile log to the head it was read at, no more, and has nothing
unanswered** (`Wires::agree`); otherwise it waits for the next frame. It
is the house's, not `harken-player`'s: a phone's `Player` reads and calls
through one peer's machine, so it has nothing to agree with. A unit test
perturbs the order by hand (the kitchen given a frame the study is not)
and is falsified by not comparing heads and by not looking at what is
pending. Bridge runs, 20 in a row: before 20/20, after 20/20.
`harken-server` 4,517 → 4,599 by `tools/loc.sh` (the rule a dozen lines,
its in-file test the rest), its tests 6,675 → 6,748. Not verified: that
the rule ever changed what the house did on main (no run was seen to hit
the window); the poll's own calls (`Speakers::heard`) are not gated, as
they were not in the experiment.

### Landed: T1, a deletion round

Kernel and peer cut, each item its own commit (`e04380b`…). **A join
pushes intents, never a chain**: `Join` makes the chain above the fork
pending intents (with any intent that refused as the device went alone,
kept unrun for the server so its `local`s have no gap), makes the log
durable at the fork (ark-peer keeps the chain as pending rows first), and
pushes them as after any connection; the server judges each as a push, so
an entry it refuses loses that entry, not the chain. `adopt`, `refuse`,
the peer's second kind of `Page`, `Peer::joining`, the re-handover in
`connected` and the `joining` fields (`Replica`, `Act::Open`, `Status`,
harken-peer's status) go; ark-ffi's status says `joining: false`. Found by
the fuzzer on the way (seed 2464): without the durable truncation a device
reopened after a join held its chain as its log. **Held keeps nothing**:
`Held` has a live use — a function an older server's build lacks — so it
stays; `Input::Loaded` (no sender but a test), the authority's queue
behind a held intent and `Local`'s `waiting` table go: what comes after a
held intent from its origin is a gap, answered by nothing and pushed again,
after it, on the next connection (`connected` re-pushes every pending
intent). Then one path for an authority's own intent (`decide`); the
`values-json` build (35 cfg sites) and the JSON-only serializer arms of
`ark::json`; uncalled API (`encode_into`, `db::write`, `Ctx::draw`,
`Log::len`/`is_empty`, `Functions::forget`, `Peer::who`), `Store::new`
and `from_rows` one each (`(schema, space)`; `from_rows` reads another
schema's rows, §1.2), `Ctx::new` and `Default` for 28 hand-written
literals; `Peer::hello`, one verdict send, `Store::undo`, one scan operator
over stored keys; the log's ends taken from the entry, not postcard
offsets; `EachOp` for `filter`/`map`/`sort` and `Members` as
`btree_map::Values`; views made again by the peer under their number
(`Update::Reset` now means read the rows afresh; the simulator and
harken-iced lose their re-view; ark-ffi's is left); a scope its own kind
in `Functions` (`Runs::Scope`, asked by table name); `Entry::new`. **No
web server on a phone**: the admin routes are `ark_explorer::serve`,
`Catalog` is `ark::domain`'s, ark-peer's hub thread is `handle` and its
socket side (`serve`, `web`, axum, tokio) the `server` feature, as is
ark-auth's routes'; ark-ffi, harken-ffi, harken-iced, harken-player and
harken-domain turn it off — `cargo tree -p ark-ffi -e normal`: axum 0.
Shared test support, one RNG (`ark_dev::sim::Rng`, sequences unchanged),
one clock, one hex decoder, `ark::row::plural`; arkui's uncalled
`Context::close`/`grid` and `Images::with_bound`; `plan-v4.md`,
`plan-phones.md` and `scopes.md` deleted (`docs/README.md`), plan-v5's
history sections a line each; the flake's `fuzz-smoke` check (the
workflow still names it: drop it there by hand). **Not done**: the typed
prefix machinery (item 12) — harken uses the by-reference forms (22 calls:
`db.listening.has(&user)`, `.from(&input.file)`) and three-column whole
keys (`db.credit.get((a, b, c))`), so there is no single form to keep.
Source by `tools/loc.sh`: `ark` 4,975 → 4,517, `ark-peer` 2,555 → 2,392,
`ark-dev` 2,142 → 2,108, `ark-explorer` 2,251 → 2,353 (the admin routes
moved in), `arkui` 2,949 → 2,935, `ark-auth` 1,037 → 1,038, domain 2,429;
the default set 19,395 → 18,829 source, tests 12,767 → 12,524; harken-server
4,599 → 4,596, harken-iced 8,163 → 8,155. `Space::` 156 → 197 (the store's
constructor names its space). Vectors: `trace/` re-derived (an open no
longer says `joining`; a join's steps are a push), `protocol/client-chain-durable`
gone; `hash/`, `rebase/`, `views/` and the rest of `protocol/` unchanged,
no schema hash moved. Tests: 451 passed, 30 ignored (each crate's, one at a time for the disk); new ones falsified once each (`alone.rs` three, `joining.rs` two, `rebase.rs`'s gap, `ark-peer` `local.rs`'s reopen after a join and `views.rs`); clippy `-D warnings` clean; fmt clean but for `harken/server/tests/bridge.rs:1432`, as on main; the wasm32 check of `ark-peer`, `ark-auth`, `ark-explorer` and `harken-iced` passes. Fuzzer 1,000 sessions from seed 1 and
1,000 from 2026, every trace replayed, none failed. Not verified: a crash
between ark-peer's pre-write of the chain and the join's step (the merge
drops a duplicate by id, by reading); the phones and a browser running it.
### Landed: typed views

The experiment of one generator's recommendation, taken: **views are
typed, functions are by name.** `#[ark(ffi_view)]` on a view's row type
(`Row`, `Input` or `Describe`; `ark-macros/src/ffi.rs`) writes its twin, a
UniFFI record named as the type with the conversions both ways
(`ark_ffi::Twin`, per the domain's UniFFI tag), and its view object
`<T>View` — made by a view's name and its input's JSON, refused when that
view's rows are not `T`, with `rows()` and `take() -> <T>Taken {update,
rows}`, the rows crossing Rust as their bytes and decoded there, never as
JSON per row (`ark-ffi/src/typed.rs`: `Twin`, `RowId`, `Op`, `Taken`,
`Typed<T>`, `Peer::view_typed`). `#[ark(ffi)]` writes the twin alone, for a
type a row holds (nested sums, options, lists, ids). harken asks for it
behind its domain's `ffi` feature (which harken-ffi enables) on
`LibraryEntry` and `Playlist`. `ark-dev gen` stays, with its pin: of a type
UniFFI declares it writes only its JSON, as extensions, and a view stub
whose rows are such a type makes that type's view object, adapted into the
generic `View<T>` (`Typed.kt`, `Typed.swift`); so `library`, `album`,
`artist`, `recording`, `search`, `playlist`, `playlists` and `playlists_of`
are typed on both phones, and the screens' code is unchanged but for eleven
Kotlin imports and `Rows.swift` taking UniFFI's `Hashable`. Not landed from
the experiment: `#[ffi]` on mutators, views and queries and its functions,
the job's twins, the inputs' twins. Fixed on the way: a kept view made
again after a reset was made from its input's JSON passed as bytes (a
reset view read nothing); it keeps the bytes now.

*Measured.* Source (`tools/loc.sh`, src): `ark-macros` 570 → 730 (`ffi.rs`
151, the experiment's 371), `ark-ffi` 487 → 634 (`typed.rs` 103), `ark-dev`
2,142 → 2,246, the domain 2,429 → 2,433; tests: `ark-ffi` 199 → 230,
harken-ffi 627 → 732. Hand-written, non-blank: `Typed.kt` 77 → 111,
`Typed.swift` 88 → 114, `Phone.kt` 164 → 165, `Rows.swift` 80 → 78.
Generated, non-blank: `Harken.kt` 837 → 835, `Harken.swift` 1,292 →
1,277; UniFFI's new `harken_domain` 1,736 Kotlin and 1,009 Swift (+554
header), `ark_ffi` 2,974 → 3,173 Kotlin and 1,941 → 2,108 Swift,
`harken_ffi` unchanged. The library for the host, release, stripped:
8,012,904 → 8,091,816 B (+1.0%; the experiment's whole prototype +1.7%),
defined dynamic symbols 285 → 370. The domain alone, dev, after a one-line
edit: 2.46–3.01 s → 3.52–3.79 s with `ffi` on. On the JVM over the host
release library, the same 2,000 `LibraryEntry` rows of `library.library`
read: 12.1–13.0 µs a row by name as JSON (`element_json`, `org.json`, the
generated `of`) against 2.2–2.5 µs typed — 5× (one machine, three rounds,
the rows equal either way; not a phone, not ART's `org.json`). Tests: the
view objects over harken's views, a wrong row type refused, nested twins,
the reset fix (`ark-ffi`), and the pin with `Playlist` left out of the
typed — each falsified once; the app's JVM tests 99 (one skipped) over the host
library, by the throwaway expression of S3b. The iOS type-check passes over
the new bindings (run by hand with `typecheck.sh`; it found `Rows.swift`'s
own `Hashable` redundant). Not verified: an APK, a phone, an iPhone; a
typed sum in Kotlin (UniFFI writes an all-unit enum as an `enum class`,
which the extension's `is` would not match; harken has none).

### Landed: T2, one client runtime

Five items, each its own commit (`cb0b747`…`1f3cf4e`). **One player**: the desktop
and browser client's own reconciler goes — `listening.rs`'s `reconcile`,
its memory fields (`met`, `alone_playing`, `started_for`, `advanced_for`,
`play`/`counted`, `asked`, `applied`, `seen`, `reported`), its `Sound`, its
presses and the `input` module; the window's `Player` is a
`harken_player::Sound` (`Sounding` in `main.rs`: the browser's `<audio>`,
which now says whether it was asked to play and whether it stopped by
itself — `asked`, `take_stalled` — and the desktop's silence, which never
plays), over the window's queue, which it plays through on its own as a
phone's player does; every press, queue edit, volume control, transfer and
group change is a `harken_player::Press`; `Player::step` returns the
refusals it takes. So there is one reconciler for the phone, the speaker,
the browser and the desktop. The nine iced tests were already the phone's
(`harken/ffi/tests/player.rs`) and the player crate's: output obeys →
`the_output_obeys_the_session_and_nobody_else_makes_a_sound`; the session
follows the output → `the_session_follows_the_phone_and_the_phone_follows_a_seek`
and `a_player_drifting_from_the_session_is_followed_and_told_nothing`; a
track ends → `a_track_that_ends_is_said_once_and_the_next_one_plays`; a
play once → `a_play_is_recorded_once_by_the_output_alone`; both ways of
coming back → `coming_back_to_a_session_sounding_elsewhere_stops_this_phone`,
`coming_back_playing_to_a_quiet_session_makes_it_this_one`; unlinked →
`a_phone_unlinked_is_its_own_session`; volume →
`the_phones_volume_is_its_entry_and_only_a_hand_is_said` and
`a_volume_is_told_and_a_hand_on_the_knob_is_said`. The one iced-only case,
a hand-off picking up where the track stopped, is
`a_hand_off_picks_up_where_the_track_stopped` beside them (falsified by
loading at the clock's count, not where the session stands). The iced
reconciler's STALL_MS/REPORT_MS timers are the core's rules now (a stop by
itself is an event; a drift is said once per clock). A browser alone now
counts its plays, as a phone alone does.
**harken-peer over `ark_ffi::Peer`**: its pump loop, stdin thread, held
lines and turn go; the peer is pumped on the FFI peer's thread and its
commands go through the FFI peer's JSON (`check`, `mutate`, `query`,
`call`, `standing`, `status`, rejections, live refusals, `login`,
`logout`); stderr's linked/dropped/denied lines come from a `Listener`.
ark-ffi gains `Peer::of` (a peer its host opened its own way, driven as
`open`'s). **One typed live view**: `ark_peer::Watched<T>` (`watched.rs`;
the peer's private `Watched` is now `Kept`) — a view by name and input, its
rows decoded by a reader and spliced as it moves, read whole on a reset,
asked again when a refused query might answer, opened again for a new
login; harken-iced's `Open`, `splice` and its row fields go (the lists are
`Peer::items()`, `albums()`, …), ark-ffi's `Kept` is a `Watched<()>` (rows
still read from the peer as asked, so nothing is held twice) and gains the
reopen for a new login; the phones' `View<T>` stays, no FFI signature
changed, no binding regenerated. **The step-and-settle loop**:
`ark_peer::link::until(within, every, done)` is ark-peer's test support
`until`, harken-peer's `wait`/`settle`, the server tests' `pump_until` and
`eventually` and the fleet's `eventually`; the desktop's copy went with
item 1; the simulator's `settle` counts rounds with no clock and the
phones' hall carries until two quiet rounds, so both stay. **Configuration**:
harken's device and hub configuration is `harken_player::config` (`peer`,
`hub`, `device`, `THROTTLE_MS`) — runtime, not domain, so the player crate,
which every client and the server already link — used by harken-iced,
harken-ffi (`config`, `Hall`), harken-peer and harken-server
(`hub_config`); the generic server configuration is `ark_peer::Serve`
(`settings.rs`, the `server` feature: `SignIn`, `Keepalive`, `Retention`,
`roles_of`, `Serve::from_env(prefix, listen)`), and harken's `Config` is a
`Serve` read under `HARKEN_` (it derefs to it) and its media and house.

*Measured* (`tools/loc.sh`, src / bins): harken-iced 8,155 / 1,651 →
7,343 / 1,679; harken-player 884 → 929; harken-server 4,596 / 1,295 →
4,440 / 1,201; ark-peer 2,392 → 2,656 (`watched.rs`, `settings.rs`,
`until`); ark-ffi 634 → 630; harken-ffi 288 → 267. Those six, source and
binaries, 19,898 → 19,148. Tests: harken-iced 1,678 → 1,686 (its in-file
reconciler tests, ~590 lines, were counted as source), ark-peer 1,759 →
1,792, harken-ffi 732 → 761, harken-server 6,755 → 6,738. New tests, each
falsified once: the hand-off (harken-ffi), `Watched` (`ark-peer/tests/
views.rs`: falsified by settling whoever is signed in), and harken-peer's
`harkens_types_cross_as_json` rewritten over the FFI peer (falsified by
sending a unit input its `{}`). `cargo test --workspace`: 447 passed, 30
ignored, none failed (the fleet's 23 and its witness, the versions' 6, the
bridge's 15 among them); clippy `--workspace --all-targets -D warnings`
and fmt clean; the wasm32 check of harken-iced passes, with and without
`demo` (harken-iced's tests with `demo`: 60 passed, 2 ignored — the
flake builds them so, which found the demo's tests still reaching for the
old row fields: `1f3cf4e`). After `rm -rf rust/target`, `nix build
--max-jobs 1` of `path:` refs: `checks.rust` 767 s (its dependency layer
already rebuilt for the new lock by a first run), `checks.harken-module`
35 s, `packages.harken-web` 689 s, all three passing.
Not verified: a real desktop audio device (there is none: the desktop
declares it cannot play, as before) and a real browser — the `<audio>`
adapter's `asked`/`take_stalled` (an autoplay refusal, a stall said as
`stalled`, the move on at a track's end while the output) is compiled for
wasm32 and run by nothing; the phones and an iPhone over the changed
ark-ffi views.
### Landed: T3

Six items, each its own commit (`9c4c36c`…). **ark-macros**: every derive
and attribute returns `syn::Result`, `?` in place of the nine early-return
matches, the error made `compile_error!` once in `lib.rs`; the expanded
code of harken-domain (with `ffi`), ark-dev, ark-auth, ark-peer, ark's macro
tests and ark-ffi's surface test is byte for byte what it was
(`-Zunpretty=expanded` before and after). **One language table**: `gen`'s
walk of types, reads, writes and stubs is `Lang` (`gen/mod.rs`), Kotlin and
Swift a table each, their records and sums their own; `Harken.kt` and
`Harken.swift` unchanged (harken-ffi's pin), `ark-dev gen` of demo and room
too. Lines went up, not down (2,212 → 2,240 src): the tables spell each
stub once per language, as the two copies did, and the walk they saved is
smaller than the table. **The bridge's patch-merge** is ark-peer's
`Update::patch`, which `Told::patched` calls. **An old data directory
opens**: `rust/ark-peer/tests/data/server/` is a server's data directory
written by this build — `log/kv`, `log/log`, `log/local.log`,
`session.key`, and a device's replica in it (`replica/log`, `local.kv`,
`local.log`; the hub's `local.kv` is not written by so small a run) — and
`tests/old_data.rs` opens a copy: the old token judged alice's, the hub's
head and state hash, seventy playlists, the replica's chain, origin and
nothing pending; falsified by a byte changed in each of `log/kv`, `log/log`
and `session.key`. `spec/README.md` § The data directory: a change to the
on-disk format comes with a migration that opens the fixtures, or a new
fixture beside them and a note; a fixture is never written over
(`ARK_WRITE_DATA=1` writes one). **Perf files to bounds**: the three
`perf.rs` (2,789 lines) go for `tests/costs.rs` in `ark`, `ark-peer` and
harken-domain (Part 3, Speed, says what each holds); each new bound
falsified once; one ignored `timings` per crate prints, in release: a peer
alone's add over 8,000 17.9 µs, a `Batch` of 256 decoded 165.1 µs, views at
16k 15.2 / 8.2 / 14.9 µs; a linked round trip 186.2 µs, an offline intent at
4,000 pending 93.4 µs; harken `add_song` growing to 8,000 31.6 µs.
`live_memory.rs` unchanged. **Trace replay** ran in five places: checks.rust
(the trace vectors, the fleets', the fuzz sessions'), the `vectors`
derivation (all of ark's and ark-dev's tests again), checks.traces (the
trace vectors again, and the corpus), the `trace-corpus` derivation (each
trace replayed as written) and `fuzz-smoke` (seeds 1–25 with `traces`,
which `fuzz.rs` replays). The three pure repeats went: the `vectors`
derivation runs only the writers (`hash`, `vectors`, `views`) and the trace
writer replays nothing while writing; `ark-fuzz … traces DIR` writes
without replaying; `fuzz-smoke` stays, without `traces`. checks.traces runs
`trace_corpus` only. Not removed, as not pure repeats: `fleet.rs`'s replay
of 41 fleets' clients (only seed 7's are vectors) and `fuzz.rs`'s of seeds
1–200 (the corpus is seeds 2026–2325).

*Measured* (`tools/loc.sh`): src `ark-macros` 730 → 687, `ark-dev` 2,212 →
2,240, `ark-ffi` 634 → 630, `ark-peer` 2,392 → 2,397; tests `ark` 3,928 →
3,205, `ark-peer` 1,759 → 1,274, harken-domain 4,003 → 3,723, `ark-dev` 975
→ 976. Tests 462 passed, 7 ignored (`cargo test --workspace`); clippy
`-D warnings` and fmt clean. nix `checks.{rust,vectors,traces,ffi}` with `--max-jobs 1` over `path:` after `rm -rf rust/target`: all four built (`traces`: 902 traces of the corpus replayed). Not verified: the release builds of
the phones; the fixture against a build other than this one (there is
none older to write it).

## Landed

**S4** (`21c7c42`, `e00541f`): **the interpreter leaves the kernel; a
views vector is a named view.** `dynamic.rs`'s expression language — `Expr`,
`Lam`, `Cmp`, `parse`, `evaluate`, `plan`, `make`, `ordered` — goes (705 →
183 lines), and with it `Db::get_raw`, `scan_raw` and `schema`, which only
it read. What stays is the JSON-of-rows path a vector, a host speaking JSON
and the explorer need (`row`, `row_bytes`, `key_bytes`, `Dynamic`) and
`Browse`: a table's rows whose columns equal given values, within a key
range, sorted by a column (ties in key order), the first `n` — values
compared and sorted as their columns' key bytes — built as a plan of the
same operators (`scan_raw`, `filter`, `sort`, `take`), so a view of it is
kept by patches, incrementally, for free; the explorer's grid reads through
it, afresh on each call as before. Anything more than a browse is a view
the domain names, which the explorer lists and calls already. The `views/`
vectors are `{batches, ctx, domain, input, rows_before, schema, steps,
store_before, view}`: a view of a reference domain in `ark-dev`
(`library.rs`: `library` — the demo's tables, people, works, movements,
songs, tracks, a view per v4 scenario — and `colors`, `pairs`, a fuzz
finding each), written by `ark-dev/tests/views.rs` and run from the file
alone (`vectors::run_view`, `spec/RUNNERS.md` § views); the falsified one
now fails a real runner. Fourteen vectors and one falsified, as before;
every step of every vector is the one the JSON plans gave. The work
contract moved with them (0 gets and 5 rows, 0, 0 at 1k, 4k, 16k); the
kernel keeps the rebase and scoped-feed view tests, over typed plans, and
a browse test. *harken-peer*: little was the kit's twice — the binding of a
bare name, a query's context and answer, an intent's standing, the cursor,
pending count, alone and linked accessors, the login wrappers — and those
now read through `Peer`; pumping, reconnect and backoff (`link.backoff`)
were the kit's already, `--ping-ms` stays a no-op, nothing retries a login,
and its sign-in path stays (it keeps a joined chain for the same session,
which `Peer::login` does not), so no knob was added. A bare name now means
the first binding ending so (`Domain::binding`), not a refusal when two do.
Source: `ark` 5,458 → 4,956, `ark-dev` 1,705 → 2,142 (the reference domains
and the runner, 400 lines, are now source), `ark-explorer` 2,254 → 2,251,
`harken-server` 4,517 and bins 1,352 → 1,304; the default `tools/loc.sh`
set 19,439 → 19,371; tests 444 passed (`ark` views 4 → 3, `ark-dev` +2;
each new one falsified once). Not verified: the explorer's grid by a
browse with a filter or a range (it sorts only; `Browse` is tested in the
kernel); a second implementation running the vectors.

**S3b** (`93d03a5`…): **one reconciler, two adapters.** What a phone did
in `Remote.kt` and the server did for a Home Assistant speaker in
`assistant.rs`'s `Bridge` — make a player agree with the listening session
and say what happened to it — is one crate, `harken/player`
(`harken-player`, over `harken-domain` and `ark-peer`). Its `Reconciler`
looks at a `View { me, joined, epoch, listening, now }` (the domain's
`host::Listening`) and drives a **`Sound`**: commands `load(queue, at, ms,
playing, from)`, `jump`, `resume`, `pause`, `seek`, `let_go(follows)`,
`wake_in`, `set_volume`, `join`/`unjoin`; readings `heard() -> Heard {
queue (ids), at, playing (asked to), ready (has the hand-off), place:
Unknown | Believed(ms) | Measured{ms, at}, ended, from }`, `take() ->
Event { Stalled{at_ms} | Resumed | Lost{why} }`, `tracks`, `caps`,
`volume`, `followers`. It answers through a `Say` (`call(transition,
input, as_of, why)`, `play(RecordPlay)`). One code path, in order: `equip`
once a connection where the entry differs from what the player can do; the
rule for coming back; volume (read against what the player read after its
own last write) and group, both ways; then output — load the session's whole
queue, jump where the session moved, a move of the player's own said as
`advance`/`jump` and waited for, `started` once ready, the queue's end once,
a stall/resume said and not fought, play/pause, a seek where the session
moved, a drift said (`report_position`, past `DRIFT_MS` its own `seek`) —
or silent, let go once (no pause for a member of the new leader's group),
or released and not taken back; plays counted once per load. Its **memory is
rows**: `Mind` (volatile, an `Instance::ephemeral::<Minds>()`), each column a
sum — `Alone{Quiet|Playing}`, `Role{Silent|Output|Released}`,
`Handoff{None|Started{here}|Advanced{here}}`, `Expect{Nowhere|At}`,
`Against{Nothing|Clock{here,clock}}`, `Waiting{Nothing|On{clock}}`,
`Equipped{Not|Said{epoch,can_volume,family}}`,
`Tuned{Untold|Told{wanted}|Settled{wanted,seen}}`, `Grouped{Untold|Told}` —
and `Count { loaded: Nothing | Track{here, counted} }`, durable
(`Instance::open::<Counts>`, the phone's under its replica's directory).
`Player` drives one over a device's own peer: its calls, its plays in the
log, a `Press` (the press → transition mapping, moved from Kotlin) and one
view, `{me, joined, linked, epoch, listening, active, stats, refused}`,
whose `active` is the session's while joined and the player's own alone.
*Adapter 1*, `harken-server`'s `Speakers`: Home Assistant's polls read into
`Heard`/`Event` (the hand-off fetched and its dropped seek said again, the
blink, the stale date, the lapse, the URL spelt back, the speaker's own
queue), commands as service calls (`Act`), which account holds a speaker;
the `Bridge`'s decisions and its `Seen`/`released`-per-rule memory go, and
all 14 HTTP tests (`tests/bridge.rs`) pass unchanged. *Adapter 2*, the
phone: `harken/ffi`'s `Player` (UniFFI object) over a callback interface
`PhoneSound` (UniFFI's `with_foreign`, as `Listener` already is), which
`ExoSound` implements; `Remote.kt` and `Volumes` go, `Listen.kt` steps the
Rust player, `Playback.kt`'s state layer draws `Heard.active` in both modes
(its alone `read()` path goes; shuffle alone is the queue reordered, not a
mode), and `Session.kt` keeps what the screens read (`Listening`, `Press`,
`Picker`). iOS has the interface and a silent stand-in (`Player.swift`).
Not done: subscribing with `peer.reconcile` — a speaker's machines are
kernel machines on the hub, not `ark_peer::Peer`s, and the session is three
volatile tables read whole, so both drivers call the reconciler on every
change instead. Source: `harken-player` 884 (+405 tests), `harken-server`
4,787 → 4,517 (its in-file tests 1,095 → 887 lines; the shipped adapter
1,936 → ~1,875: the speaker's reading is most of what the bridge was),
`harken/ffi` 123 → 274 (+523 tests), domain, `ark-peer`, `ark-ffi`
unchanged; hand-written Kotlin main 12,269 → 11,800 (`Remote.kt`'s 757 gone, `Session.kt` 319 in), tests
5,699 → 5,265; so the default Rust grows by 765 and Kotlin falls by 903,
for one reconciler where there were two (three with the desktop's, which
is `harken-iced`'s and untouched). Tests: Rust 445 passed (`RemoteTests`'s
ten in `harken/ffi/tests/player.rs`, one more for a step told of its own
change; the bridge's session rules as eight in `harken/player/tests`; its
reading of the house as nineteen beside it), each new one falsified once;
the app's JVM tests 99 (one skipped), run over a host-only library. Not
verified: a phone or an iPhone running it (`Player.swift` not
type-checked here), a real Home Assistant and Sonos.

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
