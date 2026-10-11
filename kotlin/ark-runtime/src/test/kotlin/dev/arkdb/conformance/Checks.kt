// The check of each vector kind, as spec/RUNNERS.md says it and
// rust/ark/tests/vectors.rs reads it: each throws `Failed` with its first
// unmet claim, so that one definition runs both ways — a vector must pass
// it and a `falsify/` vector must fail it.
package dev.arkdb.conformance

import dev.arkdb.Canon
import dev.arkdb.Decode
import dev.arkdb.Hash
import dev.arkdb.Hex
import dev.arkdb.MemoryStore
import dev.arkdb.Module
import dev.arkdb.Sha256
import dev.arkdb.Value
import dev.arkdb.ValueOrder
import dev.arkdb.leaf

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
}
