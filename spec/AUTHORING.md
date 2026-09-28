# Authoring: one vocabulary, three spellings

This is the contract between the specification, the three runtimes, `arkc`,
and every domain. It replaces `GENERATED.md`: nothing is generated *from*
the IR into a runtime's private shape any more. A domain is written in
Rust, Swift or Kotlin against the vocabulary below; running that program
under `Emit` yields the module (`.ark`); running it under `Native` applies
entries directly, so the language a peer is written in is the language its
domain runs in, with no code generated for it. `arkc gen <lang>` is a
pretty-printer from the module to this same vocabulary in that language,
and the property held by the checks is the round trip:

    hash(emit(gen(lang, m))) == hash(m)         for every function, every lang
    gen(lang, emit(src)) == src                  for a source file in canonical form

The hash is over the IR (`Ark.Hash`), so a function written in Swift and
its Rust print are the same function. Names of `let` bindings and closure
parameters travel in `fnNames`, outside the hash, so the print reads as the
author wrote it.

Spec version **3**. (Version 2 had scopes; `docs/scopes.md` says what they
were and why they are gone.) Section numbers refer to `spec/README.md`.

## 1. IR additions (`Ark.IR`, `Ark.Encode`, `Ark.Decode`)

### 1.1 Routers

```haskell
data Router = Router
  { rtName  :: Text          -- "playlists"
  , rtUses  :: [Text]        -- the middleware declared on it, in declaration order
  }
-- Module gains:  modRouters :: [Router]
-- Function gains: fnRouter :: Maybe Text   -- Nothing for helpers and middleware
--                 fnUses   :: [Text]       -- the middleware THIS procedure runs, in order;
--                                          -- a subsequence of its router's rtUses. [] for
--                                          -- helpers and middleware
```

A procedure runs the chain it was built from and not every middleware its
router declares: in harken `create_playlist` is `signed_in.input(..)` and
`add_to_playlist` is `owned.input(..)` where `owned` was built on
`signed_in`, so the first has `fnUses = ["signed_in"]` and the second
`["signed_in", "owned"]`. `rtUses` is the union in declaration order and a
procedure's `fnUses` must be a subsequence of it (verifier: `UsesNotOnRouter`).

A router is a group of procedures and the middleware chains built on them;
it names no tables. Any procedure, query or middleware may read and write
any table of the module. Helpers have no router.

### 1.2 Middleware

```haskell
data FnKind = Mutator | Query | Helper
            | Guard    -- runs before the body; may refuse; returns nothing
            | Provide  -- runs before the body; may refuse; returns fnRet, which
                       -- the body reads as  EProvided <middleware name>
```

A middleware function has `fnInput` (the fields of the
procedure's input it reads, by name and type — the verifier requires every
procedure using it to have those fields with those types), a body over the
same `db`, and for `Provide` an `fnRet`. `Expr` gains `EProvided Text`.

Evaluation order for a procedure: decode input → input checks (1.3) → each
middleware in `fnUses` order (a refusal stops there) → body. All of it is
inside the transaction; a refusal anywhere leaves the store untouched.

The closure of a procedure (`Ark.Hash.closure`) includes its middleware and
the helpers they reach, so editing a middleware re-hashes every procedure
on its router. That is the intended meaning.

### 1.3 Input schema and checks

```haskell
-- Function: fnArgs :: [(Text, Ty)]  becomes
--           fnInput :: [(Text, Field)]
data Field = Field { fTy :: Ty, fChecks :: [Check] }
data Check
  = CTrim                              -- text: normalise before every later check and before the body
  | CMinLen Int (Maybe Text)           -- text: length in code points ≥ n
  | CMaxLen Int (Maybe Text)
  | CRange (Maybe Int) (Maybe Int) (Maybe Text)   -- int: lo ≤ v ≤ hi, either bound optional
  | CNonEmpty (Maybe Text)             -- list: at least one element
  | CExists (Maybe Text)               -- id: a row with that key exists
  | CRefine Expr (Maybe Text)          -- any: the expression, over EArg <this field>, is true
-- Function gains: fnRefine :: [(Expr, Maybe Text)]   -- over the whole input, after the fields
```

