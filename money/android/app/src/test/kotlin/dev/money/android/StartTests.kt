// The way to a budget, walked without a network or a phone: the start's
// machine over a shelf in memory and effects that answer as a server would
// (a found server, a code exchanged) or would not. Its coroutines run where
// they are launched, so each step has happened when its call returns.
package dev.money.android

import dev.arkdb.ffi.Login
import dev.arkui.SignInPhase
import dev.arkui.server.Found
import java.util.Locale
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class StartTests {
    private val shelf = MemoryShelf()
    private val browsed = ArrayList<String>()
    private val opened = ArrayList<String>()

    private fun login(user: String = "sam") = Login("t", "s1", user, "Sam", "sam@example.com", emptyList(), 0, "")

    private fun starter(
        find: (String) -> Found = { Found.Server("https://$it", newer = false) },
        exchange: (String, String) -> Login = { _, _ -> login() },
    ): Starter =
        Starter(
            shelf,
            Effects(
                find = find,
                exchange = exchange,
                openServer = { server, _ ->
                    opened.add(server)
                    // A server's book stands in as the demo's: what matters here is that one opened.
                    Book.inline(dev.money.ffi.moneyDemo(TODAY), TODAY, demo = false, server = server)
                },
                openDemo = { demoBook() },
                browse = { browsed.add(it) },
            ),
            CoroutineScope(Dispatchers.Unconfined),
            io = Dispatchers.Unconfined,
        ).also { it.begin() }

    @Test fun aFirstLaunchWelcomesWithTheHouseholdsServer() {
        val s = starter()
        assertEquals(Step.Welcome(typed = DEFAULT_SERVER), s.step.value)
    }

    @Test fun theDemoIsRememberedUntilItIsLeft() {
        val s = starter()
        s.demo()
        assertTrue((s.step.value as Step.Running).book.demo)
        // Launched again: straight into the demo.
        val again = starter()
        assertTrue(again.step.value is Step.Running)
        again.signOut()
        assertEquals(Step.Welcome(), again.step.value)
        assertTrue(starter().step.value is Step.Welcome)
    }

    @Test fun anAddressThatIsNotAMoneyServerIsSaidUnderTheField() {
        val s = starter(find = { Found.Failed("$it answered, but it isn't an Ark server: it has no health page.") })
        s.type("example.com")
        s.check()
        val w = s.step.value as Step.Welcome
        assertFalse(w.checking)
        assertEquals("example.com answered, but it isn't an Ark server: it has no health page.", w.error)
        assertNull(Remembered(shelf).server)
    }

    @Test fun aServerIsSignedInToInABrowserTab() {
        val s = starter()
        s.check()
        assertEquals(Step.SignIn("https://money.koon.us"), s.step.value)
        assertEquals("https://money.koon.us", Remembered(shelf).server)
        s.openSignIn()
        assertEquals(SignInPhase.Browser, (s.step.value as Step.SignIn).phase)
        assertTrue(browsed.single(), browsed.single().startsWith("https://money.koon.us/auth/login?redirect="))
        assertTrue(browsed.single().contains("money"))
        // Back without a code: the person closed the tab.
        s.returned()
        assertEquals(SignInPhase.Idle, (s.step.value as Step.SignIn).phase)
        s.openSignIn()
        s.code("the-code")
        val r = s.step.value as Step.Running
        assertEquals("https://money.koon.us", r.book.server)
        assertEquals(listOf("https://money.koon.us"), opened)
        assertEquals("sam", Remembered(shelf).login?.user)
        // Launched again: the login is kept, so it opens the budget at once.
        opened.clear()
        assertTrue(starter().step.value is Step.Running)
        assertEquals(listOf("https://money.koon.us"), opened)
    }

    @Test fun aCodeThatDoesNotExchangeSaysSoAndKeepsTheServer() {
        val s = starter(exchange = { _, _ -> throw IllegalStateException("the code was used") })
        s.check()
        s.openSignIn()
        s.code("stale")
        val i = s.step.value as Step.SignIn
        assertEquals(SignInPhase.Idle, i.phase)
        assertEquals("Signing in didn't finish: the code was used. Try again.", i.error)
    }

    @Test fun signingOutKeepsTheServerAndForgetsTheLogin() {
        val s = starter()
        s.check()
        s.code("c")
        s.signOut("The server signed this phone out.")
        assertEquals(Step.SignIn("https://money.koon.us", error = "The server signed this phone out."), s.step.value)
        assertNull(Remembered(shelf).login)
        s.changeServer()
        assertEquals(Step.Welcome(typed = "money.koon.us"), s.step.value)
    }

    @Test fun aLoginIsKeptWhole() {
        val l = login()
        assertEquals(l, Logins.parse(Logins.json(l)))
        shelf.put("money.login", "{not json")
        assertNull(Remembered(shelf).login)
    }

    @Test fun theDayAndTheMonthAreWrittenInTheLocale() {
        assertEquals("October 2026", Dates.month("2026-10", Locale.US))
        assertEquals("Today", Dates.heading("2026-10-26", "2026-10-26", Locale.US))
        assertEquals("Yesterday", Dates.heading("2026-10-25", "2026-10-26", Locale.US))
        assertEquals("Mon, Oct 19", Dates.heading("2026-10-19", "2026-10-26", Locale.US))
        assertEquals("2026-10-26", Dates.ofMillis(Dates.millis("2026-10-26")!!))
    }
}
