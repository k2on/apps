# The Swift runtime against GENERATED.md

`ArkDB` is the Swift runtime of `spec/GENERATED.md`. This file lists every
place its spelling could differ from the contract, and every decision taken
where the contract or the spec was silent.

## Spelling deviations from GENERATED.md

**None.** Every name the contract lists exists with the spelling it gives:

| the contract says | here |
|---|---|
| `Value.null()`, `.bool(b)`, `.int(i64)`, `.text(s)`, `.bytesHex("0a0b")`, `.idHex("…")`, `.list([…])`, `.record([("k", v)…])` | as written (`int` takes `Int64`; the cases are also constructors, so `Value.record(["k": v])` works too); plus `.opt(Value?)`, `.bytes([UInt8])`, `.id(Id)` for the typed call surface |
| `v.isNull()`, `asBool()`, `asInt()`, `asText()`, `asList()`, `field("k")` | as written, `fatalError` on a mismatch (also `asBytes()`, `asId()`, `asRecord()`) |
| `Fault.refuse(text)`, `Fault.bug(text)` | as written; `Fault.refuse` also takes a `Value` (`throw Fault.refuse(Value.text("…"))`) |
| `Ops.add/sub/mul/div/mod/neg`, `Ops.cmp(CmpOp.Lt, a, b)`, `Ops.not`, `Ops.arg(args, "name")`, `Ops.match(opt, some, none)`, `Ops.map/filter/any/all/sortBy/fold` | as written; enum cases are lowercase (`CmpOp.lt`, `Dir.asc`), which the coordinator's transcript uses |
| `Std.trim` … `Std.unwrapOr` (28) | as written, each `throws` |
| `db.get`, `db.exists`, `db.select(plan)`, `db.put`, `db.delete` | `protocol Store`; `get`/`exists`/`select` return `Value`, `put`/`delete` `throws` |
| `Plan.from(t).filter(p).orderBy(c, Dir.Asc).limit(n).related(name, parent, child, column, childPlan)`; `Pred.cmp/inList/all/any/not` | as written; right-hand sides are `Value`s |
| `Ctx { user, session }`, `Args = [String: Value]` | `Ctx(user:session:)`, both `String`; `Args` is `[String: Value]` |

## Names the contract does not fix

- `Value`'s cases: `null, bool, int, text, bytes, id, list, record`, with
  `Id` (16 bytes, `uuid` text, `Id(uuid:)`, `Id(bytes:)`, `Id.nil_`).
- IR (`IR.swift`): `Ty.enumOf` / `Ty.structOf` (keywords otherwise);
  `Stmt.sLet/sIf/sFor/sPut/sDelete/sRefuse/sReturn`; `Expr.variable(Sym)`
  for `EVar`, `.structOf` for `EStruct`, the rest by the Haskell constructor
  without its `E`; `Pred.pcmp/pin/pall/pany/pnot` (the wire's tags), with
  the contract's `Pred.cmp/inList/all/any/not` as builders over them;
  `OrderBy(column, dir)` in place of the pair.
- `Ark.Decode.DecodeError` is `ModuleDecodeError` (path, what); `DecodeError`
  is `Ark.Canon`'s.
- `Ark.Protocol`'s frame codecs live in `enum Wire` (`Protocol` is an
  Objective-C runtime type under Foundation on Apple platforms);
  `clientMutate`/`clientRecv` are `Client.mutate`/`Client.recv`. Frame enums
  carry labelled payloads (`.batch(scope:items:hasMore:)`) with
  `BatchItem`, `FactsItem`, `ClosureItem` for the tuples.
- `Ark.Store`'s `Store` value is `MemoryStore` (a `final class`, per the
  brief); the interpreter's structured writes are `tryPut`/`tryDelete`
  returning `Result<Change?, Refusal>`, and the `Store` protocol's
  `put`/`delete` throw `Fault.refuse(refusal.text)`.
- `View.push` is a mutating method returning the patches; `Patch.value` is
  the vectors' encoding of a patch; `Patch.splice` is `splice`.
- `Replica`, `Authority`, `Log` are structs with mutating methods
  (`receive`, `ack`, `sequenceEntry` …); `localCommit(&authority, &replica)`
  is a free function.

## Decisions where the contract or the spec was silent

