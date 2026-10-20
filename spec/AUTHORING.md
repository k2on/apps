# Authoring: Ark text, and builders that write it

`docs/plan-v5.md` §1.15. Three layers, as SQL has them:

1. **The module** (§1.5): what runtimes load, check and hash. Nothing below
   changes it; a phone loads it and calls functions by name.
2. **Ark text**: one canonical, language-neutral spelling of a module
   (§1 below). `arkc print module.ark` writes it, `arkc build FILE… -o
   module.ark` compiles it, and anyone may write it. One printer, so
   `build(print(m)) == m` for every module and `print(build(t)) == t` for
   canonical text — text as the printer writes it.
3. **Builders**, per language, typed, constructing the IR (§2). They cover
   what a domain says most; `text("…")` embeds Ark text for the rest, so
   they need not grow. Rust's are `ark::author`; another language's are
   held by one rule — its demo emits the module hash Rust's does.

The reference: `rust/ark/src/text.rs` (the compiler and the printer),
`rust/ark/src/author.rs` (the Rust builders), `rust/ark/tests/demo.ark`
(the demo as text). `harken/domain` is a whole domain written with the
builders.

## 1. The text

### Items

A file is items, each beginning at the start of a line: one `schema`
block, `type` declarations, and functions, in any order. `//` comments.

```
schema {
  table playlist {
    id: Id<playlist> key
    name: Text
    user_id: Text
    index unique (user_id, name)
  }
  table item {
    playlist_id: Id<playlist> key refs playlist
    track_id: Text key
    pos: Int
    index unique (playlist_id, pos)
  }
}

type Name = Text where trim, non_empty "a playlist needs a name"
type TrackId = Text where len 1..

mutator demo.rename_playlist(playlist_id: Id<playlist>, name: Name) = if playlist.get(playlist_id).is_some() {
  playlist.update(playlist_id, p => { name: name, ..p })
} else {
  refuse "no such playlist"
}
```

```
schema   = "schema" "{" table* "}"
table    = ["volatile"] "table" name "{" (column | index)* "}"
column   = name ":" type ["key"] ["refs" table-name] ["default" x"hex"]
index    = "index" ["unique"] "(" name ("," name)* ")"
type     = "type" type-name "=" type ["where" rule ("," rule)*]
rule     = ("utf8" | "trim" | "non_empty" | "len" n | "len" n ".." | "len" n "..=" m | name "=>" expr) [string]
function = kind path "(" [name ":" type ("," name ":" type)*] ")" ["->" type] ("=" expr | block)
kind     = "mutator" | "live" | "query" | "scope" | "helper" | "guard"
```

A table's key is its leading `key` columns (the first when none is
marked); a column's type is written as its declaration's name, which the
schema keeps. A rule's words are what a form shows when a value breaks it;
`x => e` is a `Pure` predicate of the value.

**The kind is the effect bound**: `mutator` and `live` (a mutator over
volatile tables only, refused at build otherwise) are `Write`, `query`
`Read`, `scope` `Pure`; `helper` and `guard` are the least their body has,
callees included. A function's name is its binding's path — the prefix is
its router. Every function is bound by its name, except one whose name
starts with `_`. A function's declarations are the types its parameters,
result, literals, variants, lists and fresh ids name, closed over their
structures, and its callees'; the module's are its functions'.

### Types

| type | is |
|---|---|
| `Int`, `Bytes`, `Text`, `Bool`, `Never`, `()` | the leaves; `Text = Bytes where utf8`; `Bool = Sum<(), ()>`; `Never` the empty sum |
| `(A, B)`, `(A,)` | a tuple |
| `List<T>` | a list |
| `Sum<(A,), (), (B, C)>` | a sum, its variants each a tuple of fields |
| `Id<t>` | `Bytes where len 16`, declared by that name |
| `Option<T>` | `Sum<(), (T,)>`, declared by that name |
| a declared name | `Name`, `AddSong.title` |
| a table's name in camel case | its row: the tuple of its columns' types (`PlaylistItem`) |

### Expressions

Everything is an expression; `let` is the only statement. There are no
semicolons, `return`, loops or mutation.