`Maybe Text` is the message; `Nothing` means the default, which the
reference defines (`Ark.Eval.defaultMessage`) and every runtime copies:

| check | default message |
|---|---|
| `CMinLen n` | `<field>: at least <n> characters` |
| `CMaxLen n` | `<field>: at most <n> characters` |
| `CRange lo hi` | `<field>: between <lo> and <hi>` (or `at least`/`at most` with one bound) |
| `CNonEmpty` | `<field>: at least one` |
| `CExists` | `<field>: no such <table>` |
| `CRefine` | `<field>: invalid` |

A failing check is a **refusal** with that message, for mutators and for
queries alike (`Ark.Eval.queryClosure` returns `Either Refusal Value` now).
A field of type `TOption t` applies its checks when the value is `Some`.

Runtimes expose the same walk as a **form validator**: `check(schema,
partial input, db) -> [(field, message)]`, running each field's checks on
the fields present, with `CTrim` applied first and the normalised value
returned beside the messages. `fnRefine` runs only when every field is
present.

### 1.4 Table writes

`SPut` is gone. In its place:

```haskell
  | SInsert TableName Expr [FieldName]   -- write the row unless one matches on the columns
  | SUpsert TableName Expr [FieldName]   -- write the row; if one matches on the columns,
                                          --   keep its key columns and take the rest from the new row
  | SUpdate TableName [Expr] Sym Expr    -- key; the existing row bound to Sym; the new row.
                                          --   A no-op when absent
```

An empty column list means the table's key. A non-empty one must be a
declared **unique index** of the table (`Index { unique = True }`), which
is what lets a store answer the match with one lookup; otherwise a
verifier error (`OnNotUnique`). Every write reports `Add`, `Edit` or
nothing as `SPut` did, refuses on a constraint as `SPut` did, and `SUpsert
t e []` is exactly the old `SPut t e`.

### 1.5 Everything else is unchanged

`Sym`, `SLet` (reads still only as a whole right-hand side), `SIf`, `SFor`,
`SDelete`, `SRefuse`, `SReturn`, all of `Expr`, `Plan`, `Pred`, `StdFn`,
`Auto`, `fnAutos`, `fnNames`, the schema, the log, the peer, the protocol,
`live`. The verifier gains: routers (names unique, uses are
middleware), middleware input compatibility, `CExists` on an id,
`.on` uniqueness, `EProvided` only of a `Provide` in the function's own `fnUses`.

## 2. The vocabulary

One name per IR node. The printer writes exactly these; a builder accepts
exactly these (Rust may also accept `+`, `==` and friends as sugar and they
print as the methods). Identifiers are `snake_case` in Rust and
`lowerCamel` in Swift and Kotlin, mechanically; string names in the IR are
always `snake_case`.

### 2.1 Values

| type | is | Rust | Swift | Kotlin |
|---|---|---|---|---|
| bool | `TBool` | `Bool` | `Bool` | `Bool` |
| int | `TInt` | `Int` | `Int` | `Int` |
| text | `TText` | `Text` | `Text` | `Text` |
| bytes | `TBytes` | `Bytes` | `Bytes` | `Bytes` |
| id of T | `TId t` | `Id<T>` | `Id<T>` | `Id<T>` |
| option | `TOption` | `Opt<T>` | `Opt<T>` | `Opt<T>` |
| list | `TList` | `List<T>` | `List<T>` | `List<T>` |
| row of T | struct | `T` | `T` | `T` |

These are the *value types of the vocabulary*, distinct from the host
language's `bool`/`Int64`/`String`: under `Native` they carry data, under
`Emit` an expression. A host `if` on one does not compile. Literals lift:
`0`, `"a playlist needs a name"`, `true` where an `Int`, `Text`, `Bool` is
expected (`From`/`ExpressibleBy…Literal`/overloads).

### 2.2 Operations (methods on values)

