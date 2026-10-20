# Authoring: restricted Rust, lowered to the one language

`docs/plan-v5.md` §1.12. A domain is written as Rust functions under
`#[ark::*]` attributes over types the derives describe; the macros
(`rust/ark-macros`) lower each body to §1.4's `Expr` through
`ark::author`, and `module!` assembles a schema and the functions into a
`Module` and a checked `Lib`. There is one execution path, the
interpreter: the Rust a macro writes builds the function's IR and never
runs the body. `harken/domain` is a whole domain written this way.

**What is accepted is the table below**, and nothing else: any other
construct is a compile error naming it (`… is not in the subset ark
lowers`). Every row has a test that it lowers to the `Expr` it says
(`rust/ark-macros/tests/lower.rs`, and `tests/demo.rs`, the demo of
`rust/ark/tests/demo.rs` byte for byte) and a compile-fail test of the
nearest thing outside it (`rust/ark-macros/tests/ui/`, its words in the
`.stderr` beside it).

**Names are rustc's.** A field is read by name (`x.f`), a struct literal
names its fields, a match arm names its variant and the variant's fields:
the derives write, for each type, the view, setters and constants these
lower to, and a misspelled or missing name is rustc's error, which names
it. No field is ever bound by its position in a declaration. Every
expression is built as the Rust type it is, refinements dropped (`Name`
and `Text` are one `String`); the checker (`ark::check`) judges the rest
when the module is assembled.

## The functions

| attribute | the function | effect |
|---|---|---|
| `#[ark::mutator("path.name")]` | `(Input) → ()` | `Write` |
| `#[ark::query("path.name")]` | `(Input) → List<T>`, the relational subset; what a view maintains | `Read` |
| `#[ark::scope("path.name")]` | `(Ctx, <Schema>Row) → Bool` (§1.8): the derive's sum of the tables' rows | `Pure` |
| `#[ark::live("path.name")]` | a mutator over volatile tables keyed by instance (§1.9); one touching a durable table, itself or through a callee, is refused when its module is assembled | `Write` |
| `#[ark::helper]`, `#[ark::guard]` | a function others call; a guard is a helper that refuses | the least its body has, callees included |

The name is the binding's path (`library.add_song`); without one it is the
function's. Parameters are bound in order, the last innermost. A parameter
of type `Ctx` is the context and one named `db` the table set; neither is
a parameter of the function — both are ambient — and a call passes them
along as written. A return type `Result<(), Refuse>`, or none, is `()`.

## The accepted subset

