# Authoring: ArkLang, and the Rust builders

`docs/plan-v5.md` §1.15. Three layers, as SQL has them:

1. **The module** (§1.5): what runtimes load, check and hash. A phone
   loads it and calls functions by name; it needs nothing below.
2. **ArkLang**, the common text form: S-expressions (§1 below). It is what
   `arkc print module.ark` writes, what `arkc build FILE… -o module.ark`
   compiles, what the explorer shows, what the docs and the `text/`
   vectors use, and what anyone in any language may hand-write. One reader
   rule, `(head args…)`, so there is one spelling by construction, the
   printer is a tree walk and a parser in any runtime is small. One
   printer: `build(print(m)) == m` for every module, `print(build(t)) ==
   t` for text as the printer writes it.
3. **Builders**, per language, typed (§2). Rust's are `ark::author`; a
   builder's `text("(…)")` embeds one ArkLang form. Another language's
   builders are held by one rule: its demo emits the module hash Rust's
   does.

The reference: `rust/ark/src/sexp.rs` (reader, printer, lowering),
`rust/ark/src/author.rs` (the Rust builders), `rust/ark-dev/src/demo.ark`
(the demo, which builds to what `demo.rs`'s builders build, byte for
byte) and `harken/domain/domain.ark` (harken's whole module, which builds
to `harken.ark`).

## 1. ArkLang

### Reading

Atoms are **symbols**, **integers** (`5`, `-5`), **strings** `"…"` (`\"`
and `\\` the only escapes) and **bytes** `0x00ff`; `true`, `false` and
`none` are symbols with a meaning. Lists are `( … )`; `;` comments to the
end of a line. A symbol may contain `.`: a dotted symbol whose head is a
bound variable is a field path (`input.playlist_id`, `pair.0`); otherwise
it is a declared name (`playlists.items`, `Clock.Paused`). An error is
reported at its `line:column`.

**The canonical layout**: a form prints on one line when it ends by
column 100, else its head on the first line and each argument on its own
line, two spaces in from the form's `(`; strings and numbers as they read;
one blank line between top-level forms; comments are not kept.

### Top-level forms

```
(schema (table NAME [volatile] (COL TY [key] [(refs TABLE)] [(default 0x…)])… (index [unique] COL…)…)…)
(type NAME TY)
(mutator NAME ((P TY)…) BODY…)          ; Write, returns ()
(live    NAME ((P TY)…) BODY…)          ; Write over volatile tables only, returns ()
(query   NAME ((P TY)…) RET BODY…)      ; Read
(helper  NAME ((P TY)…) RET BODY…)      ; the least effect its body has
(guard   NAME ((P TY)…) RET BODY…)      ; as helper
(scope   NAME ((ROW TY)) BODY…)         ; Pure, returns Bool; TY the sum of the tables' rows
```

A table's key is its leading `key` columns (the first when none is
marked). A body of several forms is a sequence whose value is the last. A
function's name is its binding's path — the prefix is its router — and
every function is bound by its name except one whose name starts with
`_`. A function's declarations are the types its parameters, result,
literals, variants, lists and fresh ids name, closed over their
structures, and its callees'; the module's are its bound functions'. The
printer writes the kind by the effect: `live` when every table reached is
volatile, else `mutator` for `Write` returning `()`; `query` for `Read`;
`scope` for a `Pure` predicate of the rows' sum; `helper` otherwise.

### Types

| type | is |
|---|---|
| `Int` `Bytes` `Text` `Bool` `Never` | the leaves; `Text = Bytes where utf8`; `Bool` the sum of two empty variants; `Never` the empty sum |
| `(id TABLE)` | `Bytes where len 16`, declared as `Id<TABLE>` |
| `(option TY)` | `Sum [(), (TY)]`, declared as `Option<TY>` |
| `(list TY)`, `(tuple TY…)` | a list; a tuple (`(tuple)` is `()`) |
| `(row TABLE)` | the table's row: the tuple of its columns as the checker types them |
| `(record (F TY)…)` | at the top of a `type`: a tuple whose fields are named, the names kept in the declaration; a field `(F (where …))` declares `NAME.F` |
| `(sum (V (F TY)…) (W))` | at the top of a `type`: a sum whose variants and their fields are named |
| `(where TY RULE ["words"]…)` | a refinement: rules `(utf8)` `(trim)` `(non-empty)` `(len N)` `(len LO HI)` `(len LO ..)` `(fn (x) e)`, each followed by the words a form shows when a value breaks it |
| a declared name | `Name`, `AddSong` |

