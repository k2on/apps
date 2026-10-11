// The non-determinism a mutator is allowed, drawn once at the originating
// peer and frozen in the entry, as `rust/ark-client/src/autos.rs` draws it:
// from the platform, or from a seed — a test's or a fixture's — by the same
// generator the reference uses, so a seeded Kotlin peer draws exactly the
// ids and times a seeded Rust peer draws.
package dev.arkdb.client

import dev.arkdb.Auto
import dev.arkdb.Function
import dev.arkdb.Id
import dev.arkdb.Value
import java.security.SecureRandom

/** Where a peer's autos come from (`autos::Autos`). */
public sealed class Autos {
    /** The platform's randomness and clock. */
    public class System : Autos() {
        private val random = SecureRandom()

        override fun fill(b: ByteArray) {
            random.nextBytes(b)
        }

        override fun nowMs(): Long = java.lang.System.currentTimeMillis()
    }

    /**
     * `Autos::seeded`: a PCG-style multiplier over a 64-bit state, the clock starting at
     * 1,700,000,000,000 and moving one millisecond per draw.
     */
    public class Seeded(seed: Long) : Autos() {
        private var state: Long = seed xor -0x61c8864680b583ebL
        private var now: Long = 1_700_000_000_000L

        override fun fill(b: ByteArray) {
            var at = 0
            while (at < b.size) {
                state = state * 6364136223846793005L + 1442695040888963407L
                val x = state xor (state ushr 33)
                for (i in 0 until minOf(8, b.size - at)) b[at + i] = (x ushr (8 * i)).toByte()
                at += 8
            }
        }

        override fun nowMs(): Long {
            now += 1
            return now
        }
    }

    protected abstract fun fill(b: ByteArray)

    /** The time, in milliseconds. */
    public abstract fun nowMs(): Long

    /** `new_id`: a fresh id, laid out as a version 4 UUID. */
    public fun newId(): Id {
        val b = ByteArray(16)
        fill(b)
        b[6] = ((b[6].toInt() and 0x0f) or 0x40).toByte()
        b[8] = ((b[8].toInt() and 0x3f) or 0x80).toByte()
        return Id(b)
    }

    /** `draw`: the autos a mutator declares, in its order. */
    public fun draw(f: Function): Map<String, Value> =
        f.autos.associate { (name, auto) ->
            name to
                when (auto) {
                    is Auto.NewId -> Value.VId(newId())
                    Auto.Now -> Value.VInt(nowMs())
                }
        }
}