| IR | Rust | Swift | Kotlin |
|---|---|---|---|
| `ECmp Eq/Ne/Lt/Le/Gt/Ge` | `.eq(b)` `.ne` `.lt` `.le` `.gt` `.ge` | same | same |
| `EOp Add/Sub/Mul/Div/Mod/Neg` | `.add(b)` `.sub` `.mul` `.div` `.rem` `.neg()` | same | same |
| `EOp And/Or/Not` | `.and(b)` `.or(b)` `.not()` | same | same |
| `EStd Trim/IsEmpty/Lower/TextLen/…` | `.trim()` `.is_empty()` `.lower()` `.len()` `.starts_with(p)` `.split_once(p)` `.chars()` `.is_alnum()` `.utf8()` | `.isEmpty()` `.startsWith` `.splitOnce` `.isAlnum` | same as Swift |
| `EStd Concat` | `concat(list)` | `concat(list)` | `concat(list)` |
| `EStd TextOfInt/TextOfId/IdOfText/Hex/Fnv1a64/Sha256/NilId` | `.to_text()` `.to_text()` `id_of_text(t)` `.hex()` `.fnv1a64()` `.sha256()` `nil_id()` | `.toText()` … `idOfText` `nilId()` | same as Swift |
| `EStd Min/Max/Clamp/Abs` | `.min(b)` `.max(b)` `.clamp(lo, hi)` `.abs()` | same | same |
| `EStd First/Last/Len/Contains/Reverse` | `.first()` `.last()` `.len()` `.contains(x)` `.reverse()` | same | same |
| `EStd IsSome/UnwrapOr/Unwrap` | `.is_some()` `.unwrap_or(d)` `.or_refuse("why")` | `.isSome()` `.unwrapOr(d)` `.orRefuse("why")` | same as Swift |
| `EMatch` | `.map_or(d, \|x\| e)` and `.map(\|x\| e)` (option) | `.mapOr(d) { x in e }` `.map { x in e }` | `.mapOr(d) { x -> e }` `.map { x -> e }` |
| `EMap/EFilter/EAny/EAll/ESortBy` | `.map(\|x\| e)` `.filter(\|x\| e)` `.any(..)` `.all(..)` `.sort_by(..)` | `.map { x in e }` … `.sortBy` | `.map { x -> e }` … `.sortBy` |
| `EFold` | `.fold(init, \|acc, x\| e)` | `.fold(init) { acc, x in e }` | `.fold(init) { acc, x -> e }` |
| `ESome/ENone` | `some(x)` `none::<T>()` | `some(x)` `none(T.self)` | `some(x)` `none<T>()` |
| `EStruct/EField` | struct literal / `row.field` | init / `row.field` | constructor / `row.field` |
| `EList` | `list([a, b])` | `list([a, b])` | `list(a, b)` |
| `ECall` | `helper_name(args)` (a Rust fn) | `helperName(args)` | `helperName(args)` |
| `ECtxUser/ECtxSession` | `ctx.user` `ctx.session` | same | same |
| `EAuto` | `ctx.now("added_ms")` `ctx.new_id("id")` | `ctx.now("added_ms")` `ctx.newId("id")` | same as Swift |
| `EProvided` | the extra body parameter | same | same |

### 2.3 Tables (`db.<table>` is a `Table<Row>`)

| IR | Rust | Swift | Kotlin |
|---|---|---|---|
| `EGet t k` | `db.playlist.get((id,))` → `Opt<Playlist>` | `db.playlist.get(id)` | same |
| `EExists t k` | `db.playlist.exists((id,))` → `Bool` | `.exists(id)` | same |
| `ESelect` | `db.playlist.filter(p).order_by(o).limit(n).all()` → `List<Playlist>` | `.filter(p).orderBy(o).limit(n).all()` | same as Swift |
| `ESelect` + `First` | `…​.first()` → `Opt<Playlist>` | same | same |
| `ESelect` with `pRelated` | `.with(Playlist::items)` | `.with(Playlist.items)` | same |
| `SInsert` | `db.playlist.insert(row)` / `.insert(row).on((Playlist::user_id, Playlist::name))` | `.insert(row)` / `.insert(row).on(Playlist.userId, Playlist.name)` | same as Swift |
| `SUpsert` | `db.playlist.upsert(row)` / `.on(..)` | same | same |
| `SUpdate` | `db.playlist.update((id,), \|row\| Playlist { .. })` | `.update(id) { row in Playlist(..) }` | `.update(id) { row -> Playlist(..) }` |
| `SDelete` | `db.playlist.delete((id,))` | `.delete(id)` | same |
| `Pred` | `Playlist::user_id.eq(x)` `.and(..)` `.or(..)` `.not()` `Playlist::id.in_(list)` | `Playlist.userId.eq(x)` … `.isIn(list)` | same as Swift |
| order | `Playlist::name.asc()` `.desc()`; several as a tuple | same | same |

