# Authoring: one vocabulary, in Rust

This is the contract between the specification, the runtime, `arkc`, and
every domain. A domain is written in Rust against the vocabulary below
(`ark::authoring`, `rust/ark/src/authoring`); running that program under
`Emit` yields the module (`.ark`); running its mutators under `Native`
applies entries directly, so a peer written in Rust runs its domain in
Rust with no code generated for it. A query is not run natively at all: it
is a plan, described once under `Emit`, and `ark::view::pull` is what it
means.

The hash is over the IR (`rust/ark/src/hash.rs`), so what a domain's text
emits is what its functions hash as. Names of `let` bindings and closure
parameters travel in `names`, outside the hash.

Spec version **4** (`docs/plan-v4.md`): every query is a plan. Version 3
brought routers, middleware and input schemas; version 2 had scopes
(`docs/scopes.md` says what they were and why they are gone). Swift and
Kotlin authored the same vocabulary at version 3; at version 4 they are
clients that load a module and do not author one, so this vocabulary is
Rust's alone.
A bare section number (§1.5, §6) is this document's; one that names
`docs/plan-v4.md` is the design's.

## 1. The IR (`rust/ark/src/ir`)

### 1.1 Routers

```rust
pub struct Router {
    pub name: String,        // "playlists"
    pub uses: Vec<String>,   // the middleware declared on it, in declaration order
}
// Module has   routers: Vec<Router>
// Function has router: Option<String>   // None for helpers and middleware
//              uses:   Vec<String>      // the middleware THIS procedure runs, in order;
//                                       // a subsequence of its router's uses. [] for
//                                       // helpers and middleware
```

A procedure runs the chain it was built from and not every middleware its
router declares: if `create_playlist` is `signed_in.input(..)` and
`add_to_playlist` is `owned.input(..)` where `owned` was built on
`signed_in`, the first has `uses = ["signed_in"]` and the second
`["signed_in", "owned"]`. (harken itself has no `signed_in`: a peer used
before anyone signs in authors as nobody, and the server never hears from
an unsigned peer, so the guard would only refuse work done offline.) A
router's `uses` is the union in declaration order and a procedure's `uses`
must be a subsequence of it (verifier: `UsesNotOnRouter`).

A router is a group of procedures and the middleware chains built on them;
it names no tables. Any procedure, query or middleware may read any table
of the module; only a mutator writes. Helpers have no router.

### 1.2 Middleware

```rust
pub enum FnKind {
    Mutator, Query, Helper,
    Guard,    // runs before the body; may refuse; returns nothing
    Provide,  // runs before the body; may refuse; returns `ret`, which the
              // procedure reads as Expr::Provided(<middleware name>)
    Scope,    // runs nothing: what the person holds (docs/plan-guards.md D2)
}
```

A middleware function has `input` (the fields of the procedure's input it
reads, by name and type — the verifier requires every procedure using it
to have those fields with those types), a body over the same `db`, and for
`Provide` a `ret`.

Evaluation order for a procedure: decode input → input checks (1.3) → each
middleware in `uses` order (a refusal stops there) → the body, or for a
query its plan. All of it is inside the transaction; a refusal anywhere
leaves the store untouched. What a maintained query does when the tables
its middleware read change is `docs/plan-v4.md` §1.7.

The closure of a procedure (`hash::closure`) includes its middleware and
the helpers they reach — through its body, its checks or its plan — so
editing a middleware re-hashes every procedure on its router. That is the
intended meaning.

A **scope** (`docs/plan-guards.md` D2) is the third kind, and runs
nothing: it says what the person a procedure is called by *holds*, as a
function of `ctx` alone. It has no input, no body, no return and no plan,
only `holds` — at least one `Hold { table, filter, columns }`: the rows of
`table` its `filter` admits (no filter is every row) and the columns its
projection keeps (`All`, `Pick(cols)`, `Exclude(cols)`; the key is always
kept). A filter is a plan's `Pred` over the table's own columns whose
right-hand sides read the context and literals alone — never input — with
two leaves only a scope may have: `When(e)`, a `Bool` of the context
(`has_role(..)`), and `Exists(child, column, p)`, some row of `child`
whose reference `column` names this row admitted by `p` (one level; the
reference must be declared). A scope is listed in a procedure's `uses` and
hashed into its closure like any middleware; evaluation skips it. What a
person holds is the union of every scope on every procedure of the
module, evaluated for them (`ark::scope`); a module with no scope is held
whole and its every byte is what it was.

### 1.3 Input schema and checks

```rust
// Function has input: Vec<(String, Field)>
pub struct Field { pub ty: Ty, pub checks: Vec<Check> }
pub enum Check {
    Trim,                                   // text: normalise before every later check and before the body
    MinLen(i64, Option<String>),            // text: length in code points ≥ n
    MaxLen(i64, Option<String>),
    Range(Option<i64>, Option<i64>, Option<String>), // int: lo ≤ v ≤ hi, either bound optional
    NonEmpty(Option<String>),               // list: at least one element
    Exists(Option<String>),                 // id: a row with that key exists
    Refine(Expr, Option<String>),           // any: the expression, over Arg(<this field>), is true
}
// Function has refine: Vec<(Expr, Option<String>)>   // over the whole input, after the fields
```

The `Option<String>` is the message; `None` means the default, which
`eval::default_message` defines:

| check | default message |
|---|---|
| `MinLen n` | `<field>: at least <n> characters` |
| `MaxLen n` | `<field>: at most <n> characters` |
| `Range lo hi` | `<field>: between <lo> and <hi>` (or `at least`/`at most` with one bound) |
| `NonEmpty` | `<field>: at least one` |
| `Exists` | `<field>: no such <table>` |
| `Refine` | `<field>: invalid` |

A failing check is a **refusal** with that message, for mutators and for
queries alike. A field of type `Option(t)` applies its checks when the
value is `Some`.

The runtime exposes the same walk as a **form validator**:
`eval::check(schema, closure, ctx, partial input, db)`, running each
field's checks on the fields present, with `Trim` applied first and the
normalised value returned beside the messages. `refine` runs only when
every field is present.

### 1.4 Table writes

```rust
    Insert(TableName, Expr, Vec<FieldName>),   // write the row unless one matches on the columns
    Upsert(TableName, Expr, Vec<FieldName>),   // write the row; if one matches on the columns,
                                               //   keep its key columns and take the rest from the new row
    Update(TableName, Vec<Expr>, Sym, Expr),   // key; the existing row bound to Sym; the new row.
                                               //   A no-op when absent
```

An empty column list means the table's key. A non-empty one must be a
declared **unique index** of the table (`Index { unique: true }`), which
is what lets a store answer the match with one lookup; otherwise a
verifier error (`OnNotUnique`). Every write reports `Add`, `Edit` or
nothing and refuses on a constraint; `Upsert(t, e, [])` is v2's `Put`.

```rust
    Private(Block),                            // docs/plan-guards.md D3: the server half
```

A **private block** (`docs/plan-guards.md` D3) is a mutator's server half.
Only the authority runs it — `Ctx.authority`, which a server's `Authority`
sets and nothing else does; a device, a replay and a peer alone run the
body without it — and it runs **after** the body, in the order the body
reached its blocks, each over the locals bound where it was written: so
the public body can never read what a private block wrote, and a client's
run of the body is the authority's. It may read and write any table and
any column, those no person's union holds included (no `NotHeld`), and may
refuse; the refusal is the entry's verdict. A function with one says
`private: true`, which is hashed; the module a client loads is the
server's with every block stripped (`ir::strip`, `arkc strip`), and a
function's hash is of what is left, so the two modules name it alike. A
client takes every entry of a `private` function by the authority's facts —
an acknowledgement carries them, and a page to a peer that replays carries
them for those entries alone — previewing the public body until they
arrive. The verifier keeps a block in a mutator's body
(`PrivateOutsideMutator`), one level deep (`NestedPrivate`), with no
`return` in it (`ReturnInPrivate`), and its function saying so
(`PrivateUndeclared`).