| construct | Rust | lowers to | lowering test | refused nearest |
|---|---|---|---|---|
| parameter | `a: T` | a parameter of `T`'s type | `parameters` | `(a, b): (T, U)` — `param_pattern` |
| let | `let x = e; rest` (`let x: T = e;` too) | `Let(e, rest)` | `let_binds_the_rest` | `let mut x` — `mutable_binding`; `let (x, y)` — `let_pattern`; `let x = e else {..}` — `let_else` |
| statements | `s1; s2; e` | `Seq([s1, s2, e])`; one alone is itself; a last `s;` of no `()` value is `Seq([s, ()])` | `statements_are_a_seq` | an item in a body — `item_in_body`; a loop — `a_loop` |
| if | `if c { a } else { b }`, `if c { a }` | `If(c, a, b)`, `If(c, a, ())` | `if_and_else` | — |
| if let | `if let P = e { a } else { b }` | `Match(e, …)`, the else the other arms | `if_let_is_a_match` | — |
| match | on an option (`None`, `Some(x)`) or a declared sum (`E::V`, `E::V(a, _)`, `E::V { f, g: h, .. }`), `_` for the rest | `Match(e, arms)`: one per variant in declaration order, each binding all its fields, the last innermost | `match_on_a_sum` | a guard — `match_guard`; a nested pattern — `nested_pattern`; an option without both arms — `option_without_none`; a variant missing — `sum_not_exhaustive` (rustc's) |
| literals | `5`, `-5`, `"x"`, `true` | `Lit(Int)`, `Lit(Text)`, `Lit(Bool)` | `literals_and_tuples` | `1.5` — `float_literal` |
| operators | `+ - * / %`, `== != < <= > >=`, `&& \|\|`, `!`, `-` | `Op`, `Un`: checked arithmetic, comparison by bytes, short-circuit | `operators_lower_to_ops` | `+=` — `assignment`; `&` — `bitwise`; `as` — `cast`; `&x` — `reference` |
| field | `x.f`, `x.0` | `Field(x, i)`, `i` the field's declared position | `fields_by_name` | `x.misspelt` — `no_such_field` (rustc's); `xs[i]` — `indexing` |
| struct literal | `S { f: e, g, ..base }` | `Tuple` of the fields in declaration order, the rest `Field(base, i)` | `struct_literals_by_name` | a field left out — `missing_field` (rustc's) |
| tuple | `(a, b)` … four | `Tuple` | `literals_and_tuples` | five — `tuple_of_five` |
| variant | `E::V { f: e }`, `E::V(e)`, `E::V`, `Some(e)`, `None` | `Variant(E, i, fields in declaration order)` | `variants_and_lists` | a refinement's constructor `Name(t)` — `refined_constructor` |
| list | `vec![a, b]`, `[a, b]` | `List(T, …)` | `variants_and_lists` | another macro — `other_macro` |
| call | `f(a, b)` of an `#[ark::*]` function | `Call(f's hash, [a, b])`, `f`'s declarations the caller's | `calls_by_hash` | `f` calling `f` — `calls_itself` |
| standard library | text: `trim is_empty lower is_alnum chars len starts_with split_once fnv1a64 utf8`; int: `to_text min max clamp abs`; bytes and ids: `hex sha256 text_of_id`; list: `first last len is_empty contains reverse concat insert remove_at move_at slice sum`; option: `is_some is_none unwrap unwrap_or`; `concat(texts)`, `nil_id()` | `Std(f, …)` (a list's `is_empty` is `len == 0`, an option's `is_none` `!is_some`) | `the_standard_library` | `xs.push(x)` — `other_method` |
| relational | on a list or `db.t`: `filter map join group sort take`, closures as lambdas; `sort(\|r\| (k1, Desc(k2)))` gives each key its direction; `db.t` alone is `Scan`, `db.t.get(k)` `Lookup` | `Filter Map Join Group Sort Take Scan Lookup` | `the_relational_combinators` | a closure anywhere else — `loose_closure`; `move \|x\|` — `move_closure`; `.map::<T>` — `turbofish` |
| option | `map filter map_or` with closures | `Match` | `options_methods` | — |
| writes | `db.t.insert(row)`, `db.t.update(k, \|r\| row)`, `db.t.delete(k)`; a key is `k` or `(k1, k2)` | `Insert Update Delete`, the key a `Tuple` | `the_writes` | — |
| refuse | `refuse!(message)` | `Refuse(message)` | `refuse_is_refuse` | `return` — `early_return`; `?` — `question_mark` |
| context | `ctx.user`, `.session`, `.roles`, `.now` (or as methods), `ctx.new_id()` in an `Id<T>` field or `ctx.new_id::<T>()`, `ctx.has_role(r)` | `Ctx(…)`, `NewId(t)`, `Std(Contains, [Ctx(Roles), r])` | `the_context`, `tests/demo.rs` | `ctx.device` — `context_field` |
| the end | a body's last `Ok(())` | nothing | `ok_is_nothing` | `Ok(..)` anywhere else |
| a name | a parameter, a `let`, a closure's, an arm's | `Var(i)`, De Bruijn | every test | anything else — `unbound` |

A helper's effect is the least its body has (`a_helper_is_at_its_least_effect`);
every function above checks (`every_lowering_checks`).

## The types: derives

| derive | on | says |
|---|---|---|
| `Row` | a struct with named fields | a table: its name (`#[ark(table = "t")]`, else the struct's in snake case), its columns in order, each its type's name and structure, its key (`#[ark(key)]` on the leading columns, else the first), `#[ark(index(a, b))]`, `#[ark(unique(a, b))]`, `#[ark(refs = Row)]` on a column, `#[ark(volatile)]` |
| `Input` | a struct with named fields | a record named by the struct: the `Tuple` of its fields; a field with refinements is a declaration `S.f` of them |
| `Input` | a tuple struct of one field | a refinement of that field's type, named by the struct |
| `Sum` | an enum | a sum named by the enum, its variants records or tuples |
| `Schema` | a struct of `Table<Row>`s | the schema, tables in order, and `<Name>Row`, the sum of their rows |

A refinement, in order, each followed by `why = "…"` for its words: `trim`,
`utf8`, `non_empty`, `len = 16`, `len = "1..=80"` (`"1.."`, `"..=80"`); of
an `Int`, `min = n`, `max = n`, `ne = n`, `range = "lo..=hi"` (a `Pure`
predicate of the value). The Rust types are `Int` (`i64`), `Text`
(`String`), `bool`, `Bytes`, `Id<Row>` (`Bytes where len 16`, declared as
`Id<t>`), `Option<T>` (declared by its name, a name for its structure, so
a column of it reads as one), `Vec<T>`, tuples, and the derived types;
derived types are plain Rust data, so `#[derive(Node)]` beside them
encodes an input and decodes a row.

A refused derive: a row of an enum — `row_of_an_enum`; a key after a
column that is not one — `key_after_column`; an attribute that is none —
`unknown_attribute`.

## A module

```rust
let domain = ark::module! { schema: Harken, functions: [create_playlist, add_song, …] }?;
```

`ark::author::assemble(schema, &[f::built(), …])`: each function checked
over the schema, its callees first, bound by its name in sorted order; the
module's types every function's declarations. `Domain::cas` is a content
store holding every body; `Domain::run` a mutator's changes over a store,
`Domain::ask` any function's answer.

## Live state

A transition is a `#[ark::live]` mutator over volatile tables keyed by the
instance (§1.9): v4's ordered fields are tables written by ordered
transitions, its ephemeral fields a table of their own — a row per
`(instance, device)` — which only its ephemeral transitions write and no
ordered one reads. `on_join` and `on_leave` are live mutators the
authority authors as devices come and go, their input the device's
record. The calling device is `ctx.session` until the context carries one.
`harken/domain/src/listening.rs` is v4's session as three tables.

## Not in the subset, by design

There is no fold and no recursion: a function's call graph is a DAG, and
a list's elements are seen one at a time (`map`, `filter`), together
(`len`, `sum`, `concat`, `group`), or in order (`sort`, `first`). v4's
folds are a `sum`, a sorted `first`, or a `split_once`; what needs two
neighbouring elements — v4's `slug` collapsing a run of separators — has
no spelling, and waits for a standard function that gives one.
