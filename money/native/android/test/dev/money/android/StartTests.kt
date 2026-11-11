// The way to a budget as this phone carries it: the sign-in machine itself
// is `ark_auth::onboarding`'s, its every rule held by its Rust tests; here
// is what is money's — what is remembered on the shelf under the keys it
// always had and the login in the secure store, the machine as the bridge hands it over, and the dates.
package dev.money.android

import dev.arkdb.auth.Event
import dev.arkdb.auth.Login
import dev.arkdb.auth.Remembered
import dev.arkdb.auth.Step
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.ark_secure_store.SecureStore
import uniffi.money_bridge.FirstRunApp

/** The secure store as a map, for a test. */
class MemoryStore : SecureStore {
    val map: MutableMap<String, String> = LinkedHashMap()

    override fun get(key: String): String? = map[key]

    override fun put(key: String, value: String) {
        map[key] = value
    }

    override fun remove(key: String) {
        map.remove(key)
    }
}

class StartTests {
    private val shelf = MemoryShelf()
    private val store = MemoryStore()

    private fun login(user: String = "sam") = Login("t", "s1", user, "Sam", "sam@example.com", emptyList(), 0, "")

    @Test fun whatIsRememberedIsKeptWhole() {
        val r = Remembered("https://money.example.com", login(), demo = true)
        Kept(shelf, store).remembered = r
        assertEquals(r, Kept(shelf, store).remembered)
        assertEquals("1", shelf.get("money.demo"))
        // The login in the secure store alone, never on the shelf.
        assertNull(shelf.get("money.login"))
        assertEquals(setOf("login"), store.map.keys)
        store.map["login"] = "{not json"
        assertNull(Kept(shelf, store).remembered.login)
        // Signed out: gone from the store.
        Kept(shelf, store).remembered = r.copy(login = null)
        assertTrue(store.map.isEmpty())
    }

    /** A login an older build kept on the shelf is moved to the secure store once, and deleted there. */
    @Test fun anOldLoginIsMovedOnce() {
        Kept(shelf, store).remembered = Remembered("https://money.example.com", null, false)
        shelf.put("money.login", dev.arkdb.auth.loginJson(login()))
        assertEquals(login(), Kept(shelf, store).remembered.login)
        assertNull(shelf.get("money.login"))
        assertEquals("https://money.example.com", shelf.get("money.server"))
        assertEquals(login(), Kept(shelf, store).remembered.login)
    }

    /** No secure store: a login lasts this run, and is never written on the shelf. */
    @Test fun withNoStoreALoginIsNeverOnTheShelf() {
        val k = Kept(shelf, null)
        k.remembered = Remembered("https://money.example.com", login(), false)
        assertEquals(login(), k.remembered.login)
        assertNull(shelf.get("money.login"))
        assertNull(Kept(shelf, null).remembered.login)
    }

    /** The bridge's machine, as money is to it: a server with nobody signed in goes to sign in by itself. */
    @Test fun aServerAloneGoesToSignIn() {
        val m = FirstRunApp(MONEY, false, true)
        val fx = m.event(Event.Launched(Remembered("https://money.example.com", null, false), null, false))
        assertTrue(m.step() is Step.SignIn)
        assertTrue(fx.toString(), fx.single().toString().contains("money%3A%2F%2Fauth"))
        m.event(Event.TryDemo)
        assertTrue(m.step() is Step.SignIn)
    }

    @Test fun theDatePickersDayIsTheDay() {
        assertEquals("2026-10-26", Dates.ofMillis(Dates.millis("2026-10-26")!!))
    }
}