### Expressions

| form | IR |
|---|---|
| `5`, `"x"`, `0x00ff`, `true`, `false`, `(lit TY 0x…)` (any type, encoded bytes) | `Lit` |
| `()` | `Seq([])`, unit |
| `(let ((X E)…) BODY…)`; `(let () E E…)` a sequence | `Let`, `Seq` |
| `(if C A [B])` — no `B` is `()` | `If` |
| `(match E (V (X…) BODY…)…)` — every variant in order, each binding all its fields; `V` the declaration's name, `none`/`some`, `false`/`true`, or a position | `Match` |
| `x.f.g`, `(. E f g)` — a field by its declaration's or its table's name, or its position | `Field` |
| `(record (F E)…)` where the place wants a record or a row, `(tuple E…)` elsewhere | `Tuple` |
| `(T.V E…)` a declared sum's variant; `(variant TY I E…)` any | `Variant` |
| `(some E)`, `none` (where the place wants an option), `(list E…)`; `(as TY e)` gives one of these the type `TY` | `Variant`, `List` |
| `(?? E D)`, `(some? E)`, `(none? E)` | `Std(UnwrapOr)`, `Std(IsSome)`, `not` of it |
| `(== a b)` `!=` `<` `<=` `>` `>=` `+` `-` `*` `/` `%` `and` `or`, `(not E)`, `(- E)` | `Op`, `Un` |
| `(refuse "msg")` | `Refuse` |
| `(fn (X…) BODY)` — only as an operator's argument | `Lambda` |
| `TABLE` (its rows), `(get T KEY…)`, `(filter L FN)`, `(map L FN)`, `(join A B FN)`, `(group L FN FN)`, `(sort L (fn (x) KEYS))` with `(desc E)`, `(asc E)`, `(tuple k (desc k2))`, `(take L N)`, `(first L)` | `Scan` `Lookup` `Filter` `Map` `Join` `Group` `Sort` `Take` `Std(First)` |
| `(insert T REC)`, `(update T KEY… FN)`, `(delete T KEY…)` | `Insert` `Update` `Delete` |
| `ctx.user` `ctx.session` `ctx.roles` `ctx.now` `(new-id T)` `(has-role "x")` | `Ctx`, `Std(Contains, [ctx.roles, x])` |
| the standard functions by `std.rs`'s names with `-` for `_`: `(trim t)` `(concat ts)` `(concat xs ys)` `(len x)` `(lower t)` `(to-text n)` `(insert-at xs i x)` `(move-at xs i j)` … | `Std` |
| `(NAME ARG…)` a declared function | `Call(hash, …)` |

A table's key is its key columns as arguments (`(get item pid tid)`), or
one expression of the key's tuple type. A binder shadows a table.

### What the bytes do not hold

