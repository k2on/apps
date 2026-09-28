# The Kotlin runtime

`ark-runtime` (package `dev.arkdb`) is ArkDB for the JVM and Android: the
IR at spec version 2, its encoder and decoder, the verifier, the
interpreter (`Eval`), hashes, the store, the peer, the protocol and views.
`ark-runtime`'s `dev.arkdb.authoring` is the **authoring vocabulary** in
Kotlin — the contract is `spec/AUTHORING.md`, and this package is the
reference for its Kotlin spelling (§6). A domain written against it is two
programs at once: run under Emit it is the module (`Module.emit()`,
`.hash`, `.ir`), run under Native it applies entries directly
(`Module.procedures()`, which a `Replica`, an `Authority` and a
`Session` take by hash). Nothing is generated for Kotlin any more; `arkc gen
kotlin` prints the authoring form itself. `ark-client` is a peer's shell:
the socket, the pump, the files, and `Session`.

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
A row, a scope and an input are classes whose constructor takes their
columns, tables or fields in declaration order, with a companion
(`Row.Of`, `Scope.Of`, `Input.Of`) carrying the rest; the runtime reads
them by reflection (`java.lang.reflect` and `kotlin.reflect.typeOf`, no
kotlin-reflect), which Android runs unchanged.