### 1.5 A query is a plan

`Function` has `plan: Option<Plan>`. A query has `plan: Some`, an empty
body and `ret = List(node type)`; every other kind has `plan: None`
(verifier: `QueryWithoutPlan`, `QueryWithBody`, `PlanOutsideQuery`). The
plan (`docs/plan-v4.md` §1.3) is a tree of reads:

```rust
pub struct Plan {
    pub source: Source,             // Table(t) | Group { table, by }
    pub filter: Option<Pred>,       // over the source table's columns; right-hand sides constant
    pub row: Option<Sym>,           // the source row (a group's key struct); see below
    pub members: Option<Sym>,       // a group's rows, as a list in key order
    pub lookups: Vec<Lookup>,       // Lookup { name, sym, table, key: Vec<Expr> }, bound to Opt<row>
    pub related: Vec<Related>,      // Related { name, sym, on: Vec<(FieldName, Expr)>, plan }
    pub having: Option<Expr>,       // a Bool; a refused node is still an entry to a view
    pub project: Option<Expr>,      // the node's value; absent: the row and a list per related plan
    pub order: Vec<(Key, Dir)>,     // Key::Column(name) | Key::Expr(expr); completed with the key
    pub limit: Option<i64>,         // a window over the admitted nodes (per parent, in a child)
}
```

Each node is evaluated in that order: the rows the filter admits; `row`
and `members` bound; the lookups in order (each may use the ones before
it, and a `Null` key part is `None`); the related plans, each pinned by
`child.column == expr(parent)` for every `on` pair and its own filter,
order and limit per parent; `having`; `project`; the order keys. The
answer is the admitted nodes in order — each key under its direction,
then the node's key — cut to the limit.

- **No expression reads.** Lookup keys, `on`, `having`, `project` and
  expression order keys may use a binder in scope, `Arg`, `CtxUser`,
  `CtxSession`, `HasRole`, `Provided`, literals, operators, `Std`, `Call` of a helper
  and every list function — never `Select`, `Get` or `Exists`
  (`ReadInPlan`), and never `Auto` (`AutoInPlan`).
- **Scope is flat per node.** A node's expressions see its own binders; a
  child reaches its parent only through `on` (`UnboundSymbol` otherwise).
  A filter sees only what is constant for the read.
- **Options are flat**: `Some(v)` is `v` and `None` is `Null`, so an `on`
  across an `Opt<T>` column is plain equality. The verifier accepts `T ==
  T` and `Opt<T> == T` either way round in an `on`, and `Opt<T>` for a
  lookup key part whose column is `T`. An `on` pin is a filter's
  equality, so a parent whose value is `None` is pinned to the children
  whose column is `Null`, as `.filter(C::c.eq(none()))` would be; a lookup
  is different, since no key holds a `Null`, and a `None` part finds
  nothing.
- **A related plan is named** after its child's table (`.each`) or its
  reference (`.with`); a second of one name in the same node is numbered
  `_2`, `_3`…, since without a projection each is a field of the node.
- **`row` is absent exactly when nothing could reference it**: a plan
  with no lookup, related plan, having, projection, expression order key
  or group — the v3 shape, which is what every mutator's read is. Such a
  plan consumes no symbol and writes no `row` key, so a mutator's bytes and
  hash are unchanged by v4 (Appendix A); a plan that binds anything without
  one is `NoRowBinder`, and `members` on anything but a group is
  `MembersWithoutGroup`.
- **A mutator's reads stay v3-shaped** (`PlanFeatureOutsideQuery`): a
  table source, a filter, column orders, a limit and `.with` — a related
  plan whose `on` is a declared reference, `[(fk, row.key)]`.

### 1.6 Everything else is unchanged

`Sym`, `Stmt::Let` (a read only as a whole right-hand side), `If`, `For`,
`Delete`, `Refuse`, `Return`, every other `Expr`, `Pred`, `StdFn`, `Auto`,
`autos`, `names`, the schema, the log, the peer, the protocol, `live`.

## 2. The vocabulary

One name per IR node, in `snake_case`; string names in the IR are always
`snake_case`.

### 2.1 Values

| type | is | Rust |
|---|---|---|
| bool | `Ty::Bool` | `Bool` |
| int | `Ty::Int` | `Int` |
| text | `Ty::Text` | `Text` |
| bytes | `Ty::Bytes` | `Bytes` |
| id of T | `Ty::Id(t)` | `Id<T>` |
| option | `Ty::Option` | `Opt<T>` |
| list | `Ty::List` | `List<T>` |
| row of T | struct | `T` |
| non-empty list | `Ty::NonEmpty` | `NonEmpty<T>` (§2.6) |
| sum of records | `Ty::Sum` | `Sum<V>` (§2.6) |
| a slot per device | `Ty::PerDevice` | `PerDevice<T>` (§2.6) |

These are the *value types of the vocabulary*, distinct from Rust's
`bool`/`i64`/`String`: under `Native` they carry data, under `Emit` an
expression. A host `if` on one does not compile. Literals lift through
`From`: `0`, `"a playlist needs a name"`, `true` where an `Int`, `Text`,
`Bool` is expected.

### 2.2 Operations (methods on values)

| IR | Rust |
|---|---|
| `Cmp Eq/Ne/Lt/Le/Gt/Ge` | `.eq(b)` `.ne` `.lt` `.le` `.gt` `.ge` |
| `Op Add/Sub/Mul/Div/Mod/Neg` | `.add(b)` `.sub` `.mul` `.div` `.rem` `.neg()` |
| `Op And/Or/Not` | `.and(b)` `.or(b)` `.not()` |
| `Std Trim/IsEmpty/Lower/TextLen/…` | `.trim()` `.is_empty()` `.lower()` `.len()` `.starts_with(p)` `.split_once(p)` `.chars()` `.is_alnum()` `.utf8()` |
| `Std Concat` | `concat(list)` |
| `Std TextOfInt/TextOfId/IdOfText/Hex/Fnv1a64/Sha256/NilId` | `.to_text()` `.to_text()` `id_of_text(t)` `.hex()` `.fnv1a64()` `.sha256()` `nil_id()` |
| `Std Min/Max/Clamp/Abs` | `.min(b)` `.max(b)` `.clamp(lo, hi)` `.abs()` |
| `Std First/Last/Len/Contains/Reverse` | `.first()` `.last()` `.len()` `.contains(x)` `.reverse()` |
| `Std ListConcat/Insert/RemoveAt/Move/Slice` | `.concat(ys)` `.insert(i, x)` `.remove_at(i)` `.move_at(from, to)` `.slice(start, end)` — the list editors (below) |
| `Std IsSome/UnwrapOr/Unwrap` | `.is_some()` `.unwrap_or(d)`; `.or_refuse("why")` (statements, in a body) and `.unwrap()` (an expression, for a projection behind a `having` that holds `is_some()`) |
| `Match` | `.map_or(d, \|x\| e)` and `.map(\|x\| e)` (option) |
| `Map/Filter/Any/All/SortBy` | `.map(\|x\| e)` `.filter(\|x\| e)` `.any(..)` `.all(..)` `.sort_by(..)` |
| `Fold` | `.fold(init, \|acc, x\| e)` |
| `Some/None` | `some(x)` `none::<T>()` |
| `Struct/Field` | struct literal / `row.field` |
| `List` | `list([a, b])` |
| `Call` | `helper_name(args)` (a Rust fn) |
| `CtxUser/CtxSession` | `ctx.user` `ctx.session` |
| `HasRole` | `ctx.has_role("library")` → `Bool`, whether the entry's author holds the role (`docs/plan-guards.md` D1) |
| `Auto` | `ctx.now("added_ms")` `ctx.new_id("id")` |
| `Provided` | the extra body parameter |

**The list editors.** A list is edited by value, as everything is: each
editor returns a new list and leaves the one it was given alone.

```rust
queue.insert(at.add(1), track)              // after the track at `at`
queue.remove_at(index)                      // the track at `index` taken out
queue.move_at(from, to)                     // ends at `to`; those between shift over
queue.slice(0, at.add(1))                   // up to and with `at`
queue.slice(0, cut).concat(tracks).concat(queue.slice(cut, queue.len()))  // several, at `cut`
```

