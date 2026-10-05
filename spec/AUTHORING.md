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
Kotlin authored the same vocabulary at version 3 and are frozen there
(`swift/`, `kotlin/`); this document describes the Rust spelling only.
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
  `CtxSession`, `Provided`, literals, operators, `Std`, `Call` of a helper
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
| `Std IsSome/UnwrapOr/Unwrap` | `.is_some()` `.unwrap_or(d)`; `.or_refuse("why")` (statements, in a body) and `.unwrap()` (an expression, for a projection behind a `having` that holds `is_some()`) |
| `Match` | `.map_or(d, \|x\| e)` and `.map(\|x\| e)` (option) |
| `Map/Filter/Any/All/SortBy` | `.map(\|x\| e)` `.filter(\|x\| e)` `.any(..)` `.all(..)` `.sort_by(..)` |
| `Fold` | `.fold(init, \|acc, x\| e)` |
| `Some/None` | `some(x)` `none::<T>()` |
| `Struct/Field` | struct literal / `row.field` |
| `List` | `list([a, b])` |
| `Call` | `helper_name(args)` (a Rust fn) |
| `CtxUser/CtxSession` | `ctx.user` `ctx.session` |
| `Auto` | `ctx.now("added_ms")` `ctx.new_id("id")` |
| `Provided` | the extra body parameter |

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

A table may say who sees its rows and who writes them (`docs/plan-auth.md`):

```rust
columns()
    // …the columns, the key, the indexes…
    .visible(Self::user_id.is(Me).or(exists(PlaylistMember::playlist_id, PlaylistMember::user_id.is(Me))))
    .writable(Self::user_id.is(Me))          // or Role("library"), or Everyone
```

A rule is a predicate over the table's own columns, as a filter is, with
three things a filter does not have: `Me`, the user the rule is asked about
(`c.is(Me)` on a text column — the author, for `writable`; the peer being
served, for `visible`); `Role("name")`, which the identity holds or does
not (`Pred::from(Role(..))` to combine one with `.and`/`.or`); and the one
lookup, `exists(Child::fk, pred)` — some row of another table whose
reference column names this row, admitted by a predicate over that table's
own columns. A lookup reaches one table and no further. The default,
`Everyone`, is not written down at all, so a table that declares nothing is
the bytes it always was. A rule is in the module's hash and in no closure,
and `arkc check` does not compare rules: they decide what a connection is
sent and what the authority sequences from here on, never what a retained
entry means. The engine enforces them and decides none — the app declares
them.

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
  returns the entry's auto of that name. Only mutators and middleware run
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
- `arkc hash M`: the module hash and every function's hash.
- `arkc check OLD NEW`: log compatibility (spec §17, `ark::compat`) — every
  break, and exit 1; or nothing. Run it between the committed module and a
  new one before a change that could move a retained entry's meaning.
- `arkc vectors OUTDIR`: write the conformance vectors, as `ark-vectors`
  does.

There is no `print`, `gen` or `roundtrip`: they existed to hand a phone a
domain in its own language, and with Swift and Kotlin frozen at spec v3
there is nobody to print for (`docs/plan-v4.md`, decision 4). A domain's
source is the Rust a person wrote; its `.ark` is what that source emits.

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
| `let x = e` | nothing: a read is a `Let` by itself and a pure expression is inlined where it is used |
| `input.f` | `Arg("f")` |
| the provided parameter | `Provided("<middleware name>")` |
| `ctx.user` / `ctx.session` | `CtxUser` / `CtxSession` |
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

Symbols
inside a `check`'s or `refine`'s expression are numbered in the same walk
as the body, before it (input, then refine, then body), so one numbering
covers the function. `EArg` inside a field's check refers to that field
after any `trim` before it.

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
            + {"t":"prole","name":txt}           -- `docs/plan-auth.md`, only in a rule
            + {"t":"pexists","table":txt,"column":txt,"pred":pred}   -- the one lookup, only in a rule
table       : + ("visible", pred) + ("writable", pred)   -- each only where declared; `Me` is
                                              {"t":"ctx_user"} as a comparison's right-hand side
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
