# Spec v4: every query is a plan, and Rust is the spec

The design for the fourth revision of ArkDB, and the plan for landing it.
It follows from four decisions taken together, each recorded here so that
the reasoning survives the diff:

1. **Every query is a plan, maintained incrementally.** A query is no
   longer a procedure that loops over lists; it is a tree of reads —
   source, filter, joins, lookups, grouping, projection, having, order,
   limit — and the view engine keeps every one of them up to date at the
   cost of the rows that moved. Incremental all the way down, the way
   Rocicorp Zero's ZQL is, rather than incremental for a single `select`
   and a re-run for everything else.
2. **Rust is the specification.** The Haskell program under `spec/src`
   said the same things as `rust/ark` a second time, and every change was
   made twice. `rust/ark` is normative now; it writes the vectors, and the
   vectors are what any other runtime is held to.
3. **Swift and Kotlin are frozen at spec v3.** Their runtimes, their
   authoring libraries and the two phone apps stay in the tree exactly as
   they are, stop following harken's domain, and leave `nix flake check`.
   They come back when the spec settles, against the vectors of that day.
4. **The printers are dropped for now.** `arkc gen` and `arkc roundtrip`
   existed to hand a phone a domain in its own language; with the phones
   frozen there is nobody to print for, and a decompiler is the largest
   thing the spec change would otherwise have to carry.

Part 1 is the design. Part 2 is the work, split into the pieces that are
handed to implementation agents, with what each owns and what it is held
to. `docs/arkdb.md` remains the architecture; where this document and it
disagree, this one is newer, and Part 2 lists the sections of it to revise.

---

## Part 1 — Design

### 1.1 What is wrong today, measured

A query in spec v3 is a procedure: a block of `let`s over `select`s, and
then `map`, `filter`, `sort_by` and `fold` over the lists it read. That
made queries expressive enough to write harken's read model in the
vocabulary — and made every one of them cost the library on every change:

- `ark_client::View` maintains a query only when its body is one `select`
  with no middleware (`plan_of` in `rust/ark-client/src/view.rs`); every
  other query is `How::Rerun` — read again on every change, diffed into
  patches. Correct, and O(library).
- harken's `library` is two selects and a map, so the client could not
  maintain it. `harken/domain/src/view.rs` reads the emitted query's
  `Select` statements back out of its IR and rebuilds a plan by hand
  (`library_plan`), and `harken/iced/src/peer.rs` holds a private
  `Maintained` over it, with a rollback-per-change scheme for batches. It
  works, and it is a second definition of what `library` means.
- `composers`, `albums`, `artists`, `track_details` are nested loops:
  `composers` walks works × movements × songs per person and costs 57 ms
  on the demo library (`harken/iced/src/tests/bench.rs`), and the desktop
  re-reads the sidebar's four queries after every change that is not a
  playlist toggle.
- The evaluator has two halves for these: `Native` runs the loops as
  closures, `Emit` records them, and every query is tested for agreement.
  There is nothing to disagree about once a query has no body.

The indexes (`4bacc8f`) and the arena (`a0f5a81`) made each read cheap;
they cannot make a re-read free. What can is a query whose whole shape is
data the engine understands, so that a change is routed to the parts of
the answer it touches.

### 1.2 The principle

**Every read is a node of a plan; no expression reads.** A plan says what
tables it reads and how they join, and an expression — in a filter's
right-hand side, a projection, a having, an order key — only computes over
values the plan has already bound. That split is what makes maintenance
possible: the engine knows every dependency of every node from the plan
alone, and an expression is a pure function that is re-run when its
inputs change.

Mutators are unchanged. A mutator is a procedure, because a write is a
sequence of decisions and the log carries intents; its reads stay v3
plans (`select` with a constant filter, order by columns, limit, and
reference-derived `with`). Helpers are unchanged and are what a projection
calls. Guards and provides are unchanged as procedures; §1.7 says what
they mean for a maintained query.

### 1.3 The plan (IR §3.3, revised)

```
Plan {
    source:  Source,               // the rows this node is over
    filter:  Option<Pred>,         // over the source table's columns; RHS constant
    row:     Sym,                  // the binder for the source row (or the group key)
    members: Option<Sym>,          // group source only: the group's rows, as a list
    lookups: Vec<Lookup>,          // a row by key from another table, in order
    related: Vec<Related>,         // child plans, one list per parent node
    having:  Option<Expr>,         // keep the node when true; over every binder
    project: Option<Expr>,         // the node's value; over every binder
    order:   Vec<(Key, Dir)>,      // Key::Column(name) | Key::Expr(expr)
    limit:   Option<i64>,
}

Source  = Table(TableName)
        | Group { table: TableName, by: Vec<FieldName> }

Lookup  { name: FieldName, sym: Sym, table: TableName, key: Vec<Expr> }
Related { name: FieldName, sym: Sym, on: Vec<(FieldName, Expr)>, plan: Plan }
Key     = Column(FieldName) | Expr(Expr)
```