- **An index out of range is a refusal**, never clamped, as an overflow is
  (§6.5 of the evaluator): `insert` takes `0..=len` (`len` is the end),
  `remove_at` and `move_at` take `0..len`, `slice` takes `0 <= start <=
  end <= len`, and anything else refuses the run with
  `insert: index 4 out of range for a list of 3` — the function's IR name in
  snake_case (`move`, for `move_at`), the index and the length — or
  `slice: 3..1 out of range for a list of 4`. A function of two indices
  names the first that is out of range. A body that computes an index it
  has not checked should check it, or clamp it itself (`.min(xs.len())`),
  and say so: a refusal is what a peer that took a stale index sees.
- **Natively both arms of a `pick` are computed** (§2.4), so an editor in
  the arm not taken still refuses there while the IR, which runs only the
  arm taken, does not, and `Native` and `Emit` disagree. An index a guard
  has not already held in range is clamped before it is used.
- **`NonEmpty<T>`** has the editors too, typed by what they can leave:
  `insert`, `concat` and `move_at` of one are a `NonEmpty<T>`, since none of
  them can empty it; `remove_at` and `slice` of one are a `List<T>`, which
  `.non_empty()` declares one again — held where it crosses a boundary, so
  a transition whose result empties it is refused there
  (`discard: the state it makes is not a deck: hand: an empty list where a
  non-empty one is declared`). To the checker both are lists (`spec/README.md`
  §2.2), so this is the vocabulary's typing, and the module is the same
  either way.
- **`move` is Rust's**, so the method is `move_at`; the IR and the
  refusal say `Move` and `move`.

Every other edit is made of these: there is no `insert_all`, because
`slice`, `concat`, `slice` is one, and no indexed read, because
`.slice(i, i.add(1)).first()` is one.

### 2.3 Tables and plans (`db.<table>` is a `Table<Row>`)

A mutator's body reads and writes:

| IR | Rust |
|---|---|
| `Get t k` | `db.playlist.get((id,))` → `Opt<Playlist>` |
| `Exists t k` | `db.playlist.exists((id,))` → `Bool` |
| `Select` | `db.playlist.filter(p).order_by(o).limit(n).all()` → `List<Playlist>` |
| `Select` + `First` | `….first()` → `Opt<Playlist>` |
| `Select` with a related plan | `.with(Playlist::playlist_item)` |
| `Insert` | `db.playlist.insert(row)` / `.insert(row).on((Playlist::user_id, Playlist::name))` |
| `Upsert` | `db.playlist.upsert(row)` / `.on(..)` |
| `Update` | `db.playlist.update((id,), \|row\| Playlist { .. })` |
| `Delete` | `db.playlist.delete((id,))` |
| `Pred` | `Playlist::user_id.eq(x)` `.and(..)` `.or(..)` `.not()` `Playlist::id.in_(list)` |
| `Pred::Has` | `Media::title.has(needle)` — the text column holds the needle as a substring, both folded by the pinned `lower`; a `None` column holds nothing. Read through a text index on the column (`.index_text(..)`), the rows holding every trigram of the needle, when it has three characters or more; `docs/plan-db.md` D4 |
| order | `Playlist::name.asc()` `.desc()`; several as a tuple |

A key is a tuple; its shape is the row's `Key` type, so a wrong order does
not compile.

A query's closure returns a `Query<R, B, N>` — its plan, described — and
never calls `.all()` or `.first()`:

| plan | Rust |
|---|---|
| source table | `db.song` (a `Table<Song>`; `db.song.rows()` is its `Query<Song>` when the first step is a lookup) |
| filter | `.filter(Song::album_name.eq(some(input.name)))` |
| group source | `db.media.group_by(Media::creator)` — a `Query<Text, (List<Media>,)>`: closures are handed the key (the column's value, or a tuple of values for a tuple of columns) and the group's rows as the first binder |
| text search | `.filter(Media::title.has(input.needle).or(Media::creator.has(input.needle)))` — a filter like any other, so a view over it re-admits a changed row by the predicate; an `or` of `has` is read as the union of each branch's postings |
| distinct | `db.media.distinct(Media::creator)` — `group_by(Media::creator).map(\|creator, _\| creator)`, the same plan: a group with no aggregate, whose node is its key (for a tuple of columns, the key struct); `docs/plan-db.md` D4 |
| lookup | `.get(\|song, ()\| db.media.by((song.media_id,)))` — appends `Opt<Media>`; `db.t.by_opt(opt)` for a one-column key held as an option |
| related, by reference | `.with(Media::playlist_item)` — appends `List<PlaylistItem>` |
| related, general | `.each(\|song, (media,)\| db.credit.order_by(..).on(Credit::recording_id.eq(song.recording_id)))` — appends `List<Node>`; the child's own `get`, `each` and `map` nest |
| having | `.having(\|album, (songs,)\| songs.len().gt(0))` |
| order by column | `.order_by(Media::pos.asc())` |
| order by expression | `.sort_by(\|song, (media, movement)\| movement.map_or("", \|m\| m.part))`, `.sort_by_desc(..)` |
| projection | `.map(\|song, (media, items)\| LibraryEntry { .. })` — the node; the query's value is the list of them |
| limit | `.limit(n)` (per parent, in a child plan) |

Binders accumulate in a tuple, `Query<R, (A, B, C)>`, and every closure
takes `(row, (a, b, c))`: up to six, and more is a nested plan. Inside a
closure the row and each binder are ordinary vocabulary values, so every
list function and every helper is available. A read there — `db.t.get(..)`,
`.exists(..)`, `.all()`, `.first()` — is a `build()` error naming the
rule, and so is a query's feature (a lookup, a group, a having, a
projection, an expression order) in a mutator's read, and an `.on(..)`
that is not column equalities.

**`sort_by` appends a key.** Keys compare in the order given: every
`order_by` column first, as written, then every `sort_by` expression, as
written, then the key columns. The *first* `sort_by` is the primary key —
not the list `sort_by` of v3, where a stable sort made the last call
primary.

`db.t.by(key)` is `get` spelt for a lookup, so that a plan's lookup and a
mutator's `db.t.get(key)` read differently: one is a node, the other a
bound read.

### 2.4 Control (all return a value; bodies are closures)

| IR | Rust |
|---|---|
| `If c t []` | `when(c, \|\| t)` |
| `If c [] e` | `unless(c, \|\| e)` |
| `If c t e` | `if_else(c, \|\| t, \|\| e)` |
| `Expr::If` | `pick(c, a, b)` |
| `For` | `for_each(xs, \|x\| body)` |
| `Refuse` | `refuse("why")` |
| `Private` | `ctx.private(move \|db: &Db\| body)` (`docs/plan-guards.md` D3) |
| `Return` | the closure's value |
| `Let` | `let x = …` (a read becomes a `Let` at once) |

A mutator body's value is `Effect`; a query's is its plan. A helper is an
ordinary function of the vocabulary's types and is emitted when called.

### 2.5 Declarations

A domain crate is `schema.rs` (the tables), one file per router, and
`module.rs`:

```rust
// schema.rs
pub struct Harken {                          // the module's tables, in order
    pub playlist: Table<Playlist>,
    pub playlist_item: Table<PlaylistItem>,
}
impl Tables for Harken {
    fn open() -> Self {
        Harken {
            playlist: table(),
            playlist_item: table(),
        }
    }
}
// `open()` is how a body's `db` is made, and the order its fields are
// written is the schema's table order: Rust cannot enumerate a struct's
// fields, so the tables say them once, here.

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
    pub created_ms: Int,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .text(Self::user_id)
            .int(Self::created_ms)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const created_ms: Col<Self, Int> = col("created_ms");
    pub const playlist_item: Rel<Self, PlaylistItem> = rel("playlist_item"); // from PlaylistItem's .refs
}
```

