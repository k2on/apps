// The check of each vector kind, as spec/RUNNERS.md says it and
// rust/ark/tests/vectors.rs reads it: each throws `Failed` with its first
// unmet claim, so that one definition runs both ways — a vector must pass
// it and a `falsify/` vector must fail it.
package dev.arkdb.conformance

import dev.arkdb.Canon
import dev.arkdb.Decode
import dev.arkdb.Encode
import dev.arkdb.Hash
import dev.arkdb.Hex
import dev.arkdb.MemoryStore
import dev.arkdb.Module
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
}
