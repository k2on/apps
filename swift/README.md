# ArkDB for Swift

The Swift runtime of the specification (`spec/`), and the Swift spelling of
its authoring vocabulary. The contract is **`spec/AUTHORING.md`**: read it
first; this file only says where each part of it lives here and what is
Swift's own.

| product | what it is |
|---|---|
| `ArkDB` | IR v3 (`IR.swift`: one set of tables, one log), its wire form (`Encode`, `Decode`), `Hash` (closures reach helpers and middleware), `Eval` (checks, middleware in `fnUses` order, `insert`/`upsert`/`update`, queries that refuse, the form validator `Eval.check`), the verifier's structural rules, the schema's well-formedness (`Schema.problems`) and `completeOrders` (`Verify.swift`), the store, the peer (`Replica`, `Authority`, `Procedure`), the protocol's frames, its client and `refusalText` |
| `ArkAuthoring` | §2's vocabulary — `Bool Int Text Bytes Id<T> Opt<T> List<T>`, `Row`, `Tables`, `Table`, `Col`/`col`, `Rel`/`rel`, `Columns`, `Record`/`Fields`, `Input`/`object()`/field builders and `.why`, `router`/`guard`/`provide`/`input`/`mutation`/`query`/`routes`, `Module`, `when unless ifElse pick forEach refuse`, `Opt.mapOr/map/filter/orRefuse`, `ctx.user/session/now/newId`, `helper`, `evaluate` — behind one API with two backends, **Emit** (records IR) and **Native** (runs against a transaction), chosen by the ambient context a procedure runs under |
| `ArkDBClient` | the session over one replica (and where each intent it authored stands: `standing`), the link, the file, an in-process server holding the log's authority |

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
a spec-3 module; and `Tests/Kitchen`, the rest of the vocabulary. `Native`
is held to `Eval` over its own `Emit` on every procedure of all three.

A `Session` holds one replica of the log. `author(name:args:)` returns the
entry's id (or the local refusal), and `standing(id)` says where it is:
`.pending`, `.confirmed`, `.rejected(reason)` — the sentence the
authority sent (`refusalText`: a mutator's own refusal word for word, a
constraint named in a sentence) — or `.unknown`; `rejections` lists every
verdict with its `reason`. `InProcessServer` is the spec's `Server` without
live rooms: one `Authority`, one access rule, and `withOwns`, the sessions
a user owns, so that an entry authored under an older login of the same
user is accepted; any other entry whose actor or session is not the
connection's is rejected `"not yours"`, and a hello proving nobody (an
empty user) is denied.

A session can be opened signed out (`Session.openSignedOut`): everything is
authored as `Ctx.nobody`, kept pending and written to the directory, and no
connection is made. `signIn(user:session:token:)` makes every such intent
the signer's (`Client.signIn`, `Replica.signIn`), replays the view so the
rows say whose they are, writes the file and connects; each entry keeps its
id, so `standing` follows it through to confirmed or rejected. A directory
opened signed in over work nobody authored makes that work the signer's
the same way.

## What is Swift's own

- **A module of its own.** `ArkAuthoring` is not a folder of `ArkDB`: its
  `Bool`, `Int` and `Id<T>` must shadow the standard library's and
  `ArkDB.Id` in a domain file, and a module-level `Int` inside `ArkDB`
  would do the same to every app that imports it. A domain file imports
  `ArkAuthoring` alone; a file that needs both writes `ArkDB.Id`,
  `ArkDB.Ctx`, `ArkDB.Module` (and `Swift.Int`) qualified.
- **Rows, records, inputs and the tables are plain structs.** `Row`,
  `Record` and `Input` refine `Codable`; the compiler synthesises the
  coding, and the vocabulary uses it to build one from an expression or a
  value and to take one apart, with nothing written by the author. A
  property `userId` is the IR's `user_id`. The declarations sit in
  extensions beside the struct (`extension Playlist: Row { … }`,
  `extension Playlist { static let id = col<Playlist, Id<Playlist>>("id")
  }`) as Rust's `impl` blocks do; `col` and `rel` are generic typealiases,
  since Swift cannot specialise a generic function explicitly. The module's
  tables are one struct of `Table`s, `extension Harken: Tables { public
  static func open() -> Self { Harken(playlist: table(), …) } }`, and every
  router is over it: `router(Harken.self, "playlists")`. A module whose
  routers are over two different `Tables` types does not emit.
- **`Columns<Self>()` and `Fields<Self>()`, not `columns()` and
  `fields()`,** start a row's column list and a record's field list: inside
  `static func columns()` an unqualified `columns()` is the method itself.
  A record is a struct of vocabulary values that is no table's row —
  `extension AlbumsEntry: Record { public static func fields() ->
  Fields<Self> { Fields<Self>().field("art", text()).field("tracks",
  int()) } }` — whose type is the `TStruct` of those fields and whose
  construction is `EStruct`; its fields take the input builders, and no
  checks.
- **A helper is an ordinary function whose body is `helper(…)`,** the
  arguments named once beside their values and the closure's parameters
  inferred from them: `public func movementKey(_ workId: Text, _ no: Int)
  -> Text { helper("movement_key", ("work_id", workId), ("no", no)) {
  workId, no in concat(list([workId, "#", no.toText()])) } }`, up to six
  arguments. Under Emit a call is `ECall`, and the first call in an emit
  records the helper just before the first function that calls it (a
  helper it calls, before it); under Native it is the body on the values.
  `evaluate { movementKey(w, 2) }` runs one outside any procedure. The
  older `let double = helper("double", "x") { (x: Int) in … }` form, a
  closure to call, still works.
- **An option compares whole:** `opt.eq(some(x))`, `opt.ne(none(T.self))`
  are `ECmp` on the option, as every `Term` compares.
- **One auto name is one auto.** `ctx.now("added_ms")` in three rows of one
  entry is one frozen time; the same name drawn as another kind is a bug in
  the domain and stops the emit.
- **A key is positional** (`db.playlistItem.delete(playlist.id,
  input.trackId)`), typed by the row's `typealias Key` (`Id<Track>`, or a
  tuple), so a wrong order does not compile.
- **Closures are trailing**: `.mutation("create_playlist") { ctx, db, input
  in … }`; a provide annotates its input `{ (ctx, db, input: Owned) in
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
