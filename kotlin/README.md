# The Kotlin runtime

`ark-runtime` (package `dev.arkdb`) is ArkDB for the JVM and Android: the
IR at spec version 3 (one set of tables, one log), its encoder and
decoder, the verifier, the interpreter (`Eval`), hashes, the store, the
peer, the protocol — the client machine and the sans-io `Server` — and
views.
`ark-runtime`'s `dev.arkdb.authoring` is the **authoring vocabulary** in
Kotlin — the contract is `spec/AUTHORING.md`, and this package is the
reference for its Kotlin spelling (§6). A domain written against it is two
programs at once: run under Emit it is the module (`Module.emit()`,
`.hash`, `.ir`), run under Native it applies entries directly
(`Module.procedures()`, which a `Replica`, an `Authority` and a
`Session` take by hash). Nothing is generated for Kotlin any more; `arkc gen
kotlin` prints the authoring form itself. `ark-client` is a peer's shell:
the socket, the pump, the file, and `Session` — one replica of the log,
and, for every intent it authored, `statusOf(id)`: pending, confirmed at a
sequence, or rejected with the server's reason (`Protocol.refusalText`).
Its `LocalHub` is the runtime's `Server` in process, for tests.

    nix develop ..#kotlin -c gradle --no-daemon build     # from kotlin/

runs the conformance runner over `spec/vectors` (which includes the demo of
AUTHORING.md Appendix B authored in `ark-runtime/src/test/kotlin/demo`, held
to `module/demo.json`'s bytes, and every procedure of it and of
`harken/domain/gen/kotlin` run natively against the interpreter) and
`ark-client`'s tests. `build.sh` is the same without Gradle.

Kotlin's own rules shape three spellings, recorded in AUTHORING.md §6: a
file names `dev.arkdb.authoring.Int` and `.List` beside the star import
(Kotlin's default imports win over a star import), `when` is written
`` `when` ``, and a row's key type is `Key1`/`Key2`/`Key3` on the class.
A row, the module's tables and an input are classes whose constructor
takes their columns, tables or fields in declaration order; a row's and an
input's companion (`Row.Of`, `Input.Of`) carries the rest, and the tables
class implements `Tables` (a companion, if written, is a `Tables.Of` and
names nothing). Every router of a module is over the same tables class.
The runtime reads them by reflection (`java.lang.reflect` and `kotlin.reflect.typeOf`, no
kotlin-reflect), which Android runs unchanged.