A key is a tuple in Rust, positional arguments in Swift and Kotlin; its
shape is the row's `Key` type, so a wrong order does not compile.

### 2.4 Control (all return a value; bodies are closures)

| IR | Rust | Swift | Kotlin |
|---|---|---|---|
| `SIf c t []` | `when(c, \|\| t)` | `when(c) { t }` | `when(c) { t }` |
| `SIf c [] e` | `unless(c, \|\| e)` | `unless(c) { e }` | same |
| `SIf c t e` | `if_else(c, \|\| t, \|\| e)` | `ifElse(c, then: { t }, else: { e })` | `ifElse(c, { t }, { e })` |
| `EIf` | `pick(c, a, b)` | `pick(c, a, b)` | same |
| `SFor` | `for_each(xs, \|x\| body)` | `forEach(xs) { x in body }` | `forEach(xs) { x -> body }` |
| `SRefuse` | `refuse("why")` | `refuse("why")` | same |
| `SReturn` | the closure's value | the closure's value | same |
| `SLet` | `let x = …` (a read becomes an `SLet` at once) | `let x = …` | `val x = …` |

A mutator body's value is `Effect`; a query's is its return type. A helper
is an ordinary function of the vocabulary's types and is emitted when
called.

### 2.5 Declarations

Rust (what `arkc gen rust` writes, after rustfmt; one file per router, the
schema in `schema.rs`, the module in `module.rs`, and a `lib.rs` of the
crate's own naming them):

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
// written is the schema's table order: no host can enumerate a struct's
// fields, so the tables say them once, here. `arkc gen rust M OUTDIR --name Harken` names
// the struct (default `Tables`). Swift and Kotlin
// spell the same constructor in their own declarations.

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
`.nullable()` after a column, `.refs::<Parent>()` after an id column,
`.index((..))`.

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

pub struct CreatePlaylist {
    pub name: Text,
}
impl Input for CreatePlaylist {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name").max(120))
    }
}

pub fn playlists() -> Router<Harken> {
    let playlists = router::<Harken>("playlists");
    let signed_in = playlists.guard("signed_in", |ctx, _db| when(ctx.user.is_empty(), || refuse("sign in first")));
    let owned = signed_in.provide("owned", |ctx, db, input: &Owned| {
        db.playlist
            .get((input.playlist_id,))
            .filter(|row| row.user_id.eq(ctx.user))
            .or_refuse("not your playlist")
    });
    playlists.routes((
        signed_in.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| { .. }),
        owned.input::<AddToPlaylist>().mutation("add_to_playlist", |ctx, db, input, playlist| { .. }),
        signed_in.query("playlists", |ctx, db, _input: ()| { .. }),
        owned.input::<PlaylistItems>().query("playlist_items", |_ctx, db, _input, playlist| { .. }),
    ))
}
```

Field builders: `text()`, `int()`, `bool_()`, `bytes()`, `id::<T>()`,
`enum_::<E>()`, `opt(f)`, `list(f)`; checks `.trim()`, `.min(n)`,
`.max(n)`, `.range(lo, hi)`, `.at_least(lo)`, `.at_most(hi)`,
`.non_empty()`, `.exists()`, `.refine(|v| ..)`; and `.why("…")` after any
check but `.trim()` to give it its message (Rust cannot overload by arity,
so this is the one spelling in every language); `object().refine(|input|
..).why("…")` over the whole input.

A router file also declares the module's **helpers** and **records**, after
its inputs and before the router function:

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

pub fn slug(text: Text) -> Text {            // a helper of one parameter
    helper("slug", ("text", text), |text: Text| {
        concat(text.chars().map(|x| pick(x.is_alnum(), x.lower(), " ")))
            .trim()
            .chars()
            .fold("", |acc: Text, x| { .. })
    })
}

pub fn movement_key(work_id: Text, no: Int) -> Text {
    helper("movement_key", (("work_id", work_id), ("no", no)), |work_id: Text, no: Int| {
        concat(list([work_id, "#".into(), no.to_text()]))
    })
}

pub fn library_entry(media: Media, items: List<PlaylistItem>) -> LibraryEntry {
    helper(
        "library_entry",
        (("media", media), ("items", items)),
        |media: Media, items: List<PlaylistItem>| LibraryEntry { added_ms: media.added_ms, /* … */ },
    )
}
```

