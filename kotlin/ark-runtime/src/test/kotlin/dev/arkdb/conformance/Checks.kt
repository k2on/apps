// The check of each vector kind, as spec/RUNNERS.md says it and
// rust/ark/tests/vectors.rs reads it: each throws `Failed` with its first
// unmet claim, so that one definition runs both ways — a vector must pass
// it and a `falsify/` vector must fail it.
package dev.arkdb.conformance

import dev.arkdb.Canon
import dev.arkdb.Ctx
import dev.arkdb.Decode
import dev.arkdb.Encode
import dev.arkdb.Eval
import dev.arkdb.EvalFault
import dev.arkdb.Facts
import dev.arkdb.Hash
import dev.arkdb.Hex
import dev.arkdb.MemoryStore
import dev.arkdb.Module
import dev.arkdb.Ran
import dev.arkdb.Raw
import dev.arkdb.Refusal
import dev.arkdb.SPEC_VERSION
import dev.arkdb.Sha256
import dev.arkdb.Value
import dev.arkdb.ValueOrder
import dev.arkdb.Verify
import dev.arkdb.checkSchema
import dev.arkdb.leaf
import dev.arkdb.stripModule

object Checks {
    fun bytesOf(v: Value): ByteArray = Hex.decode(v.asText())

    fun moduleOf(v: Value): Module = Decode.module(v)

    // codec ---------------------------------------------------------------

    /** `check_codec`: the value encodes to the bytes, and the bytes decode to the value. */
    fun codec(v: Value) {
        val value = v.field("value")
        val bytes = bytesOf(v.field("bytes"))
        ensureBytes(Canon.encode(value), bytes, "encode")
        ensureEq(Canon.decode(bytes), value, "decode")
    }

    // order ---------------------------------------------------------------

    /** `check_order`: the input sorted by the total order is the sorted list. */
    fun order(v: Value) {
        val input = v.field("input").asList().sortedWith(ValueOrder)
        ensureEq(Value.VList(input), v.field("sorted"), "sorted")
    }

    // hash ----------------------------------------------------------------

    // The sum of two 256-bit big-endian integers, modulo 2^256: written here
    // rather than taken from `Digest`, so that the runner checks the
    // construction (§8.1) and not the runtime's arithmetic against itself.
    private fun sum256(a: ByteArray, b: ByteArray): ByteArray {
        val out = ByteArray(32)
        var carry = 0
        for (i in 31 downTo 0) {
            val s = (a[i].toInt() and 0xff) + (b[i].toInt() and 0xff) + carry
            out[i] = s.toByte()
            carry = s shr 8
        }
        return out
    }

    /**
     * `check_hash`: the store's own state hash; and, where the vector pins the construction part by
     * part (`tables`), every leaf, every digest and the hash of the pairs, each from the canonical
     * encoding and SHA-256 alone.
     */
    fun hash(v: Value) {
        val m = moduleOf(v.field("module"))
        val st = MemoryStore.fromValue(m.schema, v.field("store"))
        val want = v.field("hash").asText()
        val tables = v.asStruct().fields["tables"]
        if (tables == null) {
            ensureEq(Hex.encode(Hash.stateHash(st)), want, "state hash")
            return
        }
        val names = m.schema.tables.map { it.name }
        val ts = tables.asList()
        ensureEq(ts.size, names.size, "one entry per table of the schema")
        val pairs = ArrayList<Value>()
        for ((t, name) in ts.zip(names)) {
            ensureEq(t.field("table").asText(), name, "the tables in schema order")
            var digest = ByteArray(32)
            val rows = t.field("rows").asList()
            ensureEq(rows.size, st.scan(name).size, "$name: every row of the store")
            for (r in rows) {
                val l = Sha256.hash(Canon.encode(Value.VText(name)) + Canon.encode(r.field("row")))
                ensureEq(Hex.encode(l), r.field("leaf").asText(), "$name: a row's leaf")
                ensureBytes(leaf(name, r.field("row").asStruct()), l, "$name: the runtime's leaf")
                digest = sum256(digest, l)
            }
            ensureEq(
                Hex.encode(digest),
                t.field("digest").asText(),
                "$name: the digest is the sum of the leaves")
            pairs.add(Value.VList(listOf(Value.VText(name), Value.VBytes(digest))))
        }
        ensureEq(
            Hex.encode(Sha256.hash(Canon.encode(Value.VList(pairs)))),
            want,
            "the hash of the pairs")
        ensureEq(Hex.encode(Hash.stateHash(st)), want, "state hash")
    }

    // module --------------------------------------------------------------

