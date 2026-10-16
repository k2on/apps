// The process owns the session (Sessions.kt): one per server and login,
// shared by every end that holds it, closed when the last lets go — or for
// everyone at once, on signing out or a denial, which forgets the login as
// the onboarding would.
package dev.arkdb.android

import dev.arkdb.client.Account
import dev.arkdb.client.Domain
import dev.arkdb.client.Login
import dev.arkdb.client.MemoryStorage
import dev.arkdb.client.Options
import dev.arkdb.client.Peer
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionsTests {
    private val domain: Domain by lazy {
        var dir: File? = File("").absoluteFile
        while (dir != null) {
            File(dir, "harken/domain/harken.ark")
                .takeIf { it.isFile }
                ?.let {
                    return@lazy Domain.fromBytes(it.readBytes())
                }
            dir = dir.parentFile
        }
        error("no harken/domain/harken.ark above ${File("").absolutePath}")
    }

    private val server = "https://music.example.com"
    private val bob = Login("t", "s1", Account("bob"), 0)

    /** Sessions over bob's peers, each counted, and what each was told to say. */
    private inner class Rig {
        var opened = 0
        val said = ArrayList<SessionEvents>()
        val forgot = ArrayList<Login>()
        val sessions =
            Sessions(
                open = { s, l, events ->
                    opened++
                    said.add(events)
                    ArkSession.inline(
                        Peer.open(
                            domain, MemoryStorage(), Options.server(l.user.id, l.session, l.token)),
                        s,
                        l)
                },
                forget = { forgot.add(it) })
    }

    /** What a holder was told. */
    private class Heard : SessionEvents {
        val got = ArrayList<String>()

        override fun denied(reason: String) {
            got.add("denied: $reason")
        }

        override fun closed() {
            got.add("closed")
        }
    }

    /**
     * The activity and the media service hold one session; either letting go leaves it open for the
     * other, and the last closes it.
     *
     * Falsified by a release that closed the session whoever else held it: "open while the service
     * holds it".
     */
    @Test
    fun oneSessionHeldByTwoClosesWithTheLast() {
        val r = Rig()
        val app = r.sessions.hold(server, bob, "app")
        val service = r.sessions.hold(server, bob, "playback")
        assertSame("one session", app.session, service.session)
        assertEquals("opened once", 1, r.opened)
        assertSame(app.session, r.sessions.current.value)
        assertEquals(listOf("app", "playback"), r.sessions.holders(app.session))

        app.release()
        assertFalse("open while the service holds it", service.session.isClosed)
        assertEquals(listOf("playback"), r.sessions.holders(service.session))
        app.release()
        assertFalse("a second release is nothing", service.session.isClosed)

        service.release()
        assertTrue("closed with the last hold", service.session.isClosed)
        assertNull(r.sessions.current.value)

        val again = r.sessions.hold(server, bob, "app")
        assertNotSame("a new one after", service.session, again.session)
        assertEquals(2, r.opened)
    }

    /**
     * Signing out closes it for everyone, and says so to each; a denial forgets the login, tells
     * every holder, and closes it too.
     */
    @Test
    fun aCloseOrADenialIsEveryones() {
        val r = Rig()
        val heardApp = Heard()
        val heardService = Heard()
        val app = r.sessions.hold(server, bob, "app", heardApp)
        r.sessions.hold(server, bob, "playback", heardService)
        r.sessions.close(app.session)
        assertTrue(app.session.isClosed)
        assertEquals(listOf("closed"), heardApp.got)
        assertEquals(listOf("closed"), heardService.got)
        assertNull(r.sessions.current.value)

        val a = Heard()
        val b = Heard()
        val held = r.sessions.hold(server, bob, "app", a)
        r.sessions.hold(server, bob, "playback", b)
        r.said.last().denied("revoked")
        assertEquals("the login forgotten", listOf(bob), r.forgot)
        assertEquals(listOf("denied: revoked", "closed"), a.got)
        assertEquals(listOf("denied: revoked", "closed"), b.got)
        assertTrue(held.session.isClosed)
    }

    /** What an app keeps with the session is made once, whoever asks, and closed with it. */
    @Test
    fun whatIsKeptIsOneAndClosesWithTheSession() {
        val r = Rig()
        val h = r.sessions.hold(server, bob, "app")
        var made = 0
        var closed = 0
        val one =
            h.session.kept("library") {
                made++
                AutoCloseable { closed++ }
            }
        val two =
            h.session.kept("library") {
                made++
                AutoCloseable { closed++ }
            }
        assertSame(one, two)
        assertEquals(1, made)
        h.release()
        assertEquals("closed with the session", 1, closed)
    }
}
