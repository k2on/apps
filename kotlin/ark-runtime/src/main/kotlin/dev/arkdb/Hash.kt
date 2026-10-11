// §8 Hashes, as `rust/ark/src/hash.rs` defines them: SHA-256 over canonical
// CBOR — the state hash (§8.1), which moves with the store, and the
// function hash (§8.2).
//
// §8.1, exactly, with `enc` the canonical encoding and `‖` concatenation:
//
//     leaf(t, row)  = sha256(enc(Text t) ‖ enc(Struct row))
//     digest(t)     = Σ leaf(t, row) over the rows of t, mod 2^256
//     state_hash(s) = sha256(enc(List [List [Text t, Bytes digest(t)]
//                                      for every table t, in schema order]))
//
// A sum does not depend on order and moves by one addition and one
// subtraction per row written, so a store keeps its digests beside its
// rows (`MemoryStore`) and a `Verify` costs one SHA-256 over the tables.
package dev.arkdb

/** The 32-byte hash an entry names its function by (`hash::FnHash`). */
public typealias FnHash = ByteArray

/**
 * §8.3 A function with the helpers it was verified against: every helper and middleware it reaches,
 * normalised, in declaration order — a complete program run without consulting any module.
 */
public data class Closure(val function: Function, val helpers: List<Function>)

/** §8.1 A row's leaf: `sha256(enc(Text table) ‖ enc(Struct row))` (`hash::leaf`). */
public fun leaf(table: String, row: Row): ByteArray {
    val md = java.security.MessageDigest.getInstance("SHA-256")
    md.update(Canon.encode(Value.VText(table)))
    md.update(Canon.encode(row))
    return md.digest()
}

/**
 * §8.1 A table's digest: the sum of its rows' leaves modulo 2^256, 32 big-endian bytes, written out
 * here byte by byte rather than through a big-integer library (`hash::Digest`).
 */
public class Digest(public val bytes: ByteArray = ByteArray(32)) {
    /** This digest with a leaf added, mod 2^256. */
    public fun add(leaf: ByteArray) {
        var carry = 0
        for (i in 31 downTo 0) {
            val s = (bytes[i].toInt() and 0xff) + (leaf[i].toInt() and 0xff) + carry
            bytes[i] = s.toByte()
            carry = s ushr 8
        }
    }

    /** This digest with a leaf taken away, mod 2^256: the inverse of `add`. */
    public fun sub(leaf: ByteArray) {
        var borrow = 0
        for (i in 31 downTo 0) {
            val d = (bytes[i].toInt() and 0xff) - (leaf[i].toInt() and 0xff) - borrow
            bytes[i] = d.toByte()
            borrow = if (d < 0) 1 else 0
        }
    }

    public fun copy(): Digest = Digest(bytes.copyOf())

    override fun equals(other: Any?): Boolean = other is Digest && bytes.contentEquals(other.bytes)

    override fun hashCode(): Int = bytes.contentHashCode()

    override fun toString(): String = "Digest(${Hex.encode(bytes)})"
}

public object Hash {
    /** `table_digest`: the digest of some rows, summed one by one — the definition. */
    public fun tableDigest(table: String, rows: Iterable<Row>): Digest {
        val d = Digest()
        for (r in rows) d.add(leaf(table, r))
        return d
    }

    /** `state_hash_of`: the SHA-256 of the list of `[name, digest]` pairs, in schema order. */
    public fun stateHashOf(digests: List<Pair<String, Digest>>): ByteArray =
        Sha256.hash(
            Canon.encode(
                Value.VList(
                    digests.map { (t, d) ->
                        Value.VList(listOf(Value.VText(t), Value.VBytes(d.bytes)))
                    })))

    /**
     * §8.1 `state_hash`: every table of the store's schema, in order, with the digest the store
     * keeps for it — or, for a store that keeps none, the digest of a scan.
     */
    public fun stateHash(st: Store): ByteArray =
        stateHashOf(
            st.schema.tables.map { t ->
                t.name to (st.digest(t.name) ?: tableDigest(t.name, st.scan(t.name)))
            })
}
