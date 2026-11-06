// The onboarding as this phone carries it: the sign-in machine itself is
// `ark_auth::onboarding`'s, its every rule held by its Rust tests; here is
// what is Android's alone — the permissions, asked once in order and
// skipped where the platform lacks them — and the machine as the bridge
// hands it to harken.
package dev.harken.android.onboarding

import dev.arkdb.auth.AppInfo
import dev.arkdb.auth.Event
import dev.arkdb.auth.Onboarding
import dev.arkdb.auth.Remembered
import dev.arkdb.auth.Step
import dev.harken.android.testLogin
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class OnboardingTests {
    private val notify = Permission.Notifications("to show what's playing")
    private val contacts =
        object : Permission() {
            override val key = "contacts"
            override val android = "android.permission.READ_CONTACTS"
            override val since = 23
            override val title = "Contacts"
            override val why = "to know your friends"
        }

    @Test
    fun permissionsAreAskedOnceInOrder() {
        assertEquals(listOf(notify, contacts), pending(listOf(notify, contacts), 34, emptySet()))
        assertEquals(listOf(contacts), pending(listOf(notify, contacts), 34, setOf("notifications")))
        // Below the level a permission came in, it is granted and never asked.
        assertEquals(listOf(contacts), pending(listOf(notify, contacts), 30, emptySet()))
        assertEquals(emptyList<Permission>(), pending(listOf(notify, contacts), 34, setOf("notifications", "contacts")))
    }

    /** The bridge's machine, as harken is to it: a remembered login is the app at once. */
    @Test
    fun aRememberedLoginIsTheAppAtOnce() {
        val m = Onboarding(AppInfo("Harken", "harken", false, "your library"))
        val login = testLogin("ana", "s1", name = "Ana")
        m.next(Event.Launched(Remembered("https://music.example.com", login, false), null, false))
        assertEquals(Step.Running("https://music.example.com", login), m.step())
        m.next(Event.SignOut)
        assertTrue(m.step() is Step.SignIn)
    }
}
