// Every transition of the onboarding, with the effects each asks for: where
// a launch starts from what was remembered, the server step and its
// failures, signing in and its failures, the permissions (asked once,
// skipped where the platform lacks them), and the two ways back out of the
// app — the server's denial and signing out — each of which keeps the
// server and closes the session without touching its storage.
package dev.arkdb.android

import dev.arkdb.client.Account
import dev.arkdb.client.Login
import org.junit.Assert.assertEquals
import org.junit.Test

class OnboardingTests {
    private val login = Login("t", "s1", Account("ana", "Ana"), 0)
    private val server = "https://music.example.com"
    private val notify = Permission.Notifications("to show what's playing")
    private val machine = Onboarding(listOf(notify), sdk = 34, scheme = "harken")
    private val old = Onboarding(listOf(notify), sdk = 30, scheme = "harken")

    @Test
    fun startsFromWhatWasRemembered() {
        assertEquals(Move(Step.Server()), machine.start(null, null, emptySet()))
        assertEquals(Move(Step.Server()), machine.start(null, login, emptySet()))
        assertEquals(Move(Step.SignIn(server)), machine.start(server, null, emptySet()))
        assertEquals(
            Move(Step.Ask(server, login, notify)), machine.start(server, login, emptySet()))
        assertEquals(
            Move(Step.Running(server, login), listOf(Effect.OpenSession(server, login))),
            machine.start(server, login, setOf("notifications")))
        // Below Android 13 there is no notification permission to ask for.
        assertEquals(
            Move(Step.Running(server, login), listOf(Effect.OpenSession(server, login))),
            old.start(server, login, emptySet()))
    }

    @Test
    fun theServerStep() {
        val empty = Step.Server()
        assertEquals(
            Move(Step.Server("music.example.com")),
            machine.next(empty, Event.Typed("music.example.com"), emptySet()))
        // Nothing typed: the address's own sentence, and nothing asked of the network.
        assertEquals(
            Move(Step.Server(error = "Type the address of your server.")),
            machine.next(empty, Event.Check, emptySet()))
        val typed = Step.Server("music.example.com")
        val checking = Step.Server("music.example.com", checking = true)
        assertEquals(
            Move(checking, listOf(Effect.FindServer("music.example.com"))),
            machine.next(typed, Event.Check, emptySet()))
        // Pressed twice: one probe.
        assertEquals(Move(checking), machine.next(checking, Event.Check, emptySet()))
        assertEquals(
            Move(Step.SignIn(server), listOf(Effect.RememberServer(server))),
            machine.next(checking, Event.Checked(Found.Server(server, newer = false)), emptySet()))
        assertEquals(
            Move(Step.Server("music.example.com", error = "Couldn't reach it.")),
            machine.next(checking, Event.Checked(Found.Failed("Couldn't reach it.")), emptySet()))
        // An answer nobody is waiting for is dropped.
        assertEquals(
            Move(typed),
            machine.next(typed, Event.Checked(Found.Server(server, false)), emptySet()))
        // Typing again clears the error.
        val failed = Step.Server("x", error = "no")
        assertEquals(Move(Step.Server("xy")), machine.next(failed, Event.Typed("xy"), emptySet()))
        assertEquals(Move(failed), machine.next(failed, Event.SignOut, emptySet()))
    }

    @Test
    fun signingIn() {
        val idle = Step.SignIn(server)
        val browsing = Step.SignIn(server, Step.Phase.Browser)
        val exchanging = Step.SignIn(server, Step.Phase.Exchanging)
        assertEquals(
            Move(
                browsing, listOf(Effect.Browse("$server/auth/login?redirect=harken%3A%2F%2Fauth"))),
            machine.next(idle, Event.OpenSignIn, emptySet()))
        // The tab closed with no code.
        assertEquals(Move(idle), machine.next(browsing, Event.Returned, emptySet()))
        assertEquals(Move(idle), machine.next(idle, Event.Returned, emptySet()))
        assertEquals(
            Move(exchanging, listOf(Effect.Exchange(server, "c0de"))),
            machine.next(browsing, Event.Code("c0de"), emptySet()))
        // A code with no tab open — the app was killed behind it — is exchanged all the same.
        assertEquals(
            Move(exchanging, listOf(Effect.Exchange(server, "c0de"))),
            machine.next(idle, Event.Code("c0de"), emptySet()))
        // While exchanging, nothing else starts.
        assertEquals(Move(exchanging), machine.next(exchanging, Event.Code("again"), emptySet()))
        assertEquals(Move(exchanging), machine.next(exchanging, Event.Returned, emptySet()))
        assertEquals(Move(exchanging), machine.next(exchanging, Event.OpenSignIn, emptySet()))
        assertEquals(Move(exchanging), machine.next(exchanging, Event.ChangeServer, emptySet()))
        assertEquals(
            Move(Step.Ask(server, login, notify), listOf(Effect.RememberLogin(login))),
            machine.next(exchanging, Event.SignedIn(login), emptySet()))
        assertEquals(
            Move(
                Step.Running(server, login),
                listOf(Effect.RememberLogin(login), Effect.OpenSession(server, login))),
            machine.next(exchanging, Event.SignedIn(login), setOf("notifications")))
        assertEquals(
            Move(Step.SignIn(server, error = "that code has been used")),
            machine.next(exchanging, Event.SignInFailed("that code has been used"), emptySet()))
        // A login nobody is exchanging for is dropped.
        assertEquals(Move(idle), machine.next(idle, Event.SignedIn(login), emptySet()))
        assertEquals(Move(idle), machine.next(idle, Event.SignInFailed("x"), emptySet()))
        assertEquals(
            Move(Step.Server("music.example.com")),
            machine.next(idle, Event.ChangeServer, emptySet()))
        assertEquals(
            Move(Step.Server("music.example.com")),
            machine.next(browsing, Event.ChangeServer, emptySet()))
    }