`columns()` also has `.bool`, `.bytes`, `.enum_(Self::c, ["a", "b"])`,
`.nullable()` after a column whose type is an `Opt`,
`.refs::<Parent>()` after a column that references a parent's key,
`.index((..))`, and `.index_text(Self::c)` — a text index on one text
column, the trigrams of its folded value (`docs/plan-db.md` D4): it serves
`c.has(..)`, says nothing about the rows, and moves the module's hash and
no mutator's.

A list a node uses only as a count, a sum, a least or a greatest value is
kept by a view as that number rather than as a list (`docs/plan-perf.md`
R9, `docs/plan-db.md` D4) — nothing to declare, but worth writing so:
`xs.len()`, `xs.fold(0, |acc, x| acc.add(f(x)))`,
`xs.fold(init, |acc, x| acc.max(x.c))` (or `min`), and
`xs.first().map_or(d, |x| x.c)` (or `last`) of a list ordered by `c` and
nothing else. The extremes are kept only where an index serves `c` under
the list's `on` columns; elsewhere, and for any other use, the list is a
list.

```rust
// playlists.rs
pub struct Owned {                           // what `owned` reads of an input
    pub playlist_id: Id<Playlist>,
}
impl Input for Owned {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}

pub fn playlists() -> Router<Harken> {
    let playlists = router::<Harken>("playlists");
    let owned = playlists.provide("owned", |ctx, db, input: &Owned| {
        db.playlist
            .get((input.playlist_id,))
            .filter(|row| row.user_id.eq(ctx.user))
            .or_refuse("not your playlist")
    });
    playlists.routes((
        playlists.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| { .. }),
        owned.input::<AddToPlaylist>().mutation("add_to_playlist", |ctx, db, input, playlist| { .. }),
        playlists.query("playlists", |ctx, db, _input: ()| {
            db.playlist.filter(Playlist::user_id.eq(ctx.user)).order_by(Playlist::pos.asc())
        }),
        owned.input::<PlaylistInput>().query("playlist", |_ctx, db, _input, playlist| {
            db.playlist_item
                .filter(PlaylistItem::playlist_id.eq(playlist.id))
                .order_by(PlaylistItem::pos.asc())
                .get(|item, ()| db.media.by((item.media_id,)))
                .having(|_item, (media,)| media.is_some())
                .map(|item, (media,)| library_entry(media.unwrap(), some(item.pos)))
        }),
    ))
}
```

A **scope** (`docs/plan-guards.md` D2, §1.2) is declared with `server`,
named like any middleware, on the router — every procedure on it
inherits it — or on a chain, and returns what it holds of one table or a
tuple of tables:

```rust
let orgs = router::<Orgs>("orgs");
let scoped = orgs.server("memberships", |ctx, db| {
    (
        db.org.filter(exists(Member::org_id, Member::user.eq(ctx.user))
            .or(Pred::when(ctx.has_role("admin")))),
        db.member.filter(Member::user.eq(ctx.user)),
    )
});
let own = accounts.server("own_account", |ctx, db| {
    db.account.filter(Account::user.eq(ctx.user)).exclude(Account::password)
});
scoped.routes((scoped.client("my_orgs", |_ctx, db, ()| db.org.rows()), ..))
```

`db.t.rows()` and `db.t.filter(p)` hold every column, `.exclude(cols)` and
`.pick(cols)` narrow them; an order, a limit or anything beneath a row is
an authoring error, since a scope names rows and columns.
`Pred::when(b)` and `exists(Child::fk, p)` are the scope's two leaves.
`client(name, f)` is `query(name, f)` exactly — the same IR and hash —
named for what it runs over, and refused at build on a chain that carries
no scope. Mutations keep `mutation`: a write is a preview on the device
whatever it holds, and its server half is D3's `ctx.private`.

Field builders: `text()`, `int()`, `bool_()`, `bytes()`, `id::<T>()`,
`enum_::<E>()`, `opt(f)`, `list(f)`; checks `.trim()`, `.min(n)`,
`.max(n)`, `.range(lo, hi)`, `.at_least(lo)`, `.at_most(hi)`,
`.non_empty()`, `.exists()`, `.refine(|v| ..)`; and `.why("…")` after any
check but `.trim()` to give it its message; `object().refine(|input|
..).why("…")` over the whole input.

A router file also declares the module's **helpers** and **records**:

```rust
// library.rs
pub struct LibraryEntry {                    // a record: what `library_entry` returns
    pub added_ms: Int,
    pub playlist_pos: Opt<Int>,
    // … every field, alphabetically
}
impl Record for LibraryEntry {
    fn fields() -> Fields<Self> {
        fields().field("added_ms", int()).field("playlist_pos", opt(int())) // …
    }
}

pub fn movement_key(work_id: Text, no: Int) -> Text {
    helper("movement_key", (("work_id", work_id), ("no", no)), |work_id: Text, no: Int| {
        concat(list([work_id, "#".into(), no.to_text()]))
    })
}
```

- **A helper** is `pub fn name(a: A, ..) -> R { helper("name", PARAMS,
  |a: A, ..| body) }`: `PARAMS` is the bare pair `("a", a)` for one
  parameter and a tuple of pairs for several; the closure's parameters are
  the helper's, typed. A helper reads nothing and refuses nothing; a
  projection calls one like any expression.
- **A record** is a struct type some function returns that is not a row.
  Its fields, in the struct and in `fields()`, are in one order —
  alphabetical, the IR's struct type being a map — because a record is
  built and taken apart by position.
- **A literal is lifted with `.into()`** where the vocabulary takes exactly
  a type rather than anything that converts to it: a row's or a record's
  field, a list's element, a helper's argument.

The module: `Module::new((library(), playlists()))`, with helpers found by
being called. `module().emit()` is the `.ark` bytes, `module().procedures()`
the procedures for a runtime, and `module().hash()` the module hash.

**harken's domain is written out in full in this form** in
`harken/domain/src/{schema.rs,library.rs,playlists.rs,module.rs}`; their
`emit()` is `harken/domain/harken.ark`.

### 2.6 Live state

`docs/plan-live.md`: a typed state machine per scope, beside the log, on
the one socket (`spec/README.md` §14.1). It is declared on a router beside
its mutations, in the same words, and a module without one emits the
bytes it did.

```rust
pub struct Room {                            // a record, named in the type section
    pub count: Int,
    pub devices: PerDevice<Device>,          // who is here: ordered, kept by the hooks
    pub mode: Sum<Mode>,
    pub pointer: PerDevice<Int>,             // where each device points: ephemeral
}
impl Record for Room {
    fn fields() -> Fields<Self> {
        fields()
            .named("room")
            .field("count", int())
            .field("devices", typed::<PerDevice<Device>>())
            .field("mode", typed::<Sum<Mode>>())
            .ephemeral("pointer", typed::<PerDevice<Int>>())
    }
}

pub struct Mode;
impl SumType for Mode {
    const NAME: &'static str = "mode";
    fn variants() -> Cases<Self> {
        cases().of::<Idle>("idle").of::<Counting>("counting")
    }
}

now.live::<Room>("room")
    .scope(|ctx| ctx.user)                                   // one instance per account
    .init(|| Room { count: 0.into(), devices: PerDevice::empty(),
                    mode: Sum::of("idle", Idle {}), pointer: PerDevice::empty() })
    .transition::<Bump>("bump", |s, _ctx, input| Room { count: s.count.add(input.by), ..s })
    .transition::<()>("stop", |s, _ctx, ()| Room { mode: Sum::of("idle", Idle {}), ..s })
    .guard::<()>("not counting", |s, _ctx, ()| s.mode.is("counting"))
    .transition::<At>("point", |s, ctx, input| Room { pointer: s.pointer.put(ctx.device, input.at), ..s })
    .on_join(|s, _ctx, device| Room { devices: s.devices.put(device.id, device), ..s })
    .on_leave(|s, _ctx, device| Room { devices: s.devices.clear(device.id), ..s })
    .on_restore(|s, _ctx| Room { devices: PerDevice::empty(), mode: Sum::of("idle", Idle {}), ..s });
```