What each part means, in evaluation order per node:

- **Source.** `Table(t)`: one node per row of `t` the filter admits. `Group
  { table, by }`: the rows of `table` the filter admits, grouped by the
  values of the `by` columns; one node per distinct tuple. `row` is bound
  to the row, or for a group to a struct of the `by` columns; `members`
  (group only) to the group's rows as a list, in the table's key order.
- **Filter.** As v3: a `Pred` over the source table's columns whose
  right-hand sides are constant for the query — literals, `Arg`,
  `CtxUser`, `CtxSession`, `Provided`, helper calls over those. The
  verifier keeps it free of the row. It is what the store's indexes serve
  (`equalities`).
- **Lookups.** In order, each binds `sym` to `Opt<Row of table>`: the row
  under the key the expressions compute, over `row`, `members` and the
  lookups before it. Any key part `Null` is `None`. This is how a
  reference is followed *upward* — a song's media, a movement's work, a
  work through a movement — and it may chain.
- **Related.** Each binds `sym` to `List<Node of plan>`: the child plan
  evaluated with its own filter *and* `child.col == expr(parent)` for every
  pair in `on`, the expressions over the parent's `row`, `members` and
  lookups. The child's order and limit are per parent. A reference
  declared in the schema is one case of this — `on = [(fk, row.key)]` —
  and is what `with(Table::rel)` still writes; the general form joins on
  any column equality, which is what lets `credit` hang off a `song` by
  `recording_id` without a `recording` between them.
- **Having.** A `Bool` over every binder. A node it refuses is not in the
  answer; the node still exists to the engine (§1.5), which is what lets
  it appear when a child arrives.
- **Project.** The node's value, over every binder; any type, and a
  query's result type is `List` of it. Absent, the node is the v3 shape:
  the row's columns plus one field per related list, named by `name`.
  Expressions here may call helpers and use every list function over the
  bound lists — `map`, `filter`, `any`, `all`, `sort_by`, `fold`, `first`,
  `len` — because those are pure over values the plan already read.
- **Order.** Keys in order, each a column of the source row or an
  expression over the binders, under `compare_value`; the verifier appends
  the key columns ascending to make it total, as v3 does. `Dir` per key.
- **Limit.** A window over the ordered, having-admitted nodes.

**Expressions inside a plan** — lookup keys, `on`, having, project, order
keys — may use `Var` of a binder in scope, `Arg`, `CtxUser`, `CtxSession`,
`Provided`, literals, operators, `Std`, `Call` of a helper, the list
functions, `Match`/`If`/`Some`/`None`/`Struct`/`List`/`Field`. They may
**not** contain `Select`, `Get` or `Exists`: a read is a lookup or a
related plan, never an expression. `Auto` is refused (a query draws none).

**Scope.** A child plan's expressions see the child's own binders only;
the parent is reached through `on`. A having or projection sees its own
node's binders (`row`, `members`, its lookups, its related lists) and
nothing above. Keeping scope flat is what keeps a node's dependencies a
function of its own subtree.

**Options are flat** in the value model (`Some(v)` is `v`, `None` is
`Null`), so a join or a lookup key across an `Opt<T>` column is plain value
equality: `recording.work_id == work.id` matches when the recording has a
work. The verifier types `on` pairs as `T == T` or `Opt<T> == T` either
way round.

### 1.4 A query is a plan, and nothing else

`Function` gains `plan: Option<Plan>`. For `FnKind::Query`, `plan` is
`Some` and `body` is empty; for every other kind `plan` is `None`. The
input schema, its checks, `refine`, the router and `uses` are as v3. `ret`
is `Some(List(node type))`.

Consequences, each deliberate:

- **One evaluator.** `view::pull(plan, env, store)` is what a query means:
  `eval::query`, `Procedure::query`, a mutator body's `Select`, and a
  hydrating view all call it. There is no `Native` half for a query, so
  `agrees_on_query` goes, and the arena's list machinery is used by
  mutators and helpers alone.