The rules `arkc gen rust` prints these by, each deterministic:

- **A helper** is `pub fn name(a: A, ..) -> R { helper("name", PARAMS, |a: A, ..| body) }`:
  `PARAMS` is the bare pair `("a", a)` for one parameter and a tuple of
  pairs for several; the closure's parameters are the helper's, under the
  same names and typed (a closure passed through a trait bound cannot have
  them inferred). Inside the body a parameter is its bare name, where a
  procedure's input is `input.f`. A call is `name(args)`.
- **A record** is a struct type some function returns that is not a row.
  It is named after the first function in module order whose result it
  is, in PascalCase, with `Entry` appended when that result is a list of
  it (`LibraryEntry` for the helper `library_entry`, `AlbumsEntry` for the
  query `albums`, `WorkSummary` for the helper `work_summary`); a name
  already a row's, an input's or an earlier record's gets `Record`
  appended. Its fields, in the struct and in `fields()`, are in the IR's
  order, which is alphabetical; a record literal is `Name { f: v, .. }` in
  that order too, where a row literal keeps its table's column order.
- **Where each goes.** A procedure is in its router's file, a middleware in
  the file of the router that declares it, and a helper in the file of the
  first function after it in module order that is not a helper — which is
  its first caller's, since a helper is placed immediately before that
  (§6). A record is declared in the file of the function that names it.
  A file is: `use ark::authoring::*;`, then one `use` group of
  `use crate::schema::*;` and a `use crate::<file>::<name>;` for every
  helper its functions call and every record its text names that another
  file declares (`use crate::library::library_entry;` in `playlists.rs`),
  as rustfmt sorts them; then its inputs, its records and its helpers, each
  in module order, and last the router.
- **A literal is lifted with `.into()`** where the vocabulary takes exactly
  a type rather than anything that converts to it: a row's or a record's
  field (`kind: "song".into()`, `recorded: 0.into()`), a list's element
  (`list(["no work ".into(), input.id])`), a helper's argument. Everywhere
  else — a method's argument, `pick`, `some`, `map_or`'s default, `fold`'s
  start, `refuse` — it is written bare.
- **`pick::<T>(..)`** where a pick is passed straight to another `pick` or
  to `some`: both take any value that converts, so nothing else would say
  which `T` the inner one is (`pick(performer.is_empty(), pick::<Text>(work_id.is_some(), "", artist), performer)`).
- **A fold's accumulator is typed** (`.fold("", |acc: Text, x| ..)`), for
  the same reason: its start converts.
- **`x.is_none()`** is how `not (is_some x)` prints, `is_none` being that
  lowering; option comparisons are `.eq(some(v))`/`.ne(..)` like any other
  (`Opt::eq`, `Opt::ne`).
- **Names**: an `exists` read is named after its table like any other read
  (`let media = db.media.exists((input.media_id,));`); a closure parameter
  over a record is `row`, as over a row.

Swift and Kotlin have no spelling of helpers or records yet: `arkc gen
swift|kotlin` refuses a module that needs one, naming it.

The module: `Module::new((library(), playlists()))` with helpers found by
being called. `module().emit() -> ModuleBytes`, `module().procedures() ->
Vec<(FnHash, Procedure)>` for a runtime, and `module().hash()`.

**harken's domain is written out in full in this form** in
`harken/domain/src/{schema.rs,library.rs,playlists.rs,module.rs}`; those
four files are the canonical Rust text: the Rust builder compiles them,
their `emit()` is `harken/domain/harken.ark`, and `arkc roundtrip rust`
over that module reproduces them (comment-only lines aside).