    /**
     * `check_module`: the bytes decode to the module value; it reads as a module of spec 4 that
     * writes back as itself and encodes to the bytes; its module hash; its schema well formed. A
     * `stripped` vector also: the client's module beside it, the server's with its private blocks
     * stripped and verified, at the public hash, and both naming every function by the listed hash.
     */
    fun module(v: Value) {
        val value = v.field("module")
        val bytes = bytesOf(v.field("bytes"))
        ensureEq(Canon.decode(bytes), value, "the bytes decode to the module value")
        val m = moduleOf(value)
        ensureEq(m.spec, SPEC_VERSION, "spec version")
        ensureEq(Encode.module(m), value, "decode then encode is the identity")
        ensureBytes(Canon.encode(Encode.module(m)), bytes, "the module encodes to the bytes")
        ensureEq(Hex.encode(Hash.moduleHash(m)), v.field("hash").asText(), "module hash")
        ensure(checkSchema(m.schema).isEmpty()) { "the schema is not well formed" }
        val sval = v.asStruct().fields["stripped"] ?: return
        ensureEq(
            Canon.decode(bytesOf(v.field("strippedBytes"))),
            sval,
            "the stripped bytes decode to the stripped module")
        val stripped = moduleOf(sval)
        // The one step a client may skip; this runtime takes it.
        val want = Verify.verify(stripModule(m))
        ensureEq(
            Encode.module(want),
            sval,
            "the stripped module is the module with its private blocks taken out")
        ensureEq(
            Hex.encode(Hash.moduleHash(stripped)),
            v.field("public").asText(),
            "the public module hash")
        fun names(m: Module): List<Pair<String, String>> =
            Hash.closures(m)
                .map { (h, c) -> c.function.name to h }
                .sortedWith(compareBy({ it.first }, { it.second }))
        val listed =
            v.field("functions").asList().map { p ->
                p.asList()[0].asText() to p.asList()[1].asText()
            }
        ensureEq(names(m), listed, "the server's module names its functions by the listed hashes")
        ensureEq(names(stripped), listed, "and so does the client's")
    }

    // verify --------------------------------------------------------------

    /**
     * `check_verify`: it verifies, as itself; its schema, its spec; every function has a closure.
     */
    fun verify(v: Value) {
        val m = moduleOf(v.field("module"))
        ensure(v.field("verifies") == Value.VBool(true)) {
            "only accepting modules are checked here"
        }
        ensure(checkSchema(m.schema).isEmpty()) { "the schema is not well formed" }
        ensureEq(m.spec, SPEC_VERSION, "spec version")
        val verified = Verify.verify(m)
        ensureEq(
            Encode.module(verified), Encode.module(m), "a verified module is its own verified form")
        ensureEq(Hash.closures(m).size, m.functions.size, "one closure per function")
    }

    // eval ------------------------------------------------------------------

    /** A vector's `ctx`: the user, the login, and the roles where it says any. */
    fun ctxOf(c: Value): Ctx {
        val roles = c.asStruct().fields["roles"]?.asList()?.map { it.asText() } ?: emptyList()
        return Ctx(c.field("user").asText(), c.field("session").asText()).withRoles(roles)
    }

    fun argsOf(v: Value): Map<String, Value> = v.asStruct().fields

    fun changesValue(cs: List<dev.arkdb.Change>): Value = Value.VList(cs.map(Facts::changeValue))

    /** The function hashes `rebase/three-peers.json`'s entries name, as hex. */
    private val named: Set<String> by lazy {
        Conformance.vector("rebase/three-peers.json")
            .field("entries")
            .asList()
            .map { Hex.encode(it.field("fn").asBytes()) }
            .toSet()
    }

    /**
     * `check_eval`: a mutator applied step by step — its changes, the store and its hash after each
     * — and again through its closure over a second copy, which must reach the same store.
     */
    fun evalApply(v: Value) {
        val m = moduleOf(v.field("module"))
        val name = v.field("function").asText()
        val f = m.lookupFunction(name) ?: throw Failed("no function $name")
        val fh = Hex.encode(Hash.functionHash(Hash.closure(m, f)))
        ensure(fh in named) { "the function hash is not the one the log names" }
        ensureEq(fh, v.field("function_hash").asText(), "function_hash")
        val bodies = Hash.closures(m)
        val ctx = ctxOf(v.field("ctx"))
        val autos = argsOf(v.field("autos"))
        val st = MemoryStore.fromValue(m.schema, v.field("store_before"))
        val byClosure = st.copy()
        for ((i, step) in v.field("steps").asList().withIndex()) {
            val args = argsOf(step.field("args"))
            val changes =
                when (val r = ran("step $i") { Eval.apply(m, name, ctx, autos, args, st) }) {
                    is Ran.Ok -> r.changes
                    is Ran.Refused -> throw Failed("step $i: refused ${r.refusal.show()}")
                }
            ensureEq(changesValue(changes), step.field("changes"), "step $i changes")
            ensureEq(st.storeValue(), step.field("store_after"), "step $i store")
            ensureEq(
                Hex.encode(Hash.stateHash(st)), step.field("hash_after").asText(), "step $i hash")
            val again =
                when (val r =
                    ran("step $i closure") {
                        Eval.applyClosure(m.schema, bodies[fh]!!, ctx, autos, args, byClosure)
                    }) {
                    is Ran.Ok -> r.changes
                    is Ran.Refused -> throw Failed("step $i: closure refused ${r.refusal.show()}")
                }
            ensureEq(again, changes, "step $i through the closure")
        }
        ensure(byClosure == st) { "the closure map reached another store" }
    }