- **The vocabulary keeps its shape.** `db.t.filter(..).order_by(..)
  .limit(..)` and `.with(..)` are unchanged; a query gains `.each(..)`,
  `.get(..)`, `.group_by(..)`, `.having(..)`, `.sort_by(..)` and `.map(..)`
  (§1.9). A mutator's reads may not use the new ones; the verifier refuses
  a plan inside a body that has a group source, lookups, having, a
  projection, an expression order key, or a related `on` that is not a
  schema reference. The builder reports it at `build()`.
- **A mutator's hash does not move.** The wire form of a plan writes its
  new keys only when present (§1.8), so a v3-shaped plan inside a mutator
  encodes byte-for-byte as before, and every closure a retained entry
  names hashes the same. Queries are not in the log; their encoding is
  free to change, and does.
- **Middleware still runs.** A query's guards and provides run before
  hydrate over the same store, with the input; `Provided(name)` is a
  binder-free value in the plan's expressions. §1.7 is what happens when
  the tables they read change.

### 1.5 Maintenance (§13, revised)

A `View` is a plan, its environment (arguments, context, provided values),
and a list of **entries**, one per candidate: a source row the filter
admits (or a non-empty group). Each entry keeps

- its key (the row's key, or the group's `by` tuple),
- the values its subtree depended on: for every `Lookup` node in the plan
  tree, the key it looked up; for every `Related` node, the join value(s)
  its `on` computed — recorded at every depth as the subtree was pulled,
- its order keys, whether `having` admitted it, and its node value.

Entries are ordered by their order keys; the **answer** is the admitted
entries in that order, cut to the limit. Two indexes over the entries make
a change cheap to route: by key, and by `(plan node id, dependency value)`.

`push_all(changes)` takes the changes of one settle — the store is already
at the state after all of them — and rebuilds the entries they touch, once
each, against that store:

1. **A change in the source table** names a key. If the source is a table:
   the row now under that key (from the store) decides — absent or not
   admitted and there is an entry: remove it; admitted and no entry: build
   one; admitted and an entry: rebuild it. If the source is a group: the
   group keys of the old row and the new row are each rebuilt from the
   store's members (a group whose members are gone is removed).