Swift and Kotlin are the same declarations with `struct`/`class`,
`extension … : Row`/`companion object : Row.Of`, and
`static let id = col<Playlist, Id<Playlist>>("id")`/`val id = col<…>("id")`.
`harken/domain/gen/{swift,kotlin}` are `arkc gen` of harken's module,
restricted to what a phone carries; the two runtimes' authoring modules
(`ArkAuthoring`, `dev.arkdb.authoring`) define the vocabulary they are
written in. `GENERATED.md` and the `GENERATED-*.md` contracts are retired:
nothing is generated for a runtime.

## 3. Native and Emit

A builder library implements the vocabulary twice behind one API:

- **Emit**: every value holds an `Expr`; every `let` of a read is an
  `SLet`; every closure passed to `when`, `unless`, `if_else`, `for_each`,
  `map_or`, `map`, `filter`, `update` is run **once**, with fresh symbols
  for its parameters, to record its body; `ctx.now(name)` and
  `ctx.new_id(name)` register an auto (a repeated name is an error). The
  result is the `Function` with `fnNames` filled from the host's names
  where the host exposes them and from `_0`, `_1`… where it does not.
- **Native**: every value holds a `Value`; reads run at once against the
  transaction; `when` runs its closure only when the condition holds;
  `ctx.now(name)` returns the entry's auto of that name, which the peer
  filled at the origin from the function's declared list (so a draw
  inside an untaken branch is still filled, and replay reads the frozen
  one).

Both are selected by the context the procedure is run under; a body has
no way to tell which. `Native` must agree with `Ark.Eval` over the
function's own `Emit`; the runtime's tests hold it to that on every
procedure of the demo, and a peer may hold it at runtime in debug builds.

## 4. What `arkc` does now

- `arkc verify m.ark`, `print`, `hash`, `check OLD NEW`: as before.
- `arkc gen rust|swift|kotlin m.ark OUTDIR [--only f,g] [--package p]
  [--fmt CMD]`: writes the authoring form — `schema.<ext>`, one file per
  router, and the module file (`Schema.swift`, `Playlists.kt`… in Swift and
  Kotlin). `--only` keeps the named procedures and whatever they reach,
  and drops a router left with nothing; the schema is always whole.
  `--package` is the Kotlin package. `--fmt` runs a formatter over the
  files it wrote, which is what makes them canonical (§6, "Layout").
- `arkc roundtrip rust|swift|kotlin m.ark SRC_DIR [same options]`: gen into
  a temporary directory and compare with `SRC_DIR`, comment-only lines
  removed from both; exit 1 at the first line that differs, naming it.
  `nix flake check` runs it over harken's three domains (`checks.harken-domain`).

## 5. The checks that hold it together

1. `emit` of each language's demo domain equals `spec/vectors/module/demo.json`'s bytes (the demo is authored in all three now).
2. `arkc roundtrip <lang>` over each authored domain: harken's Rust over the
   whole module, its Swift and Kotlin over what a phone carries. (The three
   runtimes' demos live in their test suites and are held by check 1; they
   are not roundtripped.)
3. `Native` versus `Ark.Eval` on every procedure of the demo, per runtime.
4. The eval, verify and rebase vectors as before, regenerated at spec version 2.

## 6. Lowerings: what each spelling emits

The bytes of a module are a function of these rules and nothing else, so
every builder lowers the same way and every printer reads the same shapes
back. Symbols below are fresh; the printer recovers the author's names
from `fnNames`.

