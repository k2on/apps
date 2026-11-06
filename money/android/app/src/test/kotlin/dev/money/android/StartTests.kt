// The way to a budget as this phone carries it: the sign-in machine itself
// is `ark_auth::onboarding`'s, its every rule held by its Rust tests; here
// is what is money's — what is remembered on the shelf under the keys it
// always had, the machine as the bridge hands it over, and the dates.
package dev.money.android

import dev.arkdb.auth.Event
import dev.arkdb.auth.Login
import dev.arkdb.auth.Onboarding
import dev.arkdb.auth.Remembered
import dev.arkdb.auth.Step
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class StartTests {
    private val shelf = MemoryShelf()

    private fun login(user: String = "sam") = Login("t", "s1", user, "Sam", "sam@example.com", emptyList(), 0, "")

    @Test fun whatIsRememberedIsKeptWhole() {
        val r = Remembered("https://money.koon.us", login(), demo = true)
        Kept(shelf).remembered = r
        assertEquals(r, Kept(shelf).remembered)
        assertEquals("1", shelf.get("money.demo"))
        shelf.put("money.login", "{not json")
        assertNull(Kept(shelf).remembered.login)
    }

    /** The bridge's machine, as money is to it: a server with nobody signed in goes to sign in by itself. */
    @Test fun aServerAloneGoesToSignIn() {
        val m = Onboarding(MONEY)
        val fx = m.next(Event.Launched(Remembered("https://money.koon.us", null, false), null, false))
        assertTrue(m.step() is Step.SignIn)
        assertTrue(fx.toString(), fx.single().toString().contains("money%3A%2F%2Fauth"))
        m.next(Event.TryDemo)
        assertTrue(m.step() is Step.SignIn)
    }

    @Test fun theDatePickersDayIsTheDay() {
        assertEquals("2026-10-26", Dates.ofMillis(Dates.millis("2026-10-26")!!))
    }
}