| construct | text | IR |
|---|---|---|
| literal | `5`, `-5`, `"x"` (escapes `\n \t \r \0 \\ \" \u{…}`), `true`, `false`, `x"00ff"` (bytes), `lit<T>(x"…")` (any type, encoded bytes) | `Lit` |
| unit | `()` | `Seq([])` |
| block | `{` a statement a line `}`; `let x = e` binds the rest; the value is the last; a block of one is that one | `Seq`, `Let` |
| tuple | `(a, b)`, `(a,)` | `Tuple` |
| list | `[a, b]` (the type of `a`, its refinements dropped), `List<T>[…]` | `List` |
| option | `some(x)` (as a list's), `some<T>(x)`, `none<T>` | `Variant(Option<T>, …)` |
| variant | `T#i`, `T#i(a, b)` | `Variant(T, i, …)` |
| if | `if c { … } else { … }`, `else if`, no `else` is `()` | `If` |
| match | `match e {` an arm a line `}`: `none`, `some(x)`, `false`, `true`, `#i`, `#i(a, b)` — every variant in order, each binding all its fields | `Match` |
| field | `x.name` (a row's column, a refined field's declaration `R.name`), `x.2` | `Field(x, i)` |
| operators | `or` < `and` < `not` < `== != < <= > >=` (one) < `+ -` < `* / %` < `-x`, `refuse e` < postfix | `Op`, `Un`, `Refuse` |
| standard library | a method of its first argument: `x.trim()`, `xs.len()`, `o.unwrap_or(d)`, `xs.concat()` (texts), `xs.concat(ys)`, `nil_id()` — `Std`'s names | `Std` |
| relational | `xs.filter(x => p)`, `.map(x => e)`, `.join(ys, (x, y) => c)`, `.group(x => k, (k, ms) => a)`, `.sort(x => k)` with `desc k`, `asc k`, `(k1, desc k2)`, `.take(n)` | `Filter Map Join Group Sort Take` |
| tables | `t` (its rows), `t.get(k1, k2)`, `t.get(..key)`, `t.insert({ col: e, …, ..base })`, `t.update(k, r => { col: e, ..r })`, `t.delete(k)` | `Scan Lookup Insert Update Delete` |
| context | `ctx.user`, `ctx.session`, `ctx.roles`, `ctx.now`, `ctx.new_id(t)` | `Ctx` |
| call | `path.name(a, b)` | `Call(hash, …)` |

A variable shadows a table; a dotted path followed by `(` is a call when
a function has that name. A line beginning with an operator begins a new
statement inside a block; inside brackets newlines are nothing.

### What the bytes do not hold

A module keeps no binder names (variables are De Bruijn indices), no
record field names and no variant names. The printer names a binder from
its type (`playlist_id`, `n`, `xs`, a row by its table's initials, never a
table's or a router's name), a row's field by its table's column when one
table's rows are of its type, a refined record field by its declaration
(`AddSong.title`), and anything else by position (`x.2`, `#1(a, b)`).
Hand-written text uses those spellings. Where one IR node has one
spelling the printer writes it; where two IR nodes could read alike the
printer picks: the kind by effect (`mutator`, `live`, `query`, `helper`),
`not x.is_some()` for an option's emptiness, `x.unwrap_or(d)`, `T#i` with
its type where `some(x)` or `[x]` would infer another.

## 2. The builder vocabulary

What every language's builders offer, and Rust's spelling of it
(`ark::author`). An expression is an `Ex<T>`, `T` the type it is, its
refinements dropped; its methods take `&self`, so a value is used as often
as it is named. A binder is a closure.

| construct | Rust |
|---|---|
| a function | `pub static F: Def2<A, B, R> = Def2 { name: "router.f", kind: Kind::Mutator, body: \|a, b\| … };` — parameters typed by their declared types (`Name`, not `Text`); `Def0`…`Def5` |
| a call | `F.call(a, b)` |
| a module | `assemble(Schema::schema(), &[&F, &G])` — callees come with, all bound |
| block, let | `block(\|b\| { let x = b.let_(v); b.s(stmt); last })`, `let_(v, \|x\| body)`, `seq([s1, s2], last)`, `x.s()` (a statement) |
| literals | `int(5)`, `txt("x")`, `bool(true)`, `unit()`; `5`, `"x"`, `true` where an `Ex` is taken |
| if | `if_(c, a, b)`, `if_(c, a, unit())` |
| option | `some(x)`, `none::<T>()`, `o.map_or(d, \|x\| …)`, `o.map(\|x\| …)`, `o.filter(\|x\| …)`, `o.is_none()` |
| sum | `ClockEx::paused(at)`, `ClockEx::match_(&c, \|at\| …, \|at, since\| …)` — the derive's |
| record | `x.f().name`, `PlaylistEx { name, ..p.f() }.into()` — the derive's; `tup((a, b))`, `pair.f().0` |
| list | `list(vec![a, b])` |
| operators | `a + b`, `a - 1`, `-a`, `!a`, `a.eq(b)`, `ne lt le gt ge`, `a.and(b)`, `a.or(b)` |
| standard library | the text's method names on `Ex`; `concat(vec![…])`, `concat_of(&xs)`, `nil_id()` |
| relational | `xs.filter(\|x\| …)`, `.map`, `.join(&ys, \|x, y\| …)`, `.group(\|x\| k, \|k, ms\| a)`, `.sort(\|x\| (desc(a), b))`, `.take(n)` |
| tables | `let db = Harken::db();` then `db.t.all()`, `db.t.get(k)`, `db.t.get((k1, k2))`, `db.t.insert(row)`, `db.t.update(k, \|r\| row)`, `db.t.delete(k)` |
| refuse | `refuse(msg)` (any type), `refuse_(msg)` (a statement) |
| context | `Ctx.user()`, `.session()`, `.roles()`, `.now()`, `Ctx.new_id::<Row>()`, `Ctx.has_role(r)` |
| text | `text::<T>("if a < b { a } else { b }", &[("a", &a), ("b", &b)])`: checked where it is built; its tables are the module's being assembled |

A body written as one would write the Rust it means is the IR the
attribute macros (deleted) built from that Rust, byte for byte:
`harken/domain/tests/port.rs` holds every one of harken's 127 functions
to the hash the macros built.

### The types, in Rust: derives

| derive | on | says |
|---|---|---|
| `Row` | a struct with named fields | a table: its name (`#[ark(table = "t")]`, else the struct's in snake case), its columns in order, each its type's name and structure, its key (`#[ark(key)]` on the leading columns, else the first), `#[ark(index(a, b))]`, `#[ark(unique(a, b))]`, `#[ark(refs = Row)]` on a column, `#[ark(volatile)]`; and `RowEx` |
| `Input` | a struct with named fields | a record named by the struct: the `Tuple` of its fields, a field with refinements a declaration `S.f` of them; and `SEx` |
| `Input` | a tuple struct of one field | a refinement of that field's type, named by the struct |
| `Sum` | an enum | a sum named by the enum, its variants records or tuples; and `SEx`, its makers and its match |
| `Schema` | a struct of `Table<Row>`s | the schema (`S::schema()`), the tables to build over (`S::db()`), and `SRow`, the sum of the rows a scope takes |

A refinement, in order, each followed by `why = "…"` for its words:
`trim`, `utf8`, `non_empty`, `len = 16`, `len = "1..=80"` (`"1.."`,
`"..=80"`); of an `Int`, `min = n`, `max = n`, `ne = n`, `range =
"lo..=hi"`. The Rust types are `Int` (`i64`), `Text` (`String`), `bool`,
`Bytes`, `Id<Row>`, `Option<T>`, `Vec<T>`, tuples, and the derived types,
which are plain Rust data: `#[derive(Node)]` beside them encodes an input
and decodes a row.

## Live state

A transition is a `live` function over volatile tables keyed by the
instance (§1.9): v4's ordered fields are tables written by ordered
transitions, its ephemeral fields a table of their own — a row per
`(instance, device)` — which only ephemeral transitions write and no
ordered one reads; which functions are ephemeral is the client's
configuration, not the module's. `on_join` and `on_leave` are live
functions the authority authors as devices come and go. The calling
device is `ctx.session`. `harken/domain/src/listening.rs` is v4's session
as three tables.

## Not in the language, by design

There is no fold and no recursion: a function's call graph is a DAG, and
a list's elements are seen one at a time (`map`, `filter`), together
(`len`, `sum`, `concat`, `group`), or in order (`sort`, `first`). v4's
folds are a `sum`, a sorted `first`, or a `split_once`; what needs two
neighbouring elements — v4's `slug` collapsing a run of separators — has
no spelling, and waits for a standard function that gives one.