| spelling | emits |
|---|---|
| `db.t.filter(p).order_by(o).limit(n).all()` | `SLet s (ESelect plan)`; the value is `EVar s` |
| `….first()` | `SLet s (ESelect plan{limit = 1})`, `SLet s' (EStd First [EVar s])`; the value is `EVar s'` |
| `db.t.get(k)` / `db.t.exists(k)` | `SLet s (EGet t k)` / `SLet s (EExists t k)`; the value is `EVar s` |
| `opt.map_or(d, \|x\| e)` | `EMatch opt x e d` |
| `opt.map(\|x\| e)` | `EMatch opt x (ESome e) (ENone T)` |
| `opt.filter(\|x\| p)` | `EMatch opt x (EIf p (ESome (EVar x)) (ENone T)) (ENone T)` |
| `opt.or_refuse(msg)` | `SLet s opt`, `SIf (EStd IsSome [EVar s]) [] [SRefuse (ELit msg)]`; the value is `EStd Unwrap [EVar s]` |
| `opt.unwrap_or(d)` | `EStd UnwrapOr [opt, d]` |
| `when(c, \|\| body)` / `unless` / `if_else` | `SIf c then else`, with the closure's statements as the block(s) |
| `for_each(xs, \|x\| body)` | `SFor x xs body` |
| `refuse(msg)` | `SRefuse (ELit msg)` |
| `db.t.insert(row)` / `.on(cols)` | `SInsert t row []` / `SInsert t row cols` |
| `db.t.upsert(row)` / `.on(cols)` | `SUpsert t row []` / `SUpsert t row cols` |
| `db.t.update(k, \|row\| new)` | `SUpdate t k row new` |
| `db.t.delete(k)` | `SDelete t k` |
| `let x = e` | nothing: no host can see a `let`, so a read is an `SLet` by itself and a pure expression is inlined where it is used |
| `input.f` | `EArg "f"` |
| the provided parameter | `EProvided "<middleware name>"` |
| `ctx.user` / `ctx.session` | `ECtxUser` / `ECtxSession` |
| `ctx.now("n")` / `ctx.new_id("n")` | `EAuto "n"`, and `("n", Now)` / `("n", NewId t)` appended to `fnAutos` in the order the run meets them |
| a row literal | `EStruct` of every field written; the printer writes fields in the table's column order |

**Names are derived, never captured.** No host language lets a builder
see a `let`'s name or a closure parameter's, so `fnNames` may be left
empty by `Emit` and the printer names every binding by rule: a let-bound
read is named after its table (`item`, `playlist`), with `_2`, `_3`
appended for a second and third read of the same table in one function;
a closure parameter over a row is `row` (`row_2` when nested inside
another), over anything else `x`; a fold's accumulator is `acc`; the
provided parameter is named after the table its type is a row (or list of
rows) of, else after the middleware; an unused `ctx`, `db` or `input` is
`_ctx`, `_db`, `_input` in Rust and `_` in Swift and Kotlin. The canonical
sources use exactly these names.

`Unwrap` is the one addition to `StdFn`: `unwrap(opt)` is the value, or the
refusal `unwrapped none`. `or_refuse` is its only spelling in the canonical
form; a bare `Unwrap` prints as `.unwrap()`.

**A body's value.** A mutator's closure returns `Effect`, which is the last
write statement; a body with no write ends after its last statement and
nothing is returned. A guard's closure returns `Effect` the same way. A
query's, a helper's and a provider's closure returns a value: `SReturn e`
is appended, so `….all()` as a query's value is `SLet s (ESelect p)`,
`SReturn (EVar s)`.

**Function order in the module.** Routers in the order `Module::new` was
given them; within a router, its middleware in declaration order, then its
routes in order; a helper is placed immediately before the first function
in that order that calls it (a helper a helper calls precedes its caller).
`emit()` returns the verified form: orders completed and every function
normalised, so the bytes equal `verify`'s output.

**Middleware.** `fnInput` is the declared input type's fields by name and
type, checks stripped; `fnRouter` is `null`;
`fnUses` is `[]`. A procedure's `fnUses` is the chain it was built from,
oldest first.

**Orders.** The verifier completes every order with the key columns
ascending. The printer writes the shortest prefix whose completion is the
stored order, so an author never writes the completion and may not write
it in the canonical form (the printer would drop it). `.order_by` takes one
`Col.asc()`/`.desc()` or a tuple of them.

**Relationships.** For a `Ref` from `child.col` to `parent`, the parent row
type gains `pub const <child>: Rel<Self, Child> = rel("<child>")` — named
`<child>_<col>` when the child references the parent through more than
one column — and a plan's `Related.rName` is that name.

**Input types are named after the function that declares them**, in
PascalCase: `CreatePlaylist` for `create_playlist`, `Owned` for the `owned`
provider's input. A name that is already the tables' or a row's gets `Input`
appended. Two procedures with the same input fields still get a type each
(`AddToPlaylist`, `RemoveFromPlaylist`): the module does not carry a type's
name, so the print cannot know that an author shared one, and a type per
procedure is also what reads best at a call site
(`AddToPlaylist(playlist_id, track_id)` for `add_to_playlist`).