A module keeps no binder names (variables are De Bruijn indices). Its
declarations keep a record's field names and a sum's variant names
(`Decl::names`), so the printer never writes `input.3` or `#1`; it names a
binder from its type (`playlist_id`, `n`, `xs`, a row by its table's
initials, never a table's, a router's or an atom's name) or from its
variant's field, and a tuple's field by position. Typing is the checker's:
a `none` and a `(record …)` take the type their place wants — a
function's result, a branch's, a field's, an argument's, a row's — and the
printer writes `(as T …)` where no place gives the type the bytes hold.
`ctx.device` has no IR yet and is refused; a record field has no `exists`.

## 2. The builders

What every language's builders offer, Rust's spelling of it
(`ark::author`), and the ArkLang it builds. An expression is an `Ex<T>`,
`T` the type it is with its refinements dropped; its methods take
`&self`, so a value is used as often as it is named. A binder is a
closure.

| construct | Rust | ArkLang |
|---|---|---|
| a function | `pub static F: Def2<A, B, R> = Def2 { name: "router.f", kind: Kind::Mutator, body: \|a, b\| … };` — parameters typed by their declared types; `Def0`…`Def5` | `(mutator router.f ((a A) (b B)) …)` |
| a call | `F.call(a, b)` | `(router.f a b)` |
| a module | `assemble(Schema::schema(), &[&F, &G])` — callees come with, all bound | the file |
| block, let | `block(\|b\| { let x = b.let_(v); b.s(stmt); last })`, `let_(v, \|x\| body)`, `seq([s1, s2], last)`, `x.s()` | `(let ((x v)) stmt last)`, `(let () s1 s2 last)` |
| literals | `int(5)`, `txt("x")`, `bool(true)`, `unit()` | `5`, `"x"`, `true`, `()` |
| if | `if_(c, a, b)`, `if_(c, a, unit())` | `(if c a b)`, `(if c a)` |
| option | `some(x)`, `none::<T>()`, `o.map_or(d, \|x\| …)`, `o.is_some()`, `o.is_none()`, `o.unwrap_or(d)` | `(some x)`, `none`, `(match o (none () d) (some (x) …))`, `(some? o)`, `(none? o)`, `(?? o d)` |
| sum | `ClockEx::paused(at)`, `ClockEx::match_(&c, \|at\| …, \|at, since\| …)` | `(Clock.Paused at)`, `(match c (Paused (at_ms) …) (Playing (at_ms since) …))` |
| record | `x.f().name`, `PlaylistEx { name, ..p.f() }.into()`; `tup((a, b))`, `pair.f().0` | `x.name`, `(record (id p.id) (name name) (user_id p.user_id))`; `(tuple a b)`, `pair.0` |
| list | `list(vec![a, b])` | `(list a b)` |
| operators | `a + b`, `-a`, `!a`, `a.eq(b)`, `ne lt le gt ge`, `a.and(b)`, `a.or(b)` | `(+ a b)`, `(- a)`, `(not a)`, `(== a b)`, `!= < <= > >=`, `(and a b)`, `(or a b)` |
| standard library | `x.trim()`, `xs.len()`, `concat(vec![…])`, `xs.concat(ys)`, `n.to_text()`, `nil_id()` | `(trim x)`, `(len xs)`, `(concat (list …))`, `(concat xs ys)`, `(to-text n)`, `(nil-id)` |
| relational | `xs.filter(\|x\| …)`, `.map`, `.join(&ys, \|x, y\| …)`, `.group(\|x\| k, \|k, ms\| a)`, `.sort(\|x\| (desc(a), b))`, `.take(n)`, `.first()` | `(filter xs (fn (x) …))`, `(map …)`, `(join xs ys (fn (x y) …))`, `(group xs (fn (x) k) (fn (k ms) a))`, `(sort xs (fn (x) (tuple (desc a) b)))`, `(take xs n)`, `(first xs)` |
| tables | `let db = Harken::db();` then `db.t.all()`, `db.t.get(k)`, `db.t.get((k1, k2))`, `db.t.insert(row)`, `db.t.update(k, \|r\| row)`, `db.t.delete(k)` | `t`, `(get t k)`, `(get t k1 k2)`, `(insert t (record …))`, `(update t k (fn (r) (record …)))`, `(delete t k)` |
| refuse | `refuse(msg)` (any type), `refuse_(msg)` (a statement) | `(refuse msg)` |
| context | `Ctx.user()`, `.session()`, `.roles()`, `.now()`, `Ctx.new_id::<Row>()`, `Ctx.has_role(r)` | `ctx.user`, `ctx.session`, `ctx.roles`, `ctx.now`, `(new-id t)`, `(has-role r)` |
| ArkLang | `text::<T>("(if (< a b) a b)", &[("a", &a), ("b", &b)])`: checked where it is built; its tables are the module's being assembled | the form itself |

A body written as one would write the Rust it means is the IR the
attribute macros (deleted) built from that Rust, byte for byte, once the
declarations' names are set aside: `harken/domain/tests/port.rs` holds
every one of harken's 127 functions to the hash the macros built.

### The types, in Rust: derives

| derive | on | says |
|---|---|---|
| `Row` | a struct with named fields | a table: its name (`#[ark(table = "t")]`, else the struct's in snake case), its columns in order, each its type's name and structure, its key (`#[ark(key)]` on the leading columns, else the first), `#[ark(index(a, b))]`, `#[ark(unique(a, b))]`, `#[ark(refs = Row)]` on a column, `#[ark(volatile)]`; and `RowEx` |
| `Input` | a struct with named fields | a record named by the struct, its fields' names in its declaration: the `Tuple` of its fields, a field with refinements a declaration `S.f` of them; and `SEx` |
| `Input` | a tuple struct of one field | a refinement of that field's type, named by the struct |
| `Sum` | an enum | a sum named by the enum, its variants' names and their fields' in its declaration (a tuple variant's one field named as the variant in snake case); and `SEx`, its makers and its match |
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
folds are a `sum`, a sorted `first`, or a `split-once`; what needs two
neighbouring elements — v4's `slug` collapsing a run of separators — has
no spelling, and waits for a standard function that gives one.