| IR | Rust |
|---|---|
| the type section | a record's `.named("room")`, and every `SumType`: declared once by name, referred to by it (`Ty::Named`) |
| `Ty::Sum` | `Sum<V>` over `impl SumType for V`: `cases().of::<R>("name")…`, a record per variant |
| `Variant` | `Sum::of("counting", Counting { .. })` |
| `Case` | `s.mode.case::<T>().on::<Idle>("idle", \|_\| ..).on::<Counting>("counting", \|c\| ..).end()`; `.is("counting")` is the case with a `true` arm |
| `Ty::NonEmpty` | `NonEmpty<T>`: `.list()` to read it as a list, `list.non_empty()` to claim one; `.insert(i, x)`, `.concat(ys)`, `.move_at(from, to)` keep it one, `.remove_at(i)` and `.slice(start, end)` are a `List<T>` (§2.2) |
| `Ty::PerDevice` | `PerDevice<T>`: `PerDevice::empty()`; `Slot` `.get(device)` → `Opt<T>`; `SetSlot` `.set(device, opt)`, `.put(device, v)`, `.clear(device)`; `Slots` `.slots()` → `List<Slot<T>>`, `{device, value}` in device order |
| `Ty::Ephemeral` | `.ephemeral(name, typed::<PerDevice<T>>())` on the state's record |
| `LiveState` | the transition's first parameter |
| `CtxUser/CtxSession/CtxDevice/CtxNow` | `ctx.user` `ctx.session` `ctx.device` `ctx.now`; `ctx.has_role(r)` |
| a state | `r.live::<S>(name)` `.scope(..)` `.init(..)` |
| a transition | `.transition::<I>(name, \|state, ctx, input\| new_state)`, `I` an `Input` (§1.3) or `()`; `refuse(..)` and the input's checks refuse it |
| a guard | `.guard::<I>(why, \|state, ctx, input\| bool)`, on the transition or hook declared last, refusing with `why` when false |
| the hooks | `.on_join(\|s, ctx, device\| ..)`, `.on_leave(\|s, ctx, device\| ..)` — `device` a `Device` record: `can_play`, `id`, `kind`, `name`, `user` — and `.on_restore(\|s, ctx\| ..)` |

**A transition's kind is read off its body, not declared.** One that
reads nothing of the state — only its input and `ctx.device` — and writes
nothing but the caller's own slot of ephemeral fields (`point`, above) is
**ephemeral**: relayed, coalesced, never sequenced, applied by its author
at once and never pending. Every other is **ordered**. An ordered
transition may not read an ephemeral field, which the verifier refuses by
name; it may write its caller's own slot (`drop_at` in Appendix B.2, which
clears the dropper's pointer). Three rules, and the plan's "Ordered and
ephemeral fields" argues them: no ordered reads of ephemeral fields;
ephemeral fields are left out of the state's hash and of what a server
keeps; and an ephemeral call is held to the same guards and type checks as
any other, with its slot to 1 KiB.

**Write positions in content terms** — an item's id and a fraction along
it, a time in milliseconds — never in pixels: two devices drawing one
pointer at two sizes must agree on what it points at.

**Native and Emit.** A transition is run once under `Emit` to be its IR
and on every call under `Native`, as a mutator's body is, and the two are
held to agree on every transition and every hook
(`NativeLive::agrees`; `rust/ark/tests/live.rs`). The router's `lives()`
are what a runtime is handed beside its procedures.

**What a client calls.** `ark_client::Peer::live(name)` is the state as
this device sees it; `peer.call(name, transition, input)` runs it over
that view at once and sends it — one path for both kinds — and
`peer.call_throttled(..)` is the same for a transition a finger drives, at
most one send per `THROTTLE_MS` (50) for it, the last always sent;
`Pumped::live` says the view moved, which is what a host woken by
`set_waker` reads; `take_live_refusals()` the server's verdicts;
`server_offset()` the server's clock less this one's, read off the `at`
stamps, which is what a moving position is computed with. A server stands
a device it speaks for — a speaker a bridge reconciles — in its instances
with `HubHandle::participate`, and calls as it with `HubHandle::call`.

## 3. Native and Emit

The builder implements the vocabulary twice behind one API:

- **Emit**: every value holds an `Expr`; every `let` of a read is a
  `Stmt::Let`; every closure passed to `when`, `unless`, `if_else`,
  `for_each`, `map_or`, `map`, `filter`, `update` is run **once**, with
  fresh symbols for its parameters, to record its body; `ctx.now(name)`
  and `ctx.new_id(name)` register an auto. A query's closure is run once,
  in the plan's context — no statement and no read may be written there —
  and so is every closure of its plan (`get`, `each`, `having`, `sort_by`,
  `map`); what it returns is the query's plan.
- **Native**: every value holds a `Value`; a read runs at once against
  the transaction through `eval::select_plan`, which is `view::read`,
  the answer `view::pull` gives;
  `when` runs its closure only when the condition holds; `ctx.now(name)`
  returns the entry's auto of that name; `ctx.private(f)` keeps `f`, when
  the run is the authority's, and runs it after the body (and otherwise
  drops it unrun) — which is why `f` owns what it captures. Only mutators and middleware run
  natively: a query has no native half, and `Procedure::query` is
  `eval::query_closure`, which pulls the plan.

Both are selected by the context the procedure is run under; a body has
no way to tell which. A native mutator must agree with `eval` over the
function's own `Emit`; the runtime's tests hold it to that on every
mutator of the demo and of harken (`Procedure::agrees`).

## 4. What `arkc` does now

`arkc` is a binary of `rust/ark` (`rust/ark/src/bin/arkc.rs`; `nix run
.#arkc`). It reads a `.ark` file — a module's canonical CBOR — and verifies
it before anything else, so what it hashes or compares is the verified
form an entry's hash names:

- `arkc verify M`: verify, and print the module hash.
- `arkc hash M`: the module hash and every function's hash; for a server's
  module with private blocks, the public hash — of the module a client
  loads — beside it (`docs/plan-guards.md` D3).
- `arkc strip M OUT`: the module a client loads, every private block taken
  out, verified and written; its hash printed. A domain's `emit()` is
  already this, and `emit_server()` the module with its blocks.
- `arkc check OLD NEW`: log compatibility (spec §17, `ark::compat`) — every
  break, and exit 1; or nothing. Run it between the committed module and a
  new one before a change that could move a retained entry's meaning.
- `arkc vectors OUTDIR`: write the conformance vectors, as `ark-vectors`
  does.
- `arkc gen kotlin M PACKAGE OUT`, `arkc gen swift M OUT`: the module's type
  section and live states as Kotlin and Swift types (`docs/plan-live.md`),
  so an app on a phone reads a live state as a typed value: each record a
  data class or a struct, each sum a sealed class or an enum, a record or
  sum written in place named for where it is, each read from the runtime's
  `Value` and written back as one; and `LiveStates`, every state by name.
  The demo's room, printed, is `spec/vectors/gen/`, which the runtimes
  compile and round-trip every state of `eval/live-room.json` through.

There is no `print` or `roundtrip`, and `gen` prints types, never a domain:
those existed to hand a phone a domain in its own language, and with the
phones interpreting the module there is nothing of a domain to print
(`docs/plan-v4.md`, decision 4). A domain's source is the Rust a person
wrote; its `.ark` is what that source emits.

## 5. The checks that hold it together

1. `emit` of the demo (Appendix B) equals `spec/vectors/module/demo.json`'s
   bytes and hash (`the_demo_emits_what_the_spec_records`, in
   `rust/ark/tests/demo_authoring.rs`). The generator's copy of the demo
   (`rust/ark/src/bin/vectors/demo.rs`) is what wrote those bytes, and
   `rust/ark-client/src/demo.rs` is the third copy, held by being the same
   text.
2. harken's `harken/domain/harken.ark` equals what the domain crate emits,
   and verifies: `the_module_verifies_and_is_the_committed_file` in
   `harken/domain/tests/agreement.rs`, and `checks.harken-domain` in the
   flake, which writes the module with the `harken-domain` binary, diffs it
   against the committed file and runs `arkc verify` on it.
