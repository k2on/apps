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

## Added beside the contract: a transaction for generated code, and `ark-client`

Three additions to `ark-runtime`, none of them changing anything the
contract or the vectors already pinned:

- **`TransactionStore`** (`Transaction.kt`): the `Store` a generated mutator
  writes through when it runs in place of the interpreter — GENERATED.md's
  "overlay over a store that records the transaction, so a body that faults
  commits nothing". It is backed by a *fork* of the base, not a diff, for the
  reason `Eval.applyClosure` works on a fork: every constraint is decided in
  `MemoryStore.write`, once, and a diff store would decide them a second
  time against a merged picture. `TransactionStore.run(base) { db -> … }`
  is the mirror of `applyClosure`: `Eval.Applied.Ok(store, changes)`, or
  `Refused` on a `Fault.Refuse`, with a `Fault.Bug` propagating.
- **`Replica.mutateWith(i, ctx, fh, autos, args) { db -> … }`** beside
  `mutate`: the body runs through a `TransactionStore` over the optimistic
  store and the entry is recorded exactly as `mutate` records one. The
  replica must still hold the closure `fh` names, because a rebase replays
  pending intents through the interpreter (§11.6).
- **`Client.mutateWith(scope, …)`**, the same one frame up, pushing the entry
  when linked.

`ark-client` (package `dev.arkdb.client`) is the peer around the machine:

- `Link` — a `Transport` driving a `Client`: on open `connected()`, frames
  sent as `Canon.encode(msg.toValue())`, received through
  `Protocol.serverFromValue(Canon.decode(bytes))` into `recv`; reconnect
  with backoff 500 ms doubling to 30 s, reset by a connection that opened;
  a `Denied` stops it. `pump()` is the only place the machine is touched,
  from the caller's timer.
- `WebSocketTransport` over OkHttp 4.12 (resolved through the proxy), and
  `InMemoryTransport` + `LocalHub`: an in-process authority host (hello,
  push, need_facts, verify, fan-out) carrying the very same frame bytes,
  which is what the tests run the `Link` over.
- `Session` — the replicas (one per scope, `Whole`), the `Client`, a `Link`
  when there is a server and an `Authority` per scope when there is not
  (`localCommit` after every mutation, §3.10), and one canonical-CBOR file
  per scope (confirmed store, cursor, pending, and alone the log, written
  temp-and-rename). `mutate(name, args)` runs the **interpreter**;
  `mutateWith(name, args) { db -> Gen.f(db, ctx, autos, args) }` runs the
  **generated body**; both draw autos the same way (a random id per `NewId`,
  the clock per `Now`) and record the same entry. `query(name, args)` is
  `Eval.queryClosure` over the merged view; `read { db -> … }` hands the
  same store to a generated query. A change listener is called after every
  mutation and every frame that moved a view; `verify()`, `status`.
- Its tests (`gradle :ark-client:clientTests`, part of `check`), on the demo
  module: a peer alone creating and adding and restarting to the same hash
  (and refusing a store that is not its log's); two peers through the hub
  with the offline add landing last after the rebase, one of them authoring
  through generated code; the link's backoff and denial; generated code
  against the interpreter, same entries, same changes, same hash, same
  refusal; and the transaction committing nothing on a fault.

Alone, the authority is rebuilt on open by `Authority.adopt` over the log the
peer kept, which replays every intent and checks the hash; a store that
does not match its own log is refused there rather than trusted.

## Not covered

- `rebase/fleet-seed-7.json` is skipped by the runner: replaying it needs
  `Ark.Sim` and the server machine of `Ark.Protocol`, which this runtime
  does not carry (the task asked for the client machine only). Every other
  vector, and every `falsify/` vector, runs.
- The hash vector carries no schema; the runner reads the demo schema from
  `eval/add-to-playlist.json` for it.