**A binding is inlined exactly when its single use heads the next
statement**: the receiver of the chain that statement builds, or the value
the function returns. So a query's `let v = select(..); return v` prints as
the chain ending in `.all()`, a provider's `get`, `filter` and `or_refuse`
print as one chain, and a read used inside a row literal is a `let`
(`let playlist_item = …first();` before the insert that reads it). The IR
cannot record the choice — a host `let` emits nothing — and this is the
rule the print makes it by; a canonical source follows it.

**Layout is the formatter's.** The printer writes one logical line per
statement and item, and one blank line between items (a struct and its
`impl`s or extensions are one item); where lines break is decided by the
language's formatter, whose output over the print *is* the canonical text:
rustfmt under `harken/domain/rustfmt.toml`, swift-format under
`harken/domain/.swift-format` (which respects the printer's breaks, so the
Swift printer breaks a statement's chain before each call itself when it
has two or more calls and runs past 100 columns, and puts each column of
a row on a line of its own), and ktfmt in its kotlinlang style (which
also drops an explicit `Int` or `List` import from a file that names
neither). `arkc gen --fmt CMD` runs the formatter; `nix flake check` pins
all three.

**Files.** Rust: `use ark::authoring::*;` first in every file, then
`use crate::schema::*;` in a router file, with the helpers and records it
takes from other router files beside it (§2.5), then the router functions a
module file names. Swift: `import ArkAuthoring`. Kotlin: `package <p>`
(`arkc gen kotlin … --package p`, default `domain`), a blank line, then
`import dev.arkdb.authoring.*`, `import dev.arkdb.authoring.Int`,
`import dev.arkdb.authoring.List` (Kotlin's default imports beat a star
import, so the two are named). Kotlin spells `when` as `` `when` `` (a
hard keyword), a key as `Key1<A>`/`Key2<A, B>`/`Key3<A, B, C>` on the row
class and positional arguments at a call, `orderBy` and `routes` as
varargs, the tables and a row and an input as a class whose
`companion object : Tables.Of` / `Row.Of<T>` / `Input.Of<T>` carries
`NAME`, `columns()`, `schema()` and the column and relation constants; an
unused closure parameter is `_`; the tables have no `open()`, because the
Kotlin runtime reads the tables', a row's and an input's fields from its
constructor, in declaration order; the module file is
`fun module(): Module = Module(library(), playlists())`. The Kotlin runtime's
`dev.arkdb.authoring` is the reference for that spelling, and `arkc gen
kotlin` writes it. `arkc roundtrip` removes comment-only lines
(`//`, `///`, `//!`, `/** … */` on their own lines) from both sides before
comparing, so a domain may be documented and still be canonical; blank
lines are compared.

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

## Appendix B. The demo, in the vocabulary

`Ark.Demo` (and `spec/vectors/module/demo.json`) is the playlist demo the
vectors run: the tables `playlist(id, name, user_id)` and
`item(playlist_id → playlist, track_id: text, pos)`, unique
`(playlist.user_id, playlist.name)` and `(item.playlist_id, item.pos)`.
Its router, as `arkc gen rust` prints it from `arkc demo` under the
domain's rustfmt, is exactly:

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
            let item = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
            db.item.insert(Item {
                playlist_id: input.playlist_id,
                track_id: input.track_id,
                pos: item.map_or(0, |row| row.pos).add(1),
            })
        }),
        demo.input::<Items>().query("items", |_ctx, db, input| {
            db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.asc()).all()
        }),
    ))
}
```

and its inputs:

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

pub struct Items {
    pub playlist_id: Id<Playlist>,
}
impl Input for Items {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}
```

with the tables `Demo { playlist: Table<Playlist>, item: Table<Item> }`
(`arkc gen rust M OUTDIR --name Demo`),
opened as `Demo { playlist: table(), item: table() }`. A runtime's own
demo may name its input types otherwise — a name reaches no byte — but its
`emit` is held to the vector's bytes; that is check 1 of §5.
