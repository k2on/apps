# ArkDB for Swift

The Swift runtime of the specification (`spec/`), and the Swift spelling of
its authoring vocabulary. The contract is **`spec/AUTHORING.md`**: read it
first; this file only says where each part of it lives here and what is
Swift's own.

| product | what it is |
|---|---|
| `ArkDB` | IR v2 (`IR.swift`), its wire form (`Encode`, `Decode`), `Hash` (closures reach helpers and middleware), `Eval` (checks, middleware in `fnUses` order, `insert`/`upsert`/`update`, queries that refuse, the form validator `Eval.check`), the v2 verifier rules and `completeOrders` (`Verify.swift`), the store, the peer (`Replica`, `Authority`, `Procedure`), the protocol |
| `ArkAuthoring` | §2's vocabulary — `Bool Int Text Bytes Id<T> Opt<T> List<T>`, `Row`, `Scope`, `Table`, `Col`/`col`, `Rel`/`rel`, `Columns`, `Input`/`object()`/field builders and `.why`, `router`/`guard`/`provide`/`input`/`mutation`/`query`/`routes`, `Module`, `when unless ifElse pick forEach refuse`, `Opt.mapOr/map/filter/orRefuse`, `ctx.user/session/now/newId`, `helper` — behind one API with two backends, **Emit** (records IR) and **Native** (runs against a transaction), chosen by the ambient context a procedure runs under |
| `ArkDBClient` | the session, the link, files, an in-process server |

A domain is ordinary Swift written against `ArkAuthoring` — the files
`arkc gen swift` prints. `module().emit()` is its `.ark`, `module().hash`
its hash, and `module().procedures()` every procedure natively by function
hash; `Session.open(directory:module:procedures:…)` hands those to every
replica, which applies an entry natively when it holds its hash and
through the interpreter (or the entry's facts) otherwise. Nothing is
generated for this runtime any more.

Three domains are authored here: the spec's demo (`Tests/Demo`, Appendix
B), whose `emit()` the runner holds to `spec/vectors/module/demo.json` byte
for byte; harken's phone domain (`../harken/domain/gen/swift`, compiled as
the `HarkenDomain` test target and, with the iOS bridge, as `HarkenPhone`),
whose procedure hashes it holds to `harken/domain/harken.ark` when that is
a spec-2 module; and `Tests/Kitchen`, the rest of the vocabulary. `Native`
is held to `Eval` over its own `Emit` on every procedure of all three.

## What is Swift's own

- **A module of its own.** `ArkAuthoring` is not a folder of `ArkDB`: its
  `Bool`, `Int` and `Id<T>` must shadow the standard library's and
  `ArkDB.Id` in a domain file, and a module-level `Int` inside `ArkDB`
  would do the same to every app that imports it. A domain file imports
  `ArkAuthoring` alone; a file that needs both writes `ArkDB.Id`,
  `ArkDB.Ctx`, `ArkDB.Module` (and `Swift.Int`) qualified.
- **Rows, inputs and scopes are plain structs.** `Row` and `Input` refine
  `Codable`; the compiler synthesises the coding, and the vocabulary uses
  it to build a row from an expression or a value and to take one apart,
  with nothing written by the author. A property `userId` is the IR's
  `user_id`. The declarations sit in extensions beside the struct
  (`extension Playlist: Row { … }`, `extension Playlist { static let id =
  col<Playlist, Id<Playlist>>("id") }`) as Rust's `impl` blocks do; `col`
  and `rel` are generic typealiases, since Swift cannot specialise a
  generic function explicitly.
- **`Columns<Self>()`, not `columns()`,** starts a row's column list: inside
  `static func columns()` an unqualified `columns()` is the method itself.
- **A key is positional** (`db.playlistItem.delete(playlist.id,
  input.trackId)`), typed by the row's `typealias Key` (`Id<Track>`, or a
  tuple), so a wrong order does not compile.
- **Closures are trailing**: `.mutation("create_playlist") { ctx, db, input
  in … }`; a provide annotates its input `{ (ctx, db, input: PlaylistId) in
  … }`; a multi-statement body `return`s its effect.
- **A stopped native body is inert.** The vocabulary does not `throw` (a
  domain file has no `try`), so a refusal under `Native` — `refuse`, a
  failed `orRefuse`, an overflow — records the verdict and every later
  operation answers the zero of its type without touching the store; the
  runner reports the verdict. `and`, `or`, `pick` and `mapOr`'s default are
  autoclosures, so only the arm `Eval` would evaluate is evaluated.
- **Writes wait one step for `.on`.** Natively an `insert`/`upsert` is made
  at the next operation (or at the end), so `.insert(row).on(cols)` means
  what it reads as.
- **Two inference limits.** `fold`'s initial value needs its type when
  the closure alone cannot give it (`.fold(Int(0)) { acc, row in … }`),
  and an input with a field named `id` hides the `id(T.self)` builder in its
  own `schema` (write `ArkAuthoring.id(T.self)` there).
- **`Input.init(args:)` and `.args`** convert an input to and from an
  entry's arguments, for an app outside the domain's module.
- Not verified: the expression type checker stays `arkc`'s; `Emit` leaves
  `fnNames` empty (the printer derives names, §6); `live` frames are not
  authored here.

## Building

Only through nix, from this directory:

    nix develop ..#swift -c tools/swift.sh build
    nix develop ..#swift -c tools/swift.sh run ArkDBTests

`tools/swift.sh` puts `tools/nix-swiftc` in `SWIFT_EXEC` (the platform's
target triple and clang resource headers) and the Swift runtime on
`LD_LIBRARY_PATH`. The test target is an executable, since the devshell has
no XCTest; it exits non-zero on any failure. `rebase/fleet-seed-7.json` is
not run: it needs the server machine and the simulation, which this
runtime does not carry.
