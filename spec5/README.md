# The ArkDB specification, version 5

`rust/ark5` is the specification: its modules are normative, their doc
comments are the prose, and a runtime in another language is conformant
when it agrees with them on the vectors under `vectors/`, which
`rust/ark5/tests` write (`ARK5_WRITE_VECTORS=1`) and otherwise hold to what
is on disk. This file is the index. `docs/plan-v5.md` is the design;
where a module and this file disagree, the module is right.

## §1 Types and bytes

| § | `rust/ark5/src/` | what it defines |
|---|---|---|
| 1.1 | `ty.rs` | the five constructors — `Int`, `Bytes`, `List`, `Tuple`, `Sum` — and `Named`, a declaration's name, through which a type recurs |
| 1.1 | `codec.rs` | the encoding: canonical, prefix-free, order-preserving; `skip` and the structural `parse` |
| 1.1 | `ir.rs` (`Decl`, `Refine`, `Rule`) | a declaration: a structure and its refinements — `utf8`, `len n`, `len lo..=hi`, `trim`, `non-empty`, and a `Pure` predicate — each with the words a form shows |
| 1.1 | `interp.rs` (`parse`, `check_form`) | the boundary: canonical bytes of the type, every refinement held in order, `trim` normalising before the rules after it, a value refused underneath not held to more; the refusal is the rule's words or the default (`at least 1 character`, `exactly 16 bytes`, `not UTF-8 text`, `must not be empty`, `is not valid`); `check_form` reports every failing field by its path of positions |
| 1.1 | `hash.rs` | the hash spaces, each its own type |
| 1.2 | `schema.rs` (`compat`, `Table::fit`), `peer.rs` (`open`) | a schema grows by appending: a row of a table as another schema of it wrote it reads under this one completed (an older one's) or cut to its prefix (a newer one's). A log opened under a prefix-extension of the schema it was written under is checked entry against entry by each hash over its own bytes, never derived again; its rows are widened and the store's digests are the new schema's; what it logs then carries new-schema hashes. A replica whose authority runs another schema takes a confirmed entry's state as it says; one that diverges twice at one entry asks for the snapshot (`Hello` at cursor `-1`) |

`Text = Bytes where utf8` is every function's without being declared.
Everything else — `Id<t>`, `Name`, `NonEmpty<T>` — is a declaration a
function carries, so a function's hash covers what its types mean.

## §2 The IR is a value

| § | where | what it defines |
|---|---|---|
| 2.1 | `ir.rs` (`Node`), `rust/ark5-macros` (`#[derive(Node)]`) | a Rust type as a value: a struct is the `Tuple` of its fields in order, an enum the `Sum` of its variants' tuples, `Vec<u8>` and `String` are `Bytes`, a `Vec<T>` a `List`, an `Option<T>` `Sum [(), (T)]`, a hash `Bytes`; a type is `Named` by its identifier where it recurs. `Ty`, the schema, the log's records, every frame and the IR are encoded so, and nothing else encodes them |
| 2.2 | `ir.rs` (`Expr`) | the one expression language and its effects, `Pure < Read < Write`; variables are De Bruijn indices, `Var(0)` the innermost binder, so a function's bytes do not depend on the names it was written with |
| 2.3 | `ir.rs` (`Function`) | a function: its declarations sorted by name, its parameters (the last is `Var(0)`), what it returns, the most it may do, its body; `FnHash = sha256(enc(function))` |

## §3 The checker

`check.rs`: one checker of types, effects and refinements. A declared type
is a subtype of what it refines (`Name ≤ Text ≤ Bytes`); one without
refinements is its structure, both ways. A refined value is made inside
the language only by a literal — parsed under its type once, here, and in
normal form — or by a standard function whose signature says so (§5).
Comparison is of one structure, refinements aside. `Refuse` has the empty
sum's type, which every type admits. A call is by hash, to a function
checked before, whose declarations the caller has as they are, so the call
graph is a DAG and every function ends. A lambda stands only as an
operator's argument. A refinement's predicate is `Pure` and reads no
context and calls nothing. A declaration may contain itself only inside a
list or a sum. A column's type is its declaration when the function
declares that name with the column's structure.

## §4 The interpreter

`interp.rs`: one interpreter, a kernel `Procedure`. The input is parsed
under the function's parameters (one, or the tuple of several); the body
evaluates to the list of changes it makes, and the store is not written.
A body reads through its own list so far. `Insert` refuses a key that has
a row (`<t>: a row with this key exists`), `Update` a key that has none
(`<t>: no row has this key`) and reports nothing when the row is
unchanged, `Delete` of a key with no row is nothing; every other
constraint is the store's, when the list is applied. Arithmetic is
checked: `integer overflow`, `division by zero`. `NewId` is the entry's
`n`-th draw, `n` from 0 in evaluation order. A value is the tree the codec
reads under the type the checker gave it; two are compared by their
bytes. `Sort` is stable, by its key's fields each in its direction;
`Group` is in key order; `Join` in the order of its first list.

## §5 The standard library

`std.rs`, `unicode_tables.rs`: v4's names (`spec/README.md` §5), each with
a signature that says which refinements it keeps — a text function returns
`Text`; `Reverse` and `Move` keep a list's length refinements; `Insert`
and `ListConcat` keep `NonEmpty` of their first argument; every other list
function returns the bare list. An index out of range is a refusal in
exact words, never clamped. `Trim`, `Lower` and `IsAlnum` consult the
pinned tables (UCD 16.0.0) and nothing of the platform; text is compared
by its bytes, which is by code point.

**For views (V1).** v4's §6.5 makes one exception to step-by-step
checking: a sum in a plan's node, `fold` with step `acc + f(x)`, adds in
an `i128` and checks the total once, because a maintained view moves that
sum by its members in no particular order and the two evaluations must
agree. v5's interpreter checks every step; a `Group` aggregate that a view
maintains as a number will need the same rule, stated where V1 recognises
it.

## §6 Modules

`module.rs`: `Module { spec, schema, types, bindings }`, `ModuleHash =
sha256(enc(Module))`; a `Lib` keeps checked functions by hash, and its
loader is how a content store runs the bodies of the modules it was given:
an authority's holds exactly the functions bound in the modules its
operator loaded, the current one and those held from before, and never a
body a client sends (no frame carries one). A module verifies
when its bindings and types are sorted by name, each once, every bound
function checks over its schema, and every declaration a function makes is
the module's own.

## The vectors

The JSON dialect of `rust/ark/src/json.rs`; a row is a tuple, printed as an
array in schema order; a value of the language prints under its type.
Every directory has a `falsify/` vector a conformant runner must fail.

| directory | one vector is | holds |
|---|---|---|
| `hash/` | a store and its hash under its schema | K1 |
| `eval/` | a function's bytes, a store, inputs applied in turn: the changes, the rows, the hash, or the refusal; `checks` and `form-check`, the boundary's verdicts and words; `has-role`; `raw-*`, the authority's writes as functions; `list-ops`, the list editors' edges | §1.1, §4, §5 |
| `verify/` | a module's functions and whether they check; `refused/` one per refusal, with its words | §3, §6 |
| `module/` | the demo module's bytes and hash; `de-bruijn`, two spellings and one hash | §2, §6 |