    @Test
    fun permissionsAreAskedOnce() {
        val ask = Step.Ask(server, login, notify)
        assertEquals(
            Move(ask, listOf(Effect.Request(notify))), machine.next(ask, Event.Allow, emptySet()))
        // Allowed or not, answered is answered: marked, and on to the app.
        assertEquals(
            Move(
                Step.Running(server, login),
                listOf(Effect.MarkAsked("notifications"), Effect.OpenSession(server, login))),
            machine.next(ask, Event.Answered, emptySet()))
        assertEquals(Move(ask), machine.next(ask, Event.SignOut, emptySet()))
        // Two permissions: one screen each, in order.
        val two =
            Onboarding(
                listOf(
                    notify,
                    object : Permission() {
                        override val key = "contacts"
                        override val android = "android.permission.READ_CONTACTS"
                        override val since = 23
                        override val title = "Contacts"
                        override val why = "to know your friends"
                    }),
                sdk = 34,
                scheme = "harken")
        val first =
            two.next(Step.SignIn(server, Step.Phase.Exchanging), Event.SignedIn(login), emptySet())
        assertEquals(notify, (first.step as Step.Ask).permission)
        val second = two.next(first.step, Event.Answered, emptySet())
        assertEquals("contacts", (second.step as Step.Ask).permission.key)
        assertEquals(listOf(Effect.MarkAsked("notifications")), second.effects)
    }

    @Test
    fun deniedWhileRunningGoesBackToSignInKeepingTheServer() {
        val running = Step.Running(server, login)
        val m = machine.next(running, Event.Denied("revoked"), emptySet())
        assertEquals(server, (m.step as Step.SignIn).server)
        assertEquals(Step.Phase.Idle, (m.step as Step.SignIn).phase)
        assertEquals(true, (m.step as Step.SignIn).error!!.contains("revoked"))
        // The session closes (written down, its storage kept) and the dead login is forgotten;
        // the server is not.
        assertEquals(listOf(Effect.CloseSession, Effect.ForgetLogin), m.effects)
    }

    @Test
    fun signingOutKeepsTheServer() {
        val running = Step.Running(server, login)
        assertEquals(
            Move(Step.SignIn(server), listOf(Effect.CloseSession, Effect.ForgetLogin)),
            machine.next(running, Event.SignOut, emptySet()))
        // Nothing else moves a running app.
        for (e in
            listOf(
                Event.Check,
                Event.OpenSignIn,
                Event.Returned,
                Event.Code("c"),
                Event.Answered,
                Event.Allow,
                Event.ChangeServer)) assertEquals(
            Move(running), machine.next(running, e, emptySet()))
    }

    @Test
    fun aWholeFirstRunAndASecondLaunch() {
        val shelf = MemoryShelf()
        val r = Remembered(shelf)
        var step = machine.start(r.server, r.login, r.asked).step
        fun go(e: Event) {
            val m = machine.next(step, e, r.asked)
            for (x in m.effects) when (x) {
                is Effect.RememberServer -> r.server = x.url
                is Effect.RememberLogin -> r.login = x.login
                is Effect.ForgetLogin -> r.login = null
                is Effect.MarkAsked -> r.markAsked(x.key)
                else -> {}
            }
            step = m.step
        }
        go(Event.Typed("music.example.com"))
        go(Event.Check)
        go(Event.Checked(Found.Server(server, false)))
        go(Event.OpenSignIn)
        go(Event.Code("c"))
        go(Event.SignedIn(login))
        go(Event.Answered)
        assertEquals(Step.Running(server, login), step)
        // The next launch goes straight in.
        assertEquals(Step.Running(server, login), machine.start(r.server, r.login, r.asked).step)
        go(Event.SignOut)
        assertEquals(Step.SignIn(server), machine.start(r.server, r.login, r.asked).step)
    }
}
