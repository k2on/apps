// The module is verified once per content: a launch with the bytes that
// passed before decodes them without asking the verifier, and bytes that
// differ — or that never passed — are verified.
package dev.arkdb.android

import dev.arkdb.Hex
import dev.arkdb.client.Domain
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class ModuleTests {
    private class Counted {
        var verified = 0
        var decoded = 0

        fun open(m: ModuleMemory, bytes: ByteArray): String =
            m.open(
                bytes,
                verify = {
                    verified++
                    "verified " + String(it)
                },
                decode = {
                    decoded++
                    "decoded " + String(it)
                })
    }

    @Test
    fun aRememberedModuleIsNotVerifiedAgainAndAChangedOneIs() {
        val shelf = MemoryShelf()
        val c = Counted()
        assertEquals("verified one", c.open(ModuleMemory(shelf), "one".toByteArray()))
        // A later launch, with the same bytes: decoded, not verified.
        assertEquals("decoded one", c.open(ModuleMemory(shelf), "one".toByteArray()))
        assertEquals("decoded one", c.open(ModuleMemory(shelf), "one".toByteArray()))
        assertEquals(1, c.verified)
        assertEquals(2, c.decoded)
        // An app updated with another module: verified, and then remembered instead.
        assertEquals("verified two", c.open(ModuleMemory(shelf), "two".toByteArray()))
        assertEquals("decoded two", c.open(ModuleMemory(shelf), "two".toByteArray()))
        // And the first again is the first again: verified.
        assertEquals("verified one", c.open(ModuleMemory(shelf), "one".toByteArray()))
        assertEquals(3, c.verified)
        assertEquals(3, c.decoded)
    }

    @Test
    fun aModuleThatFailsIsNotRemembered() {
        val shelf = MemoryShelf()
        val m = ModuleMemory(shelf)
        repeat(2) {
            try {
                m.open<String>(
                    "bad".toByteArray(),
                    verify = { throw IllegalStateException("no") },
                    decode = { "decoded" })
                fail("a module that does not verify was taken")
            } catch (e: IllegalStateException) {
                assertEquals("no", e.message)
            }
        }
        assertTrue(shelf.map.isEmpty())
    }

    @Test
    fun harkensModuleDecodedIsTheModuleVerified() {
        var dir: File? = File("").absoluteFile
        var ark: File? = null
        while (dir != null && ark == null) {
            ark = File(dir, "harken/domain/harken.ark").takeIf { it.isFile }
            dir = dir.parentFile
        }
        val bytes = ark!!.readBytes()
        val verified = Domain.fromBytes(bytes)
        val shelf = MemoryShelf()
        val first = ModuleMemory(shelf).domain(bytes)
        val second = ModuleMemory(shelf).domain(bytes)
        for (d in listOf(first, second, Domain.fromVerifiedBytes(bytes))) {
            assertEquals(Hex.encode(verified.hash), Hex.encode(d.hash))
            assertEquals(verified.closures.keys, d.closures.keys)
            assertEquals(verified.module, d.module)
        }
    }
}