2. **A change in any table that a `Lookup` or `Related` node reads**, at
   any depth: for each such node whose table is the changed one, the
   dependency value the old row and the new row would satisfy — the row's
   key for a lookup, the value of the join column for a related — selects
   the entries whose recorded dependencies hold it; each is rebuilt. A
   match is enough to rebuild; the rebuild decides whether the row was
   really in the subtree (a child that matched the join value but not the
   child's filter costs a rebuild and changes nothing). A table may be
   both source and dependency; both rules run.
3. **Rebuilding an entry** recomputes its lookups, related lists, having,
   projection and order keys from the store and records the new
   dependencies. Against the old entry: newly admitted → `Insert`; no
   longer admitted → `Remove`; order keys moved → `Remove` then `Insert`
   at the new place; the node changed → `Update`; identical → nothing.
4. **Under a limit** the answer is a window over the ordered admitted
   entries. An entry leaving the window lets the next one in, an entry
   entering pushes the last one out, and both are reported as patches
   against the list the client holds. The candidates beyond the window
   are known (they are entries), so a refill is a step along the entries
   and not a read of the store. An implementation may drop the node value
   of an entry outside the window and rebuild it when it enters.

Why this is sound: the dependencies recorded on an entry are exactly the
values the pull joined or looked up, so any row that would now join has a
join value that some entry recorded — at depth one because every admitted
source row is an entry, having or not; at depth *n* because its parent at
depth *n−1* was pulled by an entry that recorded that join value. A row no
entry depends on cannot change any node's answer.

Why this is cheaper than v3's case analysis: v3's `push_top` decided each
case from the change alone and needed the store *as it stood after that
one change*, which is why the batch path rolled the store back change by
change. Reconciling against the final store makes a batch of *n* changes
touching *k* entries cost *k* rebuilds, and removes the rollback.

Patches are unchanged: `Insert { at, node }`, `Remove { at }`, `Update {
at, node }`, positions into the list as it stands when the patch is
applied. `splice` is unchanged.

**Cost, stated as the contract it is:** a change costs the entries whose
recorded dependencies it hits, each rebuilt from the store through the
indexes, plus the patch bookkeeping. A change nothing depends on costs
two index probes. Hydrate costs what `pull` costs, which is what v3's
`select` cost. A group source rebuilds a whole group per member change.
`nix run .#latency` and the two benchmarks measure it; a vector cannot.

**The correctness contract is unchanged**: after any sequence of changes,
the view's answer equals a fresh hydrate over the same store, and splicing
the patches into the previous answer gives the new one. `contract` is
kept, and every query harken and the demo declare is run under it against
randomized change sequences (§2, B1b).

### 1.6 `Rebuilt`

A rebase rolls the optimistic store back and replays pending on top. No
sequence of changes describes that, so the peer reports `Changes::Rebuilt`
and a view re-hydrates; `ark_client::Update::Reset` says so to the screen.
Unchanged from v3, and the engine still never synthesises changes for a
rollback.

### 1.7 Middleware over a maintained query

A guard or a provide is a procedure that reads the store — `owned` reads
the playlist row to decide it is yours and to hand it over. It runs at
hydrate. Afterwards the view keeps the set of tables its middleware read
(the tables of the `Select`/`Get`/`Exists` in their bodies, by static
inspection), and `ark_client::View::update` re-runs the middleware when a
change touches one of them before pushing anything. If the outcome is the
same — the same provided values, no refusal — the changes are pushed as
usual; if it differs, the view re-hydrates (or becomes empty on a refusal)
and reports `Update::Reset`. This is rare — a playlist renamed under an
open page — and correct, and it keeps the engine's view unaware of
middleware: `ark::view` sees a plan and an environment.

### 1.8 Wire form (§7)

A plan encodes as a `plan` node with the v3 keys — `table`, `filter`,
`order`, `limit`, `related` — plus, **only when present**: `group` (the
`by` columns; `table` is then the grouped table), `row`, `members`,
`lookups`, `having`, `project`. An order key that is an expression is `{
"expr": …, "dir": … }` beside the column form `{ "column": …, "dir": … }`.
A related encodes `name`, `sym`, `on` (a list of `[column, expr]`) and
`plan`; the v3 form (`parent`, `child`, `column`) is *not* kept: a related
is always the `on` form now, and since no v3 mutator in harken or the demo
uses `with`, no retained hash moves. If one did, it would be a spec break
to record — the check is `arkc check` over the old and new module, run
once in B1a and stated in the commit.

A `Function` writes `plan` when it has one. `SPEC_VERSION` is 4; a module
at another version is refused by `decode`, as before. `normalize` renumbers
the plan's binders with the function's other symbols.

`row`'s symbol is required even where nothing references it, so that the
binder numbering of a plan is a function of the plan and not of its use.

### 1.9 The vocabulary (`spec/AUTHORING.md` §2.3, revised)

The Rust builder, which is now the only one. A query body is run under
`Emit` alone and returns a `Query`; there is no Native path to agree with.
Everything a v3 query could say that was one `select` is unchanged.

| plan | Rust |
|---|---|
| source table | `db.song` — a `Query<Song, ()>` |
| filter | `.filter(Song::album_name.eq(some(input.name)))` as before |
| group source | `db.media.group_by(Media::creator)` — a `Query<Grouped<(Text,)>, (List<Media>,)>`; the key struct's fields are the columns, the one binder is `members` |
| lookup | `.get(|song, ()| db.media.by((song.media_id,)))` — appends `Opt<Media>` to the binders |
| related, by reference | `.with(Media::playlist_item)` as before, appends `List<PlaylistItem>` |
| related, general | `.each(|song, (media,)| db.credit.filter(..).order_by(..).on(Credit::recording_id.eq(song.recording_id)))` — appends `List<Node>`; the closure returns a `Query` whose own `each`/`get`/`map` nest |
| having | `.having(|album, (songs,)| songs.len().gt(0))` |
| order by column | `.order_by(Media::pos.asc())` as before |
| order by expression | `.sort_by(|song, (media, movement)| movement.map_or("", |m| m.part))` — appended after the column keys, ascending; `.sort_by_desc` |
| projection | `.map(|song, (media, items)| LibraryEntry { .. })` — the node type; the query's value |
| limit | `.limit(n)` as before |
| the result | the `Query` itself is the body's value (`Data`, typed `List<Node>`); `.first()` is `limit(1)` and the client takes the head |

Binders accumulate in a tuple type as `provide` does on a router: `Query<R,
(A, B, C)>`, and a closure takes `(row, (a, b, c))`. Up to six; more is a
nested plan. Inside a closure `row` and each binder are ordinary
vocabulary values, so every list function and every helper is available,
and a `Select` there is a `build()` error (the builder is in a plan
context and `db.t.…` returns a `Query` for `each`/`get` only; `.all()` and
`.first()` inside a projection panic with the rule).

`db.t.by(key)` is `get` spelt for a lookup, so that a plan's lookup and a
mutator's `db.t.get(key)` read differently: one is a node, the other a
bound read.

**Stable sort semantics.** `sort_by` appends a key; keys are compared in
the order given, columns first as written, then expressions as written,
then the key columns. This is *not* v3's list `sort_by`, where the last
call was the primary key; harken's queries are rewritten accordingly
(§2, B1a), and the doc comment on `sort_by` says so.

### 1.10 The verifier (§9)

Adds, for a query: `plan` present, body empty, `ret = List(node ty)`;
typing of every plan node under the binders in scope; `having: Bool`; lookup
key arity and types against the table's key; `on` column types; order
keys typed and made total; expressions free of `Select`/`Get`/`Exists`/
`Auto`; a group's `by` columns exist. For every other kind: `plan` absent,
and every `Select` in the body a v3-shaped plan (§1.4). `Var` of a symbol
not bound in the node's scope is `UnboundSymbol`, as anywhere.

### 1.11 What leaves

- `Ark.*` under `spec/src`, `spec/app` (`Arkc.hs`, `Vectors.hs`),
  `spec/ark-spec.cabal`, the whole of `spec/generated` and the `spec`
  devshell. `spec/generated/unicode` held three tables: the Rust one is
  `rust/ark/src/unicode_tables.rs`, and the Swift and Kotlin ones were
  already copied, byte for byte, into the frozen trees
  (`swift/Sources/ArkDB/UnicodeTables.swift`,
  `kotlin/ark-runtime/src/main/kotlin/dev/arkdb/std/UnicodeTables.kt`),
  which read nothing from `spec/`. `spec/tools/GenUnicode.hs` was ported to
  `rust/ark/src/bin/gen-unicode.rs` and deleted with `spec/tools`.
- `arkc print`, `arkc gen`, `arkc roundtrip`; `Ark.Print`, `Ark.Gen`.
- `checks.swift`, `checks.kotlin`, `packages.arkdb-swift`,
  `packages.arkdb-kotlin`, `packages.kotlin-deps`, the `swift` and `kotlin`
  devshells, `clientFunctions`, and the roundtrip lines of
  `checks.harken-domain`. `harken-apk` **stays**: it builds the frozen
  Kotlin app over the frozen print and reads no vectors.
- `ark_client::view::How::Rerun` and `diff`; `harken/domain/src/view.rs`'s
  `library_plan`, `entry_of` and `patch`; `harken/iced/src/peer.rs`'s
  `Maintained`.
- `Procedure::agrees_on_query`, and the Native path of query bodies.

### 1.12 What stays

- The log, entries, closures, function hashes, the authority, the
  protocol, live rooms, sign-in, the overlay, the indexes, the store trait,
  `Native`/`Emit` for mutators, guards, provides and helpers. Every mutator
  in harken and the demo is untouched.
- `spec/vectors/` as the conformance suite — now written by
  `rust/ark`'s generator and read by `rust/ark/tests/vectors.rs`, two
  programs against one set of files, with a `falsify/` case per directory
  as before.
- `spec/AUTHORING.md` as the contract, rewritten for one spelling and the
  plan vocabulary; `spec/README.md` as the index, pointing at the modules
  of `rust/ark/src` by section.
- `swift/`, `kotlin/`, `harken/ios`, `harken/android`,
  `harken/domain/gen/{swift,kotlin}` — byte-for-byte, with a `FROZEN.md`
  in `swift/` and `kotlin/` naming spec v3 and the commit they were last
  green at.

### 1.13 Risks, stated as risks

- **Group rebuilds are whole-group.** `artists` regroups one creator's
  media per media change; a creator with a thousand tracks costs a
  thousand-row rebuild per tap on any of them. Acceptable for a library;
  a maintained aggregate (a count kept as a number, moved by ±1) is the
  next step if it is not, and the plan shape leaves room for it.
- **Expression order keys under a limit are exact but not cheap to
  hydrate**: every candidate's keys are computed at hydrate. That is what
  v3's `select` did too (it sorted every admitted row), so no regression;
  it is worth saying that a `limit` does not make a hydrate cheaper than
  the table.
- **Entries hold the candidates.** A view over a large table keeps a key,
  a few values and (inside the window) a node per admitted row. This is
  the price of having-driven appearance and of memory refills, and it is
  bounded by the store's own size.
- **The phones fall behind.** Frozen at v3 they run yesterday's harken
  domain against a server that will move. They are not shipped to anyone;
  when Swift and Kotlin return they return to the vectors as they stand
  then.

---

## Part 2 — The work

All code is written by implementation agents; the coordinator writes
this document, reviews, runs `nix flake check`, commits others' work
where noted, and pushes. Each piece names what it owns; nobody edits
outside their paths without asking. The order is forced by APIs:

```
B1a  ──►  B1b  ──►  B2
   └──►  B1c  ──►  B3
```

B1a lands first, on its own, and leaves the workspace green. B1b and B1c
run in parallel on top of it. B2 follows B1b; B3 follows B1c (it deletes
what B1c ports) and lands last, with the flake.

### Rules every agent keeps

- **Git**: commit your own paths only — `git add -- <paths>` then
  `git commit -m "…" -- <paths>` — and never push, reset, checkout, stash,
  rebase or clean; other agents' uncommitted work shares the tree. Retry
  on `index.lock`. Every commit message ends with exactly:

      Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
      Claude-Session: https://claude.ai/code/session_014Nz5smRBnkQqoe6xuVGUvk

  No model names anywhere else, and no personal names or emails anywhere.
- **Nix for everything**: `export PATH=/nix/var/nix/profiles/default/bin:$PATH
  CARGO_INCREMENTAL=0`, then from `/home/user/apps`
  `nix develop .#rust -c bash -c 'cd rust && cargo test -p ark'` and the
  like. `rust/target` is shared; no per-agent target directories. The disk
  is a fixed allowance: when it fills, delete stale test binaries under
  `rust/target/debug/deps` (keep the newest per name) and say so, rather
  than deleting others' files.
- **Falsify every new test once** by breaking what it checks, and say in
  the test's doc comment what broke it.
- **Say what is not verified.**
- **No Rust macros in a domain**, no SQL, no wasm.
- When this document does not answer a question, ask the coordinator with
  a concrete proposal; the answer is added here.

### B1a — IR v4, verifier, the one evaluator, the builder, harken's queries

Owns: `rust/ark/src/{ir/**, verify.rs, eval.rs, plan.rs, hash.rs,
authoring/**, lib.rs}`, `rust/ark/src/view.rs` **`pull` and the types only**
(`View`'s incremental half is B1b's; leave `push`/`contract` compiling
against the new types, correct for v3-shaped plans, and marked for B1b),
`rust/ark-client/src/demo.rs`, `harken/domain/src/**`,
`spec/AUTHORING.md` §1–§3 and §6 (the IR, the vocabulary, the lowerings —
rewrite for v4 and one spelling), and the smallest edits elsewhere that
keep the workspace compiling (`rust/ark-client/src/view.rs` may fall back
to re-running every query until B1b; `harken/iced/src/peer.rs`'s
`Maintained` may read `Function.plan` instead of the body until B2).

Delivers:

1. `ir::Plan` as §1.3, `Function::plan`, `SPEC_VERSION = 4`; `encode`,
   `decode`, `normalize` as §1.8, with tests that a v3-shaped plan encodes
   byte-identically (pin the bytes of the demo's `add_to_playlist` closure
   hash before and after).
2. `verify` as §1.10, with a complaint per rule and a test per complaint.
3. `view::pull(sch, plan, env, store) -> Vec<Entry>` where `Entry` carries
   what §1.5 names (key, dependencies by plan node id, order keys,
   admitted, node); `eval::select_plan`, `Expr::Select`, `eval::query` and
   `Procedure::query` all evaluate through it. Expression evaluation
   inside a plan reuses `eval`'s expression evaluator with a binder
   environment; no second evaluator.
4. The builder as §1.9 — `Query<R, B>` with `each`, `get`/`by`, `group_by`,
   `having`, `sort_by`, `map`, up to six binders — Emit-only for queries;
   `build()` errors for a plan feature inside a mutator body and for a
   read inside a projection.
5. The demo domain's queries as plans; `agrees_on_query` and the Native
   query path removed.
6. **Every harken query as a plan**, in `harken/domain/src/{library,
   playlists}.rs`, same names, same inputs, same result rows in the same
   order as today — held by the existing domain tests, which must pass
   unchanged except where a comment explains a deliberate difference.
   The shapes, as a starting point (the agent may find better ones):

   | query | plan |
   |---|---|
   | `library(playlist_id)` | media, order `pos`; related `playlist_item` on `media_id`, filter `playlist_id = arg`; map `LibraryEntry` with `playlist_pos = items.first().map(pos)` |
   | `albums` | album; related song on `album_name = name`, with lookup media by `media_id` beneath; having `songs.len() > 0`; map (`creator` from the first song's media) |
   | `artists` | media grouped by `creator`, order by the key; lookup person by `(creator,)`; map (`tracks = members.len()`) |
   | `track_details` | song; lookup movement by `movement_id`, lookup work by `movement.work_id`, lookup recording by `recording_id`; related credit on `recording_id`; map with `performers(credits, recording_id)` |
   | `album(playlist_id, name)` | song filter `album_name = name`; lookup media, lookup movement; related `playlist_item` on `media_id`, filter `playlist_id`; sort_by part, `track == 0`, track, title; map `LibraryEntry` |
   | `artist(playlist_id, name)` | media filter `creator`, order `pos`; related `playlist_item`; map |
   | `composers` | person; related work on `composer = name`, beneath it related movement on `work_id`, beneath that related song on `movement_id`; having `works.len() > 0`; map with the counts folded from the nested lists |
   | `works(composer)`, `work(id)` | work filter, order catalogue, title; related recording on `work_id`; related movement on `work_id` with related song beneath; map `WorkSummary` |
   | `recordings(work_id)` | recording filter; related song on `recording_id`; related credit on `recording_id`; sort_by `-songs.len()`, recorded; map |
   | `credits(recording_id)` | credit filter, order pos, person_name; related credit (sibling) on `recording_id`, filter `pos > 0`; having `pos > 0 or real.is_empty()`; map |
   | `recording(playlist_id, id)` | song filter `recording_id`; lookups media, movement; related `playlist_item`; sort_by `movement_id.is_none()`, `movement.no`, title; map |
   | `playlists` | playlist filter `user_id = ctx.user`, order pos |
   | `playlists_of(media_id)` | playlist filter `user_id`, order pos; related `playlist_item` on `playlist_id`, filter `media_id = arg`; having non-empty; map to the row |
   | `playlist(playlist_id)` (under `owned`) | `playlist_item` filter `playlist_id = provided.id`, order pos; lookup media; having `media.is_some()`; map `LibraryEntry` with `playlist_pos = pos` |

   Helpers `library_entry`, `credited`, `joined`, `performers`,
   `tracks_on`, `work_summary` are kept or reshaped as the projections
   need; they are pure and unchanged in kind.
7. `harken/domain/harken.ark` regenerated; the `arkc check` of §1.8 run
   against the old module (`nix run .#arkc -- check old new` still exists
   at this point) and its outcome stated in the commit message.
8. `cargo test` green across the workspace, clippy clean, formatted; the
   two benchmarks still run (`--ignored`).

Not B1a's: the incremental `push_all` (B1b), the vectors (B1c), the iced
client (B2), the flake (B3).

### B1b — the view engine, and `ark_client::View`

Owns: `rust/ark/src/view.rs` (all of it after B1a), `rust/ark/tests/`
files it adds, `rust/ark-client/src/view.rs`, `rust/ark-client/src/peer.rs`
(the `view` method and what it needs), `rust/ark-client/README.md`.

Delivers:

1. `View { plan, env, entries, by_key, by_dep }` and `push_all(sch, store,
   changes, &mut View) -> Vec<Patch>` as §1.5, including group sources,
   lookups, related at any depth, having, expression order keys and the
   limit window. `hydrate`, `rebuild`, `rows`, `splice`, `contract` kept
   with their meanings.
2. The contract held on every query of the demo and of harken, under a
   seeded generator of change sequences (adds, edits, removes, across all
   the tables the plan reads, including rows that join and rows that do
   not), asserting after every step that the answer equals a fresh
   hydrate and that the patches splice the old list into the new. One
   test per query, each falsified once (e.g. by skipping the dependency
   index for one node kind).
3. `ark_client::View`: always incremental; `Peer::view(name, args)` runs
   the middleware, hydrates, and keeps the middleware's tables (§1.7);
   `update` re-runs middleware on a change to those tables and resets on
   a difference; `Update::{Unchanged, Patched, Reset}` unchanged. `How`,
   `plan_of`, `diff` removed. Tests for the middleware reset (rename the
   playlist under an open `playlist` view; sign out under `playlists`).
4. `rust/ark-client/README.md`'s views paragraph rewritten.

### B1c — vectors, compat, `arkc`, the spec index

Owns: `rust/ark/src/bin/**`, `rust/ark/src/compat.rs`, `rust/ark/tests/
vectors.rs`, `spec/vectors/**`, `spec/README.md`, `rust/ark/Cargo.toml`
(binaries, `serde_json` for the generator), `spec/tools/**`.

Delivers:

1. `ark-vectors` (a binary in `rust/ark`): every case `spec/app/Vectors.hs`
   writes — codec, order, hash, module, verify, protocol, eval, views,
   rebase (`three-peers`, `fleet-seed-N`) — in the same JSON dialect
   (`$bytes`, `$id`, `$int`), regenerated at spec v4, each directory with
   its `falsify/` case. Byte-for-byte agreement with the Haskell output
   is expected for every directory whose content the spec change does not
   touch (codec, order, hash, protocol, rebase); differences elsewhere
   (module, verify, eval, views) are the v4 encodings and are listed in
   the commit message.
2. New `views/` vectors, after B1b lands: a projection, a having that
   admits a node when a child arrives, a group source, a lookup chain, a
   related on a non-key column, an expression order key under a limit, a
   depth-three related tree — each as plan, rows, changes, expected
   patches and rows.
3. `rust/ark/tests/vectors.rs` reads them all, and the falsify cases fail.
4. `compat.rs`: `Break`, `check`, `check_retained`, `is_additive` ported
   from `Ark.Compat` with its tests (one per `Break`).
5. `arkc` (a binary in `rust/ark`): `verify M`, `hash M`, `check OLD NEW`,
   `vectors OUT`. No `print`, `gen`, `roundtrip`.
6. `GenUnicode.hs` ported to `rust/ark/src/bin/gen-unicode.rs` if it
   stays small (§1.11), regenerating `unicode_tables.rs` identically;
   otherwise left with a note.
7. `spec/README.md` rewritten as the index: one row per section naming the
   `rust/ark/src` module that is normative for it, the requirements that
   are not functions (unchanged), the vectors table (with the new
   directories), building (`nix run .#vectors`, `nix run .#arkc`), and
   "Changing the spec" for a Rust module.

### B2 — the iced client on maintained views

Owns: `harken/iced/**`, `harken/domain/src/view.rs` (delete the plan hack;
keep `Item`), `harken/server/**` if anything there moved.

Delivers:

1. Every list the desktop and browser client shows is an
   `ark_client::View`: the library, the sidebar's playlists, albums,
   artists, composers, the page's album / artist / works / recordings /
   credits / recording / playlist. `Maintained` and `library_plan` gone;
   `refresh` hands `take_changes()` to every open view and splices what
   comes back; `reload_sidebar`/`reload_shown` reduced to opening views
   when the selection changes.
2. `harken/iced/src/tests/bench.rs` extended: the cost of one change to a
   view over the demo library, per query, printed; flat in the library
   size is the claim.
3. The client's tests green, including the browser (`wasm32`) build.

### B3 — retirement, the flake, the documents

Owns: `spec/src/**`, `spec/app/**`, `spec/ark-spec.cabal`,
`spec/generated/**`, `flake.nix`, `README.md`, `harken/README.md`,
`swift/FROZEN.md`, `kotlin/FROZEN.md`, `docs/arkdb.md`, `spec/AUTHORING.md`
§4–§5 and Appendices.

Delivers:

1. The Haskell package deleted (after B1c's port is in), the `spec`
   devshell gone, `ark-spec`, `arkc`, `vectors` packages rebuilt from
   `rust/ark`'s binaries; `checks.vectors` regenerates with the Rust
   binary and diffs; `checks.rust` unchanged; `checks.harken-domain` is
   "`harken.ark` equals what the domain crate emits, and `arkc verify`
   passes"; `checks.swift`, `checks.kotlin`, their packages and shells
   removed; `harken-apk` kept and still building; `harken-module`,
   `harken-serve`, `harken-web` kept.
2. `swift/FROZEN.md` and `kotlin/FROZEN.md`: frozen at spec v3, the last
   commit they were green at, what it would take to come back (the
   vectors of that day, the plan algebra, the projections' expression
   evaluator).
3. `README.md`, `harken/README.md`, `spec/AUTHORING.md` §4 (`arkc`), §5
   (the checks), Appendix B (the demo, one spelling), and `docs/arkdb.md`
   §3.13 (Views), §3.16 (the suite: written by the Rust reference — the
   paragraph already says so; the table of directories updated), §3.17
   (layout: `spec/` is vectors, the contract and the index; the frozen
   trees named as frozen), §3.18 (the path: steps 3–5 marked deferred),
   §3.20 (a decision each for the four at the top of this document).
4. `nix flake check` green, and `nix build .#harken-apk` still evaluating
   (building it is the CI's job; say whether it was built here).

### What the coordinator does between pieces

Read each agent's diff against this document; run the workspace's fast
checks and, before the push, `nix flake check`; commit the agent's paths
if the agent could not; push to `main`. A question an agent raises is
answered by an edit to this document first and a message second.
