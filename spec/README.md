# The ArkDB specification, version 5

`rust/ark` is the specification: its modules are normative, their doc
comments are the prose, and a runtime in another language is conformant
when it agrees with them on the vectors under `vectors/`, which
`rust/ark/tests` and `rust/ark-dev/tests` write (`ARK_WRITE_VECTORS=1`)
and otherwise hold to what is on disk; written over an empty `vectors/`
they are the tree byte for byte, which the flake's `vectors` check holds.
This file is the index; `RUNNERS.md` says what a runner does with the
vectors `rust/ark-dev` writes, and `AUTHORING.md` is ArkLang, the common
text form, and the Rust builders. `docs/plan-v5.md` is the design; where a module and this
file disagree, the module is right. v5 replaced v4 here (R1); v4's
specification is in the history, at the commits `kotlin/FROZEN.md` names.

## §1 Types and bytes

| § | `rust/ark/src/` | what it defines |
|---|---|---|
| 1.1 | `ty.rs` | the five constructors — `Int`, `Bytes`, `List`, `Tuple`, `Sum` — and `Named`, a declaration's name, through which a type recurs |
| 1.1 | `codec.rs` | the encoding: canonical, prefix-free, order-preserving; `skip` and the structural `parse` |
| 1.1 | `ir.rs` (`Decl`, `Refine`, `Rule`) | a declaration: a structure, its refinements — `utf8`, `len n`, `len lo..=hi`, `trim`, `non-empty`, and a `Pure` predicate — each with the words a form shows, and its names: a record's fields, or a sum's variants each followed by its fields', one to each and distinct (§1.15) |
| 1.1 | `interp.rs` (`parse`, `check_form`) | the boundary: canonical bytes of the type, every refinement held in order, `trim` normalising before the rules after it, a value refused underneath not held to more; the refusal is the rule's words or the default (`at least 1 character`, `exactly 16 bytes`, `not UTF-8 text`, `must not be empty`, `is not valid`); `check_form` reports every failing field by its path of positions |
| 1.1 | `hash.rs` | the hash spaces, each its own type |
| 1.2 | `schema.rs` (`compat`, `Table::fit`), `peer.rs` (`open`) | a schema grows by appending: a row of a table as another schema of it wrote it reads under this one completed (an older one's) or cut to its prefix (a newer one's). A log opened under a prefix-extension of the schema it was written under is checked entry against entry by each hash over its own bytes, never derived again; its rows are widened and the store's digests are the new schema's; what it logs then carries new-schema hashes. A replica whose authority runs another schema takes a confirmed entry's state as it says; one that diverges twice at one entry asks for the snapshot (`Hello` at cursor `-1`) |
| 1.6 | `log.rs` (`Origin`, `EntryId`) | an entry's id is `sha256(enc(Bytes origin) ‖ enc(Int local))[..16]`, `origin` sixteen bytes a replica draws once at its first open and keeps, `local` its count of intents from 1; the `n`-th row id an entry draws is `sha256(id ‖ enc(Int n))[..16]`. Neither depends on who is signed in. An authority dedupes and orders by `(origin, local)`, binds an origin to the actor whose intents first came from it, and stamps each entry with the actor, session and roles of the connection it judged it on |

`Text = Bytes where utf8` is every function's without being declared.
Everything else — `Id<t>`, `Name`, `NonEmpty<T>` — is a declaration a
function carries, so a function's hash covers what its types mean.

## §2 The IR is a value

| § | where | what it defines |
|---|---|---|
| 2.1 | `ir.rs` (`Node`), `rust/ark-macros` (`#[derive(Node)]`) | a Rust type as a value: a struct is the `Tuple` of its fields in order, an enum the `Sum` of its variants' tuples, `Vec<u8>` and `String` are `Bytes`, a `Vec<T>` a `List`, an `Option<T>` `Sum [(), (T)]`, a hash `Bytes`; a type is `Named` by its identifier where it recurs. `Ty`, the schema, the log's records, every frame and the IR are encoded so, and nothing else encodes them |
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
`Group` is in key order; `Join` in the order of its first list. A read —
`Scan`, `Lookup`, `Filter`, `Map`, `Join`, `Group`, `Sort`, `Take` — is
not the interpreter's: it is a view hydrated once (`view.rs`, §1.10),
every row an insert, its lambdas run by the interpreter on the same run
(its stack, its writes so far, its draws); the read of a filter's
equalities and a sort's order through the store's indexes is the view's
too.

