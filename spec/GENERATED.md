# What generated code is made of

`arkc gen rust|swift|kotlin` writes the domain as ordinary source in each
language from one emitter, so the three outputs differ in syntax and in
nothing else. This file is the contract between that emitter and the three
runtimes: the names generated code calls, spelled once, with each
language's spelling beside it. A runtime is conformant when generated code
compiles against it and the `eval/` vectors pass through it.

Generated code is deliberately *dynamic*: it computes over `Value` and calls
a small library, because the priority is that three generated bodies mean
exactly what `Ark.Eval` says, and a typed, idiomatic surface can be laid
over that later without touching the meaning. What the app sees as typed
is the call surface (`<fn>Args(...)`), the ids, and the module constants.

## Spelling

| the contract says | Rust (`use ark::gen::*`) | Swift (`import ArkDB`) | Kotlin (`import dev.arkdb.*`) |
|---|---|---|---|
| a local | `let v3: Value = e;` | `let v3 = e` | `val v3 = e` |
| `if c { } else { }` | `if c.asBool() { } else { }` | `if c.asBool() { } else { }` | `if (c.asBool()) { } else { }` |
| `for x in xs { }` | `for v3 in xs.asList() { }` | `for v3 in xs.asList() { }` | `for (v3 in xs.asList()) { }` |
| return nothing / a value | `return Ok(());` / `return Ok(e);` | `return` / `return e` | `return` / `return e` |
| a call that may fault | `f(a)?` | `try f(a)` | `f(a)` |
| a fallible closure `\x -> e` | `\|v3\| -> Result<Value, Fault> { Ok(e) }` | `{ v3 in e }` | `{ v3 -> e }` |
| string literal | `"…"` | `"…"` | `"…"` |

A fault is `Fault`, with two constructors that are never conflated:
`Fault.refuse(text)` is a verdict, `Fault.bug(text)` is a bug. In Rust a
fault is `Err(Fault)`; in Swift and Kotlin it is thrown.

## The library

Every name below exists in every runtime with these semantics; the
Haskell modules named are the meaning.

**Value** (`Ark.Value`) — constructors, all returning `Value`:
`Value.null()`, `Value.bool(b)`, `Value.int(i64)`, `Value.text(s)`,
`Value.bytesHex("0a0b")`, `Value.idHex("32 hex digits")`,
`Value.list([v…])`, `Value.record([("k", v)…])`.
Accessors, which **fail fatally on a type mismatch** (a verified module
never mismatches, so this is a bug and not a fault): `v.isNull()`,
`v.asBool()`, `v.asInt()`, `v.asText()`, `v.asList()`, `v.field("k")`.
Rust: `Value::int(1)`, `v.as_bool()`… — snake case is the one spelling
difference, applied uniformly.

**Ops** (`Ark.Eval` §6.4, §6.5) — `Ops.add(a,b)`, `sub`, `mul`, `div`,
`mod`, `neg(a)`: checked, fault `refuse("integer overflow")` /
`refuse("division by zero")`. `Ops.cmp(CmpOp.Lt, a, b)` → `Value.bool` under
the total order. `Ops.not(a)`. `Ops.arg(args, "name")` → the argument,
bug if missing. `Ops.match(opt, some: (v) -> Value, none: () -> Value)`.
`Ops.map(xs, f)`, `Ops.filter(xs, f)`, `Ops.any(xs, f)`, `Ops.all(xs, f)`,
`Ops.sortBy(xs, key)` (stable), `Ops.fold(xs, init, f(acc, x))` — closures
may fault. `and`/`or` are emitted as the language's short-circuit
operators over `asBool()`, wrapped in `Value.bool`.