- **Record keys are Swift `String`s.** `Value.==` and `compareValue` are
  structural on Unicode scalars, as the spec demands (`"e\u{301}" !=
  "é"`), and field names are compared by scalar too. But a `[String: Value]`
  is keyed by Swift's `String`, whose hashing folds canonical equivalence,
  so two *field names* that differ only by normalisation would be one key.
  Rows and struct fields are identifiers in practice; nothing in the spec's
  vectors, or in a domain, names a field two ways. Documented rather than
  paid for with a custom key type on every row.
- **`Refusal.text`** renders a store refusal as `Ark.Store`'s `Show` would
  (`UniqueViolation "t" ["c"]`, `NotNull "t" "c"`, …) and a mutator's
  `refuse`/arithmetic fault as its bare text. That is what a `Fault.refuse`
  from `db.put` carries; the spec does not say how a structured refusal
  becomes text on the client side, and the server's `Reject` frame uses
  Haskell's `show`.
- **`Ops.arg` is a bug, not a fault, when the argument is missing**
  (`fatalError`), because the contract's spelling calls it without `try`.
  Likewise `MemoryStore.select` fatal-errors on a plan whose right-hand
  sides are not literals or whose table the schema lacks — a verified
  module never produces either, and generated code cannot `try` there.
- **Bug texts** (`Fault.bug`) name the Haskell constructor (`MissingArg x`,
  `TypeError expected Bool, got …`) but are not held to `show`'s exact
  spelling; no vector expects a bug.
- **Stores held by a `Replica` or `Authority` are immutable by convention:**
  every apply returns a new `MemoryStore` (a clone of a copy-on-write
  dictionary), so a `Replica` copy is independent of its original as the
  spec's value is. Writing through `replica.view.put(…)` directly would
  break that; generated code writes through the store the interpreter hands
  it, never one a replica holds.
- **`Eval.select` is generic over any `Store` through `scan`**, and
  `MemoryStore.scan` sorts its rows by key under `compareValue`, which is
  what makes the sort's ties fall in key order as `Ark.Store.scan` does.
- **`View.hydrate`/`push` take any `Store`**; the vectors run them over a
  `MemoryStore` advanced with `applyChange` before each push, as the spec's
  session does.
- **`eval/add-to-playlist.json`'s `function_hash`** (`48e1632f…`) is the
  hash of `add_to_playlist` *as authored*, before `Ark.Verify.completeOrders`
  appended the key columns to its plan's order — `Vectors.hs` hashes the raw
  `addToPlaylist` there, while the module in that same file and every entry
  in `protocol/` and `rebase/` carry the verified form's hash
  (`b9d99ed0…`). The runner reproduces both: it checks the module's closure
  hashes to `b9d99ed0…` (through `protocol/` and `rebase/`) and holds the
  eval vector's number to the authored form, printing a note. Worth fixing
  in the spec's emitter (`closure m addToPlaylist` → the verified function).
- **Not implemented, deliberately:** the server machine, `Ark.Sim`,
  `Ark.Live`'s rooms (the `Say`/`Heard` frames are there), `Ark.Verify`
  (a runtime executes only verified modules; the verifier is `arkc`'s), and
  `Ark.Peer.adopt`. So `rebase/fleet-seed-7.json` is not run: it needs the
  server and the seeded network. Everything else under `vectors/` is.

## Building

Only through nix, from the repository root:

    cd swift && nix develop ..#swift -c tools/swift.sh build
    cd swift && nix develop ..#swift -c tools/swift.sh run ArkDBTests

`tools/swift.sh` puts `tools/nix-swiftc` in `SWIFT_EXEC` — the platform
quirk from the brief (`-target x86_64-unknown-linux-gnu`, and the
toolchain's clang resource headers for Foundation's C modules) has to reach
the manifest compile too, which `-Xswiftc` does not — and puts the Swift
runtime on `LD_LIBRARY_PATH`, which the manifest binary and the test binary
both need for `libdispatch.so`. Both paths are read from the devshell's own
environment.

The test target is an **executable** (`swift run ArkDBTests`, exit status
non-zero on any failure) because the `swift` devshell carries no XCTest.
The library target uses Foundation alone and no Linux-only API, so it
compiles for iOS as it stands; the `UnicodeTables.swift` it embeds is the
spec's, unchanged.