## §5 The standard library

`std.rs`, `unicode_tables.rs`: v4's names, each with
a signature that says which refinements it keeps — a text function returns
`Text`; `Reverse` and `Move` keep a list's length refinements; `Insert`
and `ListConcat` keep `NonEmpty` of their first argument; every other list
function returns the bare list. An index out of range is a refusal in
exact words, never clamped. `Trim`, `Lower` and `IsAlnum` consult the
pinned tables (UCD 16.0.0) and nothing of the platform; text is compared
by its bytes, which is by code point.

**`Sum`**, appended for views (V1), is v4's §6.5 rule: a list's integers
added in an `i128` and the total judged once (`integer overflow` only if
it does not fit), because a maintained view moves a sum by its members in
no particular order and the two evaluations must agree
(`views/a-maintained-sum-*`).

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

## §7 The kernel

| § | `rust/ark/src/` | what it defines |
|---|---|---|
| 1.3 | `store.rs` | one ordered map per table, rows by key and index entries; every write judges the constraints (canonical row bytes, unique indexes, a reference names a row that exists, a row still named is not removed) and is a `Change` — `Add`, `Remove`, `Edit` — that inverts exactly; the state hash kept incrementally per table, over the durable space only, the schema hash in it; a store has a space; the log is its `_log` table, keyed by sequence, a row a `Logged`'s bytes, its invariant the chain (a row added or removed only at an end it links onto, never edited), outside the schema, the state hash and a snapshot sent |
| 1.6 | `log.rs` | the chained log: `Entry { id, actor, session, origin, local, roles, fn, input, now }`, `Logged { seq, entry, facts, state, hash }`, the hash the chain to it; the chain is the log's identity; a log stands on its table's first row (a snapshot's head, or the horizon, a delete below); what is durable is one format, the store's rows whole and the writes since (`Record`, `Durable`) |
| 1.7 | `peer.rs` | the one sans-io machine — `Replica` or `Authority { journal }`, alone being an authority with a fork — `step(Input) -> [Output]`: sequencing, dedupe per `(origin, local)`, judging, the rebase (pending lists undone newest-first, the confirmed facts applied, the pending run again, once per page), compaction, going alone and joining (a chain adopted by its facts or refused), a held intent kept pending and what follows it waiting on the authority, `Input::Loaded` judging what was held; a replica applies confirmed entries by their facts |
| 1.8 | `peer.rs` (`Feed`), `view.rs` | partial sync: a scope is one `Pure` function over the `Sum` of the schema's rows; an authority keeps a view of it per scoped connection (`Input::Feed`) and answers it by `Output::To`, a union holding what its rows reference |
| 1.9 | `peer.rs` (the volatile space) | live state: tables marked `Volatile`, a second machine of the same kind over them on the same socket, frames tagged with their `space`; `on_join`, `on_leave` and `on_restore` entries the authority authors; not in the state hash, not journaled |
| 1.10 | `view.rs` | maintained views: one node per operator of a query's relational spine, each updated from a delta; after every step the answer equals a fresh one — a fresh read is the same nodes hydrated once, the one evaluator of reads (K3a) — and the patches (`Insert`, `Remove`, `Update` at a position) splice the old list into the new |
| 1.11 | `frames.rs` | the frames, one derived `Sum` of four: `Hello`, `Page` (a peer's intents, or its chain made alone; an authority's log), `Snapshot`, `Verdict` (`Ok`, `Refused`, `Held`). A peer sends pages of its chain; an authority answers with verdicts and its own pages. No frame carries a function's body (K2) |
| 1.15 | `sexp.rs` | ArkLang, the common text form: S-expressions, one reader rule; `build` lowers a file through the checker to a module, `print` writes a module, `build(print(m)) == m`; the grammar is `AUTHORING.md` §1 |
| 1.15 | `author.rs`, `rust/ark-macros` (the derives) | the Rust builders: a function a `static` `DefN` whose body builds `Expr`, records and sums by the derives, their names into the module's declarations; `text()` embeds one ArkLang form |

The drivers are not the specification but the reference reading of it:
`rust/ark-client` (a client: storage, the link, views for a UI, the live
relay), `rust/ark-server` (the hub on its thread, persistence, the sync
socket, the admin page's listener) and `rust/ark-dev` (the simulator, the
trace recorder and player, the fuzzer, the vector writer, `arkc`).

## The vectors

Printed in the JSON dialect of `rust/ark/tests/support/json.rs` under a
type (`{"$int": …}`, `{"$bytes": …}`, `{"$id": …}`, text as itself); a row
is a tuple, printed as an array in schema order. `RUNNERS.md` gives the
plainer form of `protocol/`, `trace/`, `rebase/` and `text/`. Every directory but
`hash/` has a `falsify/` vector a conformant runner must fail.

| directory | written by | one vector is | holds | since |
|---|---|---|---|---|
| `hash/` | `rust/ark/tests/hash.rs` | a store and its hash under its schema; `leaves-and-digests`, each table's digest and the state hash | §1.1, §1.3 | K1; unmoved by K2 |
| `eval/` | `rust/ark/tests/eval.rs` | a function's bytes, a store, inputs applied in turn: the changes, the rows, the hash, or the refusal; `checks` and `form-check`, the boundary's verdicts and words; `has-role`; `raw-*`, the authority's writes as functions; `list-ops`, the list editors' edges | §1.1, §4, §5 | L1; unmoved by K2; A3 re-derived the functions' bytes and hashes (a declaration carries its names), every state unmoved |
| `verify/` | `rust/ark/tests/verify.rs` | a module's functions and whether they check; `refused/` one per refusal, with its words; `scoped-ok`, a scope | §3, §6 | L1; unmoved by K2; A3 re-derived the functions' bytes and hashes |
| `module/` | `rust/ark/tests/module.rs` | the demo module's bytes and hash; `de-bruijn`, two spellings and one hash | §2, §6 | L1; unmoved by K2; A3 re-derived (the declarations' names) |
| `views/` | `rust/ark/tests/views.rs` | a store, a query and a script of batches: after each, the answer and the patches; `top-two-by-pos`, `group-source`, `playlist-with-items` and the rest of v4's shapes, and the maintained sum's overflow both ways | §1.10, §5 | V1; unmoved by K2; A3 re-derived the query's bytes and hash, every answer unmoved |
| `protocol/` | `rust/ark-dev/tests/vectors.rs` | one frame of each kind each side writes, in each space it is sent in: its bytes and the same value as JSON under the frame type | §1.11 | D1; K2 renumbered the frames to eight (`Fetch`, `Bodies` and `Push`'s body gone) and re-derived every one carrying an entry; K3a made them four (`Page` for `Push`, `Join`, `Batch`; `Verdict` for `Ack`, `Reject`, `Held`) and re-derived every one; A3 re-derived those naming a function |
| `trace/` | `rust/ark-dev/tests/vectors.rs` | one client's life in one session, step by step: what its two machines were given and said, and where they stood | §1.7, §1.9 | D1; K2 re-derived all (ids from an origin, `open` and `sign_in` carrying it); A3 re-derived all (the functions' bytes), every state unmoved |
| `rebase/` | `rust/ark-dev/tests/vectors.rs` | a fleet run by the simulator from a seed (Rust's only: `RUNNERS.md`): `fleet-seed-7`, its script and where the hub stands settled, and the five `fleet-fuzz-seed-N` sessions the fuzzer found a kernel fault with, each where it stands once fixed | §1.6, §1.7 | D1; K2 re-derived all (`fleet-seed-7` `ff04f56d…`); A3: `fleet-seed-7`'s script names the new hash, its head and state hold; the fuzz sessions' functions declare nothing, so they hold |
| `text/` | `rust/ark-dev/tests/vectors.rs` | the demo as ArkLang (`demo.ark`, `rust/ark-dev/src/demo.ark`) beside its module file and hash (`RUNNERS.md`) | §1.15 | A3 |
