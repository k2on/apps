// The app's module, verified once per content. The verifier's answer is a
// function of the module's bytes, so a launch with the bytes that passed
// last time does not ask it again: it checks the bytes are those bytes —
// their SHA-256 against the one remembered — and decodes them. Bytes that
// differ (an app updated with a new `.ark`) are verified as the first ones
// were, and remembered only once they pass.
package dev.arkdb.android

import dev.arkdb.Hex
import dev.arkdb.Sha256
import dev.arkdb.client.Domain

public class ModuleMemory(private val shelf: Shelf) {
    /** The module in `bytes`: verified, unless these exact bytes verified before. */
    public fun domain(bytes: ByteArray): Domain =
        open(bytes, Domain::fromBytes, Domain::fromVerifiedBytes)

    /** What [domain] does, with the verifier and the decoder as arguments — for the tests. */
    internal fun <T> open(bytes: ByteArray, verify: (ByteArray) -> T, decode: (ByteArray) -> T): T {
        val digest = Hex.encode(Sha256.hash(bytes))
        if (shelf.get(KEY) == digest) return decode(bytes)
        val d = verify(bytes)
        shelf.put(KEY, digest)
        return d
    }

    private companion object {
        /** The SHA-256 of the module bytes that last passed the verifier, as hex. */
        const val KEY = "ark.module.verified"
    }
}
