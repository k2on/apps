// §8 Hashes (Ark.Hash): the state hash and the function hash, both SHA-256
// over canonical CBOR, so there is one encoder and one hash in the system.
package dev.arkdb

/** The 32-byte hash an entry names its function by. */
public class FnHash(bytes: ByteArray) : Comparable<FnHash> {
    public val bytes: ByteArray = bytes.copyOf()

    init {
        require(bytes.size == 32) { "a function hash is thirty-two bytes, got ${bytes.size}" }
    }

    public val hex: String get() = Hex.encode(bytes)

    override fun equals(other: Any?): Boolean = other is FnHash && bytes.contentEquals(other.bytes)
    override fun hashCode(): Int = bytes.contentHashCode()
    override fun compareTo(other: FnHash): Int = ByteOrder.compare(bytes, other.bytes)
    override fun toString(): String = hex

    public companion object {
        public fun ofHex(hex: String): FnHash = FnHash(Hex.decode(hex))
    }
}

/**
 * §8.3 A function with the helpers it was verified against: every helper it
 * reaches, in the version current when it was hashed, in declaration order.
 * A complete, self-contained program.
 */
public data class Closure(val fn: Function, val helpers: List<Function>)

public object Hash {
    /**
     * The closure of a function within a module: the function normalised,
     * with the middleware it runs and the helpers it and they reach, in
     * module order.
     */
    public fun closure(m: Module, fn: Function): Closure {
        val reach = LinkedHashSet<String>()
        val todo = ArrayDeque(Encode.deps(fn))
        while (todo.isNotEmpty()) {
            val n = todo.removeFirst()
            if (n in reach) continue
            val h = m.lookupFunction(n) ?: continue
            reach.add(n)
            todo.addAll(0, Encode.deps(h))
        }
        return Closure(Encode.normalize(fn), m.functions.filter { it.name in reach }.map { Encode.normalize(it) })
    }

    /** A closure as a value: `{ t: "closure", fn, helpers }`. */
    public fun closureValue(c: Closure): Value = Value.VStruct(
        mapOf(
            "t" to Value.VText("closure"),
            "fn" to Encode.functionValue(emptyMap(), c.fn),
            "helpers" to Value.VList(c.helpers.map { Encode.functionValue(emptyMap(), it) }),
        ),
    )

    /** Every function of a module, by the hash of its closure, in module order (middleware and helpers included). */
    public fun closures(m: Module): Map<FnHash, Closure> {
        val out = LinkedHashMap<FnHash, Closure>()
        for (fn in m.functions) {
            val c = closure(m, fn)
            out[functionHash(c)] = c
        }
        return out
    }

    /**
     * §8.1 The state hash: SHA-256 of the canonical encoding of a list with,
     * for every table of the schema in schema order, the table's name and its
     * rows in key order. An empty table contributes its name and an empty list.
     */
    public fun stateHash(store: Store): ByteArray {
        val tables = store.schema.tables.map { t ->
            Value.VList(listOf(Value.VText(t.name), Value.VList(store.scan(t.name))))
        }
        return Sha256.hash(Canon.encode(Value.VList(tables)))
    }

    /**
     * §8.2 The hash of a function: over its normalised canonical form, names
     * excluded, with the hashes of the helpers it calls directly, which cover
     * theirs in turn.
     */
    public fun functionHash(c: Closure): FnHash {
        val deps = LinkedHashMap<String, Value>()
        for (n in Encode.deps(c.fn)) {
            val h = c.helpers.firstOrNull { it.name == n } ?: continue
            deps[n] = Value.VBytes(functionHash(Closure(h, c.helpers)).bytes)
        }
        return FnHash(Sha256.hash(Canon.encode(Encode.functionValue(deps, c.fn))))
    }

    /** The hash of a whole module, normalised. */
    public fun moduleHash(m: Module): ByteArray = Sha256.hash(Canon.encode(Encode.toValue(Encode.normalizeModule(m))))
}