3. `Native` against the evaluator over the function's own `Emit`, on every
   *mutator* of the demo (`native_agrees_with_the_interpreter_on_every_procedure`)
   and of harken (`every_procedure_agrees_with_the_interpreter`), step
   after step over one evolving store: the same verdicts, changes and
   stores (`Procedure::agrees`). A query has no native half to compare
   (§3); what it means is `view::pull`, and the view engine's contract
   holds its maintenance to a fresh pull.
4. The vectors: `rust/ark`'s `ark-vectors` writes `spec/vectors`, and
   `rust/ark/tests/vectors.rs` reads every one back and fails every
   `falsify/` case — two programs against one set of files.
   `checks.vectors` regenerates them into a temporary directory and `diff
   -r`s the committed tree, so a change to the reference that moves a
   vector cannot land without the moved vector.

## 6. Lowerings: what each spelling emits

The bytes of a module are a function of these rules and nothing else.
Symbols below are fresh; normalisation renumbers them.

| spelling | emits |
|---|---|
| `db.t.filter(p).order_by(o).limit(n).all()` | `Let(s, Select(plan))`; the value is `Var(s)` |
| `….first()` | `Let(s, Select(plan{limit = 1}))`, `Let(s', Std(First, [Var(s)]))`; the value is `Var(s')` |
| `db.t.get(k)` / `db.t.exists(k)` | `Let(s, Get(t, k))` / `Let(s, Exists(t, k))`; the value is `Var(s)` |
| `opt.map_or(d, \|x\| e)` | `Match(opt, x, e, d)` |
| `opt.map(\|x\| e)` | `Match(opt, x, Some(e), None(T))` |
| `opt.filter(\|x\| p)` | `Match(opt, x, If(p, Some(Var(x)), None(T)), None(T))` |
| `opt.or_refuse(msg)` | `Let(s, opt)`, `If(Std(IsSome, [Var(s)]), [], [Refuse(Lit(msg))])`; the value is `Std(Unwrap, [Var(s)])` |
| `opt.unwrap()` | `Std(Unwrap, [opt])`: the value, or the refusal `unwrapped none` |
| `opt.unwrap_or(d)` | `Std(UnwrapOr, [opt, d])` |
| `when(c, \|\| body)` / `unless` / `if_else` | `If(c, then, else)`, with the closure's statements as the block(s) |
| `for_each(xs, \|x\| body)` | `For(x, xs, body)` |
| `refuse(msg)` | `Refuse(Lit(msg))` |
| `db.t.insert(row)` / `.on(cols)` | `Insert(t, row, [])` / `Insert(t, row, cols)` |
| `db.t.upsert(row)` / `.on(cols)` | `Upsert(t, row, [])` / `Upsert(t, row, cols)` |
| `db.t.update(k, \|row\| new)` | `Update(t, k, row, new)` |
| `db.t.delete(k)` | `Delete(t, k)` |
| `r.crud::<T>()` / `.except(&["insert_t"])` | four mutators on `r`, each `uses` the chain `r` is, and each exactly what the same lines written by hand emit (`docs/plan-guards.md` D4): `insert_<t>`, input every column in order (a nullable one an option), `Let(s, Exists(t, key))`, `If(Var(s), [Refuse(Lit("<t>: a row with this key exists"))], [])`, `Insert(t, row, [])`; `update_<t>`, the same input, `Let(s, Exists(t, key))`, `If(Var(s), [], [Refuse(Lit("<t>: no row with this key"))])`, `Update(t, key, _, row)`; `delete_<t>`, input the key's columns, `Delete(t, key)`; `put_<t>`, `Upsert(t, row, [])`. `.except` leaves the named ones out, to be written by hand under the same names |
| `let x = e` | nothing: a read is a `Let` by itself and a pure expression is inlined where it is used |
| `input.f` | `Arg("f")` |
| the provided parameter | `Provided("<middleware name>")` |
| `ctx.user` / `ctx.session` | `CtxUser` / `CtxSession` |
| `ctx.has_role("r")` | `HasRole("r")`: natively, whether the run's `Ctx` holds `r` — the device's belief when it authors, the entry's stamped roles at the authority and in every replay |
| `ctx.private(f)` | `Private(block)`, the closure's statements as the block; the function's `private` set (`docs/plan-guards.md` D3) |
| `ctx.now("n")` / `ctx.new_id("n")` | `Auto("n")`, and `("n", Now)` / `("n", NewId(t))` appended to `autos` in the order the run meets them |
| a row or record literal | `Struct` of every field written |

A query's plan (§1.5), part by part:

