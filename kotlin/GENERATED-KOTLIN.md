# The Kotlin runtime against `spec/GENERATED.md`

`ark-runtime` (package `dev.arkdb`) is the library generated Kotlin compiles
against. Every name the contract lists exists with the contract's spelling
and semantics: `Value.bool/int/text/bytesHex/idHex/bytes/id/list/record/opt`,
`isNull/asBool/asInt/asText/asList/field`, `Ops.add/sub/mul/div/mod/neg/cmp/
not/arg/match/map/filter/any/all/sortBy/fold`, the twenty-eight `Std.*`,
`Store.get/exists/select/put/delete`, `Plan.from(...).filter(...).orderBy(...,
Dir.Asc).limit(n).related(...)`, `Pred.cmp/inList/all/any/not`, `CmpOp.Eq…Ge`,
`Dir.Asc/Desc`, `Ctx(user, session)`, `typealias Args = Map<String, Value>`,
`Fault.refuse(Value | String)`, `Fault.bug(String)`.

## Spelling deviations from the contract

One, and it is Kotlin's rather than this runtime's:

- **`Value.null()` must be written `` Value.`null`() `` in Kotlin.** `null`
  is a hard keyword and cannot be an identifier without backticks. The
  function exists under exactly that name (`` fun `null`(): Value ``), so the
  emitter's one job is to put the backticks on the call. The Rust and Swift
  spellings are unaffected.

Everything else generated code is documented to call compiles as written,
including the exact lines the coordinator quoted:

```kotlin
val v2 = db.select(Plan.from("playlist_item").filter(Pred.cmp("playlist_id", CmpOp.Eq, Ops.arg(args, "playlist_id"))).orderBy("pos", Dir.Desc).limit(1))
val v4 = Std.unwrapOr(Ops.match(Std.first(v2), { v3 -> (v3).field("pos") }, { Value.`null`() }), Value.int(0L))
db.put("playlist", Value.record(listOf("id" to Ops.arg(autos, "id"), "name" to Std.trim(Ops.arg(args, "name")), "user_id" to Value.text(ctx.user))))
if ((v0).asBool()) { return }
throw Fault.refuse(Value.text("a playlist needs a name"))
```

`ark-runtime/src/test/kotlin/dev/arkdb/conformance/DemoGen.kt` is the demo
module's two mutators written in exactly that form, and the conformance
runner drives them through the `eval/` vector beside the interpreter.

## What `spec/src/Ark/Gen.hs` emits today that this surface does not serve

Held against the emitter as it stood at 13:58 (not against the contract),
two lines of its Kotlin output will not compile against any Kotlin runtime
that follows `GENERATED.md`, and are the emitter's to fix:

- `ENone`/`VNull` → `valueCtor t "null" []`, i.e. `Value.null()`: needs the
  backticks (`` Value.`null`() ``) in Kotlin, as above.
- `plan` → `".order_by(" …` for every target, where the contract, the
  coordinator's quoted line and this runtime all spell it `.orderBy(`. No
  `order_by` alias is provided here, so that the contract stays the one
  spelling.

Everything else the emitter writes for Kotlin (`Ops.arg`, `Ops.match`,
`Ops.fold(xs, z) { acc, x -> … }`, `Ops.<camel>(xs) { x -> … }`,
`Std.<lowerFirst>`, `Pred.inList`, `CmpOp.Eq`, `Dir.Asc`, `Value.bytesHex`,
`Value.idHex`, `Value.int(…L)`, `Value.int(Long.MIN_VALUE)`,
`Value.record(listOf(k to v))`, `Value.opt(v?.let { x -> … })`,
`db.get/exists(table, listOf(…))`, `fun apply(fnHash: String, db: Store,
ctx: Ctx, autos: Args, args: Args)`) resolves against this module.

## Names the contract leaves open (this runtime's choices)

- `Value`'s subclasses are `VNull, VBool, VInt, VText, VBytes, VId, VList,
  VStruct`, as `Ark.Value` names its constructors; the contract only names
  the constructors and accessors, which are as written there.
- `Id` is the runtime's sixteen-byte type (`Id(bytes)`, `Id.ofHex`,
  `Id.ofText`, `.text`, `.hex`, `Id.nil`); `FnHash` the thirty-two-byte one.
- The IR's plan with expressions is `IR.Plan` / `IR.Pred` / `IR.Related`;
  the contract's `Plan` (values already evaluated) is the top-level `Plan`,
  which is also what a `View` maintains (`ViewPlan` and `Filter` are
  typealiases for it and `Pred`).
- `Store` is an interface with the five contract operations plus `schema`,
  `scan` and `applyChange`; `MemoryStore` is the spec's map of maps.
  `MemoryStore.fork()` is how a caller gets a value: `Eval.applyClosure`
  runs a mutator on a fork and returns it with its changes, so a refusal
  leaves the original untouched (the whole entry rolls back).
- `Plan.filter` called twice narrows (both predicates must hold) rather than
  replacing; the contract calls it once.
- `Canon.DecodeError` and `Decode.DecodeError` are nested in their objects,
  as the two Haskell modules each have one.
- `Replica`, `Authority`, `Log` and `Client` are objects whose methods move
  them (the spec's are values). Stores inside them are still values: every
  store a replica holds came out of `applyClosure` or `applyChanges` on a
  fork and is never written in place.
- Bug texts (`Fault.Bug`) follow Haskell's `show` of `EvalError` closely
  (`UnknownFunction "f"`, `MissingArg "a"`, `TypeError "expected Int, got …"`)
  but are not held byte-exact: a vector never expects a bug.

## Not covered

- `rebase/fleet-seed-7.json` is skipped by the runner: replaying it needs
  `Ark.Sim` and the server machine of `Ark.Protocol`, which this runtime
  does not carry (the task asked for the client machine only). Every other
  vector, and every `falsify/` vector, runs.
- The hash vector carries no schema; the runner reads the demo schema from
  `eval/add-to-playlist.json` for it.