**Std** (`Ark.Std`) — one function per `Ark.IR.StdFn`, camel-cased from the
constructor (`Std.trim`, `Std.isEmpty`, `Std.concat`, `Std.lower`,
`Std.isAlnum`, `Std.chars`, `Std.textLen`, `Std.startsWith`,
`Std.splitOnce`, `Std.textOfInt`, `Std.hex`, `Std.min`, `Std.max`,
`Std.clamp`, `Std.abs`, `Std.fnv1a64`, `Std.sha256`, `Std.idOfText`,
`Std.textOfId`, `Std.nilId`, `Std.utf8`, `Std.first`, `Std.last`, `Std.len`,
`Std.contains`, `Std.reverse`, `Std.isSome`, `Std.unwrapOr`), each over
`Value`s, each may fault. The three Unicode functions use the generated
tables (`spec/generated/unicode/`) and nothing of the platform.

**Store** (`Ark.Store`, `Ark.Eval` §6.6) — an interface the runtime's
store implements: `db.get(table, [key…])` → the row or `Value.null()`;
`db.exists(table, [key…])` → `Value.bool`; `db.select(plan)` → `Value.list`
of nodes exactly as `Ark.Eval.select` builds them; `db.put(table, row)` and
`db.delete(table, [key…])`, which fault with the store's refusal.

**Plan** (`Ark.IR`) — `Plan.from(table).filter(pred).orderBy(col, Dir.Asc)
.limit(n).related(name, parent, child, column, childPlan)`; `Pred.cmp(col,
CmpOp.Eq, v)`, `Pred.inList(col, [v…])`, `Pred.all([p…])`, `Pred.any([p…])`,
`Pred.not(p)`. Right-hand sides are `Value`s, already evaluated.

**Ctx** — `ctx.user`, `ctx.session` as `Value.text`.

## Only what a peer calls

`arkc gen … --only create_playlist,add_to_playlist,library` emits those
functions and the helpers they reach, and a dispatch that knows only those.
A peer built from it applies every other entry by facts, which the
authority keeps beside each entry, and ends in the same state — so a phone
that never authors `add_track` carries no code for it and still shows every
track. The module bytes and hash are always the whole module's.

## The generated file

One file per module, named after it (`harken_gen.rs`, `HarkenGen.swift`,
`HarkenGen.kt`), containing in this order:

1. a header: generated by `arkc`, from which module hash; do not edit;
2. `MODULE_BYTES` — the module's canonical CBOR, as a hex string constant,
   and `MODULE_HASH`;
3. one function per helper: `fn slug(a: Value) -> Value` (positional
   arguments, may fault);
4. one function per mutator: `fn add_to_playlist(db, ctx, autos, args)`
   (Rust `-> Result<(), Fault>`, Swift/Kotlin `throws`/plain). In Rust
   `db` is `&mut Db` for a mutator and `&Db` for a query: `ark::gen::Db` is
   an overlay over a store that records the transaction, so a body that
   faults commits nothing (`ark::gen::run_mutator`); Swift and Kotlin pass
   the `Store` protocol/interface and the runtime's `Db` conforms to it;
5. one function per query: `fn library(db, args) -> Value`;
6. `apply(fnHashHex, db, ctx, autos, args)`: a `match` / `switch` / `when`
   over every mutator's closure hash (lowercase hex), calling it; an
   unknown hash is `bug("unknown function …")`;
7. `query(name, db, args)`: the same over queries by name;
8. `FUNCTIONS`: the list of `(name, hash)` for every function;
9. the typed call surface: for each mutator `fn <name>Args(...)` taking
   typed parameters and returning the `Args` map, so an app never spells a
   field name. Types: `Int`→`i64`/`Int64`/`Long`, `Text`→`String`,
   `Bool`, `Bytes`→`Vec<u8>`/`[UInt8]`/`ByteArray`, `Id t`→`Id` (the
   runtime's 16-byte type), `Option t`→`Option`/`?`/`?`, `List t`→`Vec`/`[]`/
   `List`, `Enum`→`String`, `Struct`→`Value`;
10. the live frame types, as plain structs/enums with a `toValue`/`fromValue`
    pair, so no client mirrors them by hand.

Rust wraps everything in a module; Swift in `public enum <Name>Gen`; Kotlin
in `object <Name>Gen`. Symbol names come from the author's names where the
module carries them and are `v<n>` otherwise.