| spelling | emits |
|---|---|
| the query's closure returning `q` | `plan: Some(q's plan)`, `body: []`, `ret: Some(List(q's node type))` |
| `db.t.group_by(c)` / `group_by((c, d))` | `source: Group { table: t, by: [c] }` / `by: [c, d]`, `members: Some(m)`; the row binder is the key struct, handed to closures as the column's value (a tuple of them) |
| `T::c.has(x)` | `Pred::Has(c, x)` |
| `.index_text(T::c)` | `c` appended to the table's `text` (`Table::with_text`); on the wire an `index` of kind `text` |
| `db.t.distinct(c)` / `distinct((c, d))` | `group_by`'s plan with `project: Some(Field(Var(row), c))` / `project: Some(Var(row))`; the node type is the column's / the key struct |
| `.get(\|row, bs\| db.u.by(k))` / `db.u.by_opt(o)` | `Lookup { name: u, sym, table: u, key: k }` / `key: [o]`, appended to `lookups` |
| `.with(T::rel)` | `Related { name: rel, sym, on: [(fk, Field(Var(row), key))], plan: <every row of the child> }` |
| `.each(\|row, bs\| q.on(C::c.eq(e)))` | `Related { name: <q's table>, sym, on: [(c, e)], plan: q's plan }` (`.and` for several pairs) |
| a related plan's name taken by an earlier one in the node | the name with `_2`, `_3`… appended |
| `.having(\|row, bs\| p)` | `having: Some(p)` (a second one `And`s onto the first) |
| `.sort_by(\|row, bs\| k)` | `(Key::Expr(k), Asc)` after the column keys; `.sort_by_desc` is `Desc` |
| `.map(\|row, bs\| e)` | `project: Some(e)`; the node type is `e`'s |
| the row binder | `row: Some(s)`, or `None` when the plan is bare: no lookup, related plan, having, projection, expression order key or group |

**A body's value.** A mutator's closure returns `Effect`, which is the last
write statement; a body with no write ends after its last statement and
nothing is returned. A guard's closure returns `Effect` the same way. A
helper's and a provider's closure returns a value: `Return(e)` is
appended. A query's closure returns its plan, and its body is empty.

**Function order in the module.** Routers in the order `Module::new` was
given them; within a router, its middleware in declaration order, then its
routes in order; a helper is placed immediately before the first function
in that order that calls it — in its body, its checks or its plan (a
helper a helper calls precedes its caller). `emit()` returns the verified
form: orders completed and every function normalised, so the bytes equal
`verify`'s output.

**Middleware.** `input` is the declared input type's fields by name and
type, checks stripped; `router` is `None`; `uses` is `[]`. A procedure's
`uses` is the chain it was built from, oldest first.

**Orders.** The verifier completes every order with the key columns
ascending — a group's `by` columns — omitting any already there as a
column key; an author never writes the completion.

**Relationships.** For a `Ref` from `child.col` to `parent`, the parent row
type gains `pub const <child>: Rel<Self, Child> = rel("<child>")` — named
`<child>_<col>` when the child references the parent through more than
one column — and the related plan `.with` writes is named after it.

**Input types are named after the function that declares them**, in
PascalCase: `CreatePlaylist` for `create_playlist`, `Owned` for the `owned`
provider's input. The module does not carry a type's name, so this is a
convention of the canonical sources and reaches no byte.

## Appendix A. Wire encoding of the additions (`Ark.Encode`)

Every node is a struct with a `"t"` tag, as today. Field order in a struct
is irrelevant (canonical CBOR sorts keys); listed here in the order the
reference writes them.

```
module      : + ("routers", [router])                        -- after "functions"
router      : {"t":"router","name":txt,"uses":[txt]}
fn          : "kind" ∈ "mutator"|"query"|"helper"|"guard"|"provide"
              - "scope"                                       -- gone in version 3
              + ("router", txt | null)                        -- after "kind"
              + ("uses", [txt])                               -- after "router"; [] when not a procedure
              "args" becomes "input": [field]
              + ("refine", [{"t":"refine","e":expr,"why":txt|null}])   -- after "input"
field       : {"t":"field","name":txt,"ty":ty,"checks":[check]}
check       : {"t":"trim"}
            | {"t":"min_len","n":int,"why":txt|null}
            | {"t":"max_len","n":int,"why":txt|null}
            | {"t":"range","lo":int|null,"hi":int|null,"why":txt|null}
            | {"t":"non_empty","why":txt|null}
            | {"t":"exists","why":txt|null}
            | {"t":"refine","e":expr,"why":txt|null}
stmt        : {"t":"insert","table":txt,"row":expr,"on":[txt]}
            | {"t":"upsert","table":txt,"row":expr,"on":[txt]}
            | {"t":"update","table":txt,"key":[expr],"sym":int,"row":expr}
            -- "put" is gone
expr        : + {"t":"provided","fn":txt}
```

`docs/plan-guards.md` D1 adds one expression, written only where a module
has one, so every module and closure before it keeps its bytes:

```
expr        : + {"t":"has_role","role":txt}   -- a Bool; the role is not empty (EmptyRole)
```

It reads no table and its value is a function of the entry alone: the
roles frozen in it, which the authority stamped with the connection's at
sequencing. A guard written with it — `guard("is_library", |ctx, _|
unless(ctx.has_role("library"), || refuse("…")))` — refuses on the device
whose sign-in did not say the role, and at the server for a device that
believed one its login was never given.

`docs/plan-guards.md` D2 adds a kind and two predicate leaves, each
written only where a module has a scope, so every module before it keeps
its bytes:

```
fn          : "kind" + "scope"
              + ("holds", [hold])             -- a scope's, and only a scope's
hold        : {"t":"hold","table":txt,"filter":pred|null}
              + ("pick", [txt]) | ("exclude", [txt])   -- only when not every column
pred        : + {"t":"pwhen","e":expr}         -- a Bool of the context; a scope only
            | + {"t":"pexists","table":txt,"column":txt,"pred":pred}   -- a scope only
```

The `"scope"` of version 2, gone in version 3, named something else; a
version-4 module never carried the word, so it is free.

`docs/plan-guards.md` D3 adds a statement and a flag, each written only
where a mutator has a server half, so every module before it keeps its
bytes:

```
fn          : + ("private", true)                  -- present only when true; hashed
stmt        : + {"t":"private","body":[stmt]}      -- the server's module only
```

A function's hash is taken of it with every `private` statement removed
and its symbols numbered again, `private: true` kept: the server's
function and the client's stripped one are one hash.

Symbols
inside a `check`'s or `refine`'s expression are numbered in the same walk
as the body, before it (input, then refine, then body), so one numbering
covers the function. `EArg` inside a field's check refers to that field
after any `trim` before it.

`docs/plan-live.md` adds the type section and live states, each written
only where a module declares one, so every module before it keeps its
bytes:

```
module      : + ("types", [type])          -- only when not empty
              + ("states", [state])         -- only when not empty
type        : {"t":"type","name":txt,"ty":ty}
state       : {"t":"state","name":txt,"deps":{txt:bytes},"ty":ty,"init":expr,"scope":expr,
               "transitions":[transition]}
              + ("on_join", transition) | ("on_leave", transition) | ("on_restore", transition)
                                             -- each only where declared
transition  : {"t":"transition","name":txt,"input":[field],"guards":[guard],"body":[stmt]}
guard       : {"t":"guard","e":expr,"why":txt}
ty          : + {"t":"named","name":txt}
            | + {"t":"non_empty_list","of":ty}
            | + {"t":"sum","variants":{txt:{txt:ty}}}     -- each variant its record's fields
            | + {"t":"per_device","of":ty}
            | + {"t":"ephemeral","of":ty}                 -- a field of a state's record only
expr        : + {"t":"variant","ty":ty,"name":txt,"fields":{txt:expr}}
            | + {"t":"case","e":expr,"arms":[{"t":"arm","name":txt,"sym":int,"e":expr}]}
            | + {"t":"live_state"} | {"t":"ctx_now"} | {"t":"ctx_device"}
            | + {"t":"slot","e":expr,"key":expr}
            | + {"t":"set_slot","e":expr,"key":expr,"v":expr}   -- v an option; none empties it
            | + {"t":"slots","e":expr}
```

A state's `deps` are written in its closure, empty in the module, as a
function's are. A hook's input is the one field `device` (`ir::device_ty`),
`on_restore`'s none. Symbols are numbered per transition, input then
guards then body. The value of a sum is a struct of its variant's fields
with the variant's name under `t`, so no variant has a field `t`; the
value of a `PerDevice` is a struct keyed by device id.

### Spec version 4 (`docs/plan-v4.md` §1.8)

```
fn          : + ("plan", plan)             -- only when present: a query's
plan        : {"t":"plan","table":txt,"filter":pred|null,"order":[by],"limit":int|null,
               "related":[related]}        -- v3's keys, always
              + ("group", [txt])           -- a group source; "table" is the grouped table
              + ("row", int)               -- the row binder, when the plan binds anything
              + ("members", int)           -- a group's rows
              + ("lookups", [lookup])
              + ("having", expr)
              + ("project", expr)          -- each only when present
by          : {"t":"by","column":txt,"dir":"asc"|"desc"}
            | {"t":"by","expr":expr,"dir":"asc"|"desc"}
lookup      : {"t":"lookup","name":txt,"sym":int,"table":txt,"key":[expr]}
related     : {"t":"related","name":txt,"sym":int,"on":[[txt, expr]],"plan":plan}
              -- "parent", "child", "column" are gone: a related plan is always the on form
pred        : + {"t":"phas","column":txt,"e":expr}   -- `docs/plan-db.md` D4, only where used
index       : + ("kind", "text")           -- a text index, on one column, not unique; after
                                              the table's other indexes, only where declared
```

A `phas` and an index of kind `text` are written only where a module has
one, so every module before D4 — every retained closure, every vector —
keeps its bytes; a text index is additive under `compat` and is in the
schema, which is in no closure.

**The row binder, and why it may be absent.** `row` is written, and
numbered, exactly when the plan binds something: a lookup, a related plan,
a having, a projection, an expression order key or a group. A plan with
none of these — the v3 shape every mutator reads with — has no `row` in
the IR and none on the wire, and consumes no symbol, so its bytes and the
function's numbering are v3's and every mutator's closure hash is
unchanged; `rust/ark/tests/demo_authoring.rs` and
`harken/domain/tests/agreement.rs` pin those hashes to their v3 values.
The verifier refuses a plan that binds something without a row
(`NoRowBinder`). This is how `docs/plan-v4.md` §1.8's "`row`'s symbol is
required" is read: required wherever something can reference it, and
absent where nothing can, which is what keeps retained entries' hashes
where they were.

A plan's binders are numbered in its evaluation order, within the one walk
of the function: the filter (in the scope around the plan), `row`,
`members`, each lookup's key and then its symbol, each related plan's
`on`, its child plan (in the scope around the parent — scope is flat per
node) and then its symbol, `having`, `project`, the expression order keys.
A query's plan is walked after its input's checks and refinements.

## Appendix B. The demo, in the vocabulary

The playlist demo every vector runs over, whose emit is
`spec/vectors/module/demo.json`: the tables `playlist(id, name, user_id)`
and `item(playlist_id → playlist, track_id: text, pos)`, keyed by `id` and
by `(playlist_id, track_id)`, with unique `(playlist.user_id,
playlist.name)` and `(item.playlist_id, item.pos)`; the mutators
`create_playlist` and `add_to_playlist`; and the query `items`, a plan.
It is written three times, identically — `rust/ark-client/src/demo.rs`
(what `ark-client`'s and `ark-server`'s tests run against),
`rust/ark/src/bin/vectors/demo.rs` (the generator's, since a binary of
`ark` cannot depend on `ark-client`) and `rust/ark/tests/demo_authoring.rs`
— and check 1 of §5 holds the copies to one set of bytes. Its tables and
rows:

```rust
pub struct Demo {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
}
impl Tables for Demo {
    fn open() -> Self {
        Demo {
            playlist: table(),
            item: table(),
        }
    }
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .text(Self::user_id)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
#[allow(non_upper_case_globals)]
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const user_id: Col<Self, Text> = col("user_id");
    pub const item: Rel<Self, Item> = rel("item");
}

pub struct Item {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
    pub pos: Int,
}
impl Row for Item {
    const NAME: &str = "item";
    type Key = (Id<Playlist>, Text);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .text(Self::track_id)
            .int(Self::pos)
            .key((Self::playlist_id, Self::track_id))
            .unique((Self::playlist_id, Self::pos))
    }
}
#[allow(non_upper_case_globals)]
impl Item {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const track_id: Col<Self, Text> = col("track_id");
    pub const pos: Col<Self, Int> = col("pos");
}
```

its inputs:

```rust
pub struct CreatePlaylist {
    pub name: Text,
}
impl Input for CreatePlaylist {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name"))
    }
}

pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
}
impl Input for AddToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("track_id", text().min(1))
    }
}

pub struct PlaylistId {
    pub playlist_id: Id<Playlist>,
}
impl Input for PlaylistId {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}
```

and its router:

```rust
pub fn demo() -> Router<Demo> {
    let demo = router::<Demo>("demo");
    demo.routes((
        demo.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
            db.playlist
                .insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    user_id: ctx.user,
                })
                .on((Playlist::user_id, Playlist::name))
        }),
        demo.input::<AddToPlaylist>().mutation("add_to_playlist", |_ctx, db, input| {
            let last = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
            db.item.insert(Item {
                playlist_id: input.playlist_id,
                track_id: input.track_id,
                pos: last.map_or(0, |row| row.pos).add(1),
            })
        }),
        demo.input::<PlaylistId>().query("items", |_ctx, db, input| {
            db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.asc())
        }),
    ))
}

pub fn module() -> Module {
    Module::new((demo(),))
}
```

`add_to_playlist` reads with a v3-shaped plan — a constant filter, an
order by a column, `first()` — which is all a mutator's read may be
(§1.5), so its closure hash is the one it had at spec v3. `items` returns
the `Query` itself: no `.all()`, no statement, and its value is the plan
`view::pull` evaluates and a client keeps as a view. A domain written
elsewhere may name its input types otherwise — a name reaches no byte —
but its `emit` is held to the vector's bytes.

### B.2 The room

The demo's live state, which every live vector is over
(`spec/vectors/module/live.json`, `eval/live-room.json`): a `room` per
account, on a router of its own, `now`, declared after the demo's so that
the demo's module and every vector of it keep their bytes —
`Module::new((demo(), live::<Demo>()))`. It counts (`bump`, ordered: it reads the
count); is set counting by a device and stopped (`start` reads the server's
clock and the caller's device; `stop` is guarded); keeps who is here in an
ordered `PerDevice`, by its hooks; and carries each device's pointer and a
label in ephemeral ones — `point` and `label` the ephemeral transitions,
`drop_at` the ordered one that also clears its caller's pointer. Restored
at start it is still counted, not counting, and nobody is in it: whoever
was here when the server stopped never left, and only the app knows
`devices` is who is here. It is written three times, identically, as the
demo is — `rust/ark/src/bin/vectors/demo_live.rs`, `rust/ark/tests/live.rs`
(through `#[path]`) and `rust/ark-client/src/demo.rs` — and held to one set
of bytes by `module/live.json`.

```rust
/// The room: a record, named in the type section.
pub struct Room {
    pub count: Int,
    pub devices: PerDevice<Device>,
    pub label: PerDevice<Text>,
    pub mode: Sum<Mode>,
    pub pointer: PerDevice<Int>,
}
impl Record for Room {
    fn fields() -> Fields<Self> {
        fields()
            .named("room")
            .field("count", int())
            .field("devices", typed::<PerDevice<Device>>())
            .ephemeral("label", typed::<PerDevice<Text>>())
            .field("mode", typed::<Sum<Mode>>())
            .ephemeral("pointer", typed::<PerDevice<Int>>())
    }
}

/// Whether the room is counting, and since when, by whom.
pub struct Mode;
impl SumType for Mode {
    const NAME: &'static str = "mode";
    fn variants() -> Cases<Self> {
        cases().of::<Idle>("idle").of::<Counting>("counting")
    }
}

pub struct Idle {}
impl Record for Idle {
    fn fields() -> Fields<Self> {
        fields()
    }
}

pub struct Counting {
    pub by: Text,
    pub since: Int,
}
impl Record for Counting {
    fn fields() -> Fields<Self> {
        fields().field("by", text()).field("since", int())
    }
}

pub struct Bump {
    pub by: Int,
}
impl Input for Bump {
    fn schema() -> Object<Self> {
        object().field("by", int().range(1, 10))
    }
}

pub struct Say {
    pub text: Text,
}
impl Input for Say {
    fn schema() -> Object<Self> {
        object().field("text", text())
    }
}

pub struct At {
    pub at: Int,
}
impl Input for At {
    fn schema() -> Object<Self> {
        object().field("at", int())
    }
}

/// The router that declares the room.
pub fn live<S: Tables>() -> Router<S> {
    let now = router::<S>("now");
    now.live::<Room>("room")
        .scope(|ctx| ctx.user)
        .init(|| Room {
            count: 0.into(),
            devices: PerDevice::empty(),
            label: PerDevice::empty(),
            mode: Sum::of("idle", Idle {}),
            pointer: PerDevice::empty(),
        })
        // Ordered: reads the count.
        .transition::<Bump>("bump", |s, _ctx, input| Room {
            count: s.count.add(input.by),
            ..s
        })
        // Ordered: reads the server's clock and the caller's device.
        .transition::<()>("start", |s, ctx, ()| Room {
            mode: Sum::of(
                "counting",
                Counting {
                    by: ctx.device,
                    since: ctx.now,
                },
            ),
            ..s
        })
        .transition::<()>("stop", |s, _ctx, ()| Room {
            mode: Sum::of("idle", Idle {}),
            ..s
        })
        .guard::<()>("not counting", |s, _ctx, ()| s.mode.is("counting"))
        // Ephemeral: the caller's own pointer, and nothing read.
        .transition::<At>("point", |s, ctx, input| Room {
            pointer: s.pointer.put(ctx.device, input.at),
            ..s
        })
        // Ephemeral too: what the device says beside its pointer, held to
        // the slot's size.
        .transition::<Say>("label", |s, ctx, input| Room {
            label: s.label.put(ctx.device, input.text),
            ..s
        })
        // Ordered, and writes the caller's pointer: a drop clears it.
        .transition::<At>("drop_at", |s, ctx, input| Room {
            count: input.at,
            pointer: s.pointer.clear(ctx.device),
            ..s
        })
        .on_join(|s, _ctx, device| Room {
            devices: s.devices.put(device.id, device),
            ..s
        })
        .on_leave(|s, _ctx, device| Room {
            devices: s.devices.clear(device.id),
            ..s
        })
        // What a room restored at start is: still counted, not counting, and
        // nobody in it — whoever was here when the server stopped never left,
        // and only the app knows `devices` is who is here.
        .on_restore(|s, _ctx| Room {
            devices: PerDevice::empty(),
            mode: Sum::of("idle", Idle {}),
            ..s
        });
    now
}
```