    // A bug is a failure of the vector, named; a verdict is the caller's to judge.
    private fun ran(what: String, f: () -> Ran): Ran =
        try {
            f()
        } catch (b: EvalFault.Bug) {
            throw Failed("$what: bug ${b.error}")
        }

    /**
     * `eval_cases`: one store, each case over a fresh copy — a mutator that must apply (its changes
     * compared where given) or be refused by exactly a text; or the form validator's messages and
     * normalised values over a partial input.
     */
    fun evalCases(v: Value) {
        val m = moduleOf(v.field("module"))
        val bodies = Hash.closures(m)
        fun byName(n: String) =
            bodies.values.firstOrNull { it.function.name == n } ?: throw Failed("no function $n")
        val ctx0 = v.asStruct().fields["ctx"]?.let(::ctxOf) ?: Ctx("alice", "session-1")
        val fs = v.asStruct().fields
        val st = MemoryStore.fromValue(m.schema, fs["store_before"] ?: v.field("store"))
        for (case in v.field("cases").asList()) {
            val cf = case.asStruct().fields
            val name = case.field("name").asText()
            val c = byName(case.field("function").asText())
            val ctx = cf["ctx"]?.let(::ctxOf) ?: ctx0
            val refused = cf["refused"]
            if (cf.containsKey("refused")) {
                val got =
                    ran(name) {
                        Eval.applyClosure(
                            m.schema,
                            c,
                            ctx,
                            argsOf(case.field("autos")),
                            argsOf(case.field("args")),
                            st.copy())
                    }
                when {
                    refused is Value.VNull && got is Ran.Ok ->
                        cf["changes"]?.let {
                            ensureEq(changesValue(got.changes), it, "$name changes")
                        }
                    refused is Value.VText &&
                        got is Ran.Refused &&
                        got.refusal is Refusal.Refused ->
                        ensureEq((got.refusal as Refusal.Refused).why, refused.value, name)
                    else -> throw Failed("$name: wanted ${refused?.show()}, got $got")
                }
            } else {
                val got =
                    try {
                        Eval.check(m.schema, c, ctx, argsOf(case.field("input")), st)
                    } catch (b: EvalFault.Bug) {
                        throw Failed("$name: bug ${b.error}")
                    }
                val want =
                    case.field("messages").asList().map {
                        it.field("field").asText() to it.field("message").asText()
                    }
                ensureEq(got.messages, want, "$name messages")
                ensureEq(Value.VStruct(got.values), case.field("normalised"), "$name normalised")
            }
        }
    }

    /**
     * `check_raw`: a raw write named by the hash of its name, applied step by step — its changes,
     * or the refusal's text with the store unmoved — and the store and its hash after each.
     */
    fun evalRaw(v: Value) {
        val m = moduleOf(v.field("module"))
        val name = v.field("raw").asText()
        val fh = Raw.hashOf(name)
        ensureEq(Hex.encode(fh), v.field("function_hash").asText(), "function_hash")
        val raw = Raw.of(fh) ?: throw Failed("$name is no raw write")
        val st = MemoryStore.fromValue(m.schema, v.field("store_before"))
        for ((i, step) in v.field("steps").asList().withIndex()) {
            val args = argsOf(step.field("args"))
            val before = st.copy()
            val got = ran("step $i") { Raw.apply(raw, args, st) }
            val want = step.asStruct().fields["refused"]
            when {
                got is Ran.Ok && want == null ->
                    ensureEq(changesValue(got.changes), step.field("changes"), "step $i changes")
                got is Ran.Refused && want != null -> {
                    ensureEq(got.refusal.show(), want.asText(), "step $i refusal")
                    ensure(st == before) { "step $i: a refused write moved the store" }
                }
                else -> throw Failed("step $i: got $got, wanted ${want?.show()}")
            }
            ensureEq(st.storeValue(), step.field("store_after"), "step $i store")
            ensureEq(
                Hex.encode(Hash.stateHash(st)), step.field("hash_after").asText(), "step $i hash")
        }
    }
}
