// The server step's three questions without a network: what an address
// could mean (`Address`), whether an answer is an Ark server serving this
// app (`Health`), and the order and sentences of asking (`Probe`, over a
// request that is a lambda) — and the remembered store under it all.
package dev.harken.android.onboarding

import dev.arkui.server.Address
import dev.arkui.server.Answer
import dev.arkui.server.Found
import dev.arkui.server.Health
import dev.arkui.server.Probe

import dev.harken.android.testLogin
import java.io.IOException
import java.net.ConnectException
import java.net.UnknownHostException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ServerTests {
    private fun urls(typed: String): List<String> =
        (Address.parse(typed) as Address.Candidates).urls

    private fun why(typed: String): String = (Address.parse(typed) as Address.Invalid).why

    @Test
    fun addresses() {
        assertEquals(listOf("https://music.example.com"), urls("music.example.com"))
        assertEquals(listOf("https://music.example.com"), urls("  Music.Example.com/  "))
        assertEquals(listOf("https://music.example.com:8443"), urls("music.example.com:8443"))
        // A scheme is taken at its word, in the clear too.
        assertEquals(listOf("http://music.example.com"), urls("http://music.example.com"))
        assertEquals(
            listOf("https://music.example.com/harken"), urls("https://music.example.com/harken/"))
        // Private and loopback: https first, then http.
        assertEquals(
            listOf("https://192.168.1.20:8787", "http://192.168.1.20:8787"),
            urls("192.168.1.20:8787"))
        assertEquals(listOf("https://10.0.2.2:8787", "http://10.0.2.2:8787"), urls("10.0.2.2:8787"))
        assertEquals(
            listOf("https://localhost:8787", "http://localhost:8787"), urls("localhost:8787"))
        assertEquals(listOf("https://nas", "http://nas"), urls("nas"))
        assertEquals(listOf("https://music.local", "http://music.local"), urls("music.local"))
        assertEquals(listOf("https://[::1]:8787", "http://[::1]:8787"), urls("[::1]:8787"))
        assertEquals(
            listOf("https://100.101.102.103", "http://100.101.102.103"), urls("100.101.102.103"))
        // A public address is never tried in the clear.
        assertEquals(listOf("https://8.8.8.8"), urls("8.8.8.8"))
        assertEquals(listOf("https://172.32.0.1"), urls("172.32.0.1"))
        assertEquals("Type the address of your server.", why("   "))
        assertEquals("An address has no spaces in it.", why("music example"))
        assertEquals(
            "A server is reached over http or https, not ftp.", why("ftp://music.example.com"))
        assertEquals("The port has to be a number from 1 to 65535.", why("music.example.com:99999"))
        assertEquals("The port has to be a number from 1 to 65535.", why("music.example.com:x"))
        assertTrue(why("user@music.example.com").startsWith("That isn't an address"))
        assertTrue(why("mu_sic.example.com").startsWith("That isn't an address"))
        assertTrue(why("https://").startsWith("That isn't an address"))
        assertTrue(Address.isLocal("127.0.0.1"))
        assertTrue(Address.isLocal("172.16.0.1"))
        assertFalse(Address.isLocal("example.com"))
        assertFalse(Address.isLocal("11.0.0.1"))
    }

    private val ours = "0a0b 0.1.0"
    private val theirs = "0c0d 0.2.0"

    private fun health(domain: String) = "ok\nconnections 1\nhead 3\nhorizon 0\ndomain $domain current\n"

    private fun decide(status: Int, body: String) = Health.decide("h", status, body, ours, "Harken")

    @Test
    fun health() {
        assertEquals(Health.Serving(false), decide(200, health(ours)))
        // Another version of the same schema: harken's still.
        assertEquals(Health.Serving(false), decide(200, health("0a0b 0.2.0")))
        // Another schema: a server grown under this phone, or another app — usable either way,
        // the replica taking states on faith (§1.16).
        assertEquals(Health.Serving(true), decide(200, health(theirs)))
        assertEquals(Health.Refused("h is an Ark server with no app on it yet."), decide(200, "ok\nconnections 0\n"))
        assertEquals(Health.Refused("h answered, but not the way an Ark server does."), decide(200, """{"status":"down"}"""))
        assertEquals(Health.Refused("h answered, but not the way an Ark server does."), decide(200, ""))
        assertEquals(Health.Refused("h answered, but it isn't an Ark server: it has no health page."), decide(404, "not found"))
        assertEquals(
            Health.Refused("h answered 503. It may be starting up or broken; try again in a moment."),
            decide(503, "the hub is gone"))
    }

    @Test
    fun probing() {
        val ok = Answer(200, health(ours))
        val asked = ArrayList<String>()
        // A private address: https refused, then http answers.
        val found =
            Probe.find("192.168.1.20:8787", ours, "Harken") { url ->
                asked.add(url)
                if (url.startsWith("https")) throw ConnectException("refused") else ok
            }
        assertEquals(Found.Server("http://192.168.1.20:8787", false), found)
        assertEquals(
            listOf("https://192.168.1.20:8787/healthz", "http://192.168.1.20:8787/healthz"), asked)
        // https answers with no app on it: not asked again in the clear.
        asked.clear()
        val wrong =
            Probe.find("192.168.1.20", ours, "Harken") { url ->
                asked.add(url)
                Answer(200, "ok\n")
            }
        assertTrue((wrong as Found.Failed).why.startsWith("192.168.1.20 is an Ark server with no app"))
        assertEquals(1, asked.size)
        // Nobody answers.
        assertEquals(
            Found.Failed("Couldn't reach 192.168.1.20 over https or http: nothing answered there."),
            Probe.find("192.168.1.20", ours, "Harken") { throw ConnectException("refused") })
        assertEquals(
            Found.Failed("Couldn't reach nowhere.example.com: there is no such name."),
            Probe.find("nowhere.example.com", ours, "Harken") {
                throw UnknownHostException("nowhere.example.com")
            })
        assertEquals(
            Found.Failed("Couldn't reach a.example.com: broken pipe."),
            Probe.find("a.example.com", ours, "Harken") { throw IOException("broken pipe") })
        // A bad address asks nothing.
        assertEquals(
            Found.Failed("Type the address of your server."),
            Probe.find("", ours, "Harken") { error("asked") })
    }

    @Test
    fun remembered() {
        val shelf = MemoryShelf()
        val r = Remembered(shelf)
        assertNull(r.server)
        assertNull(r.login)
        assertEquals(emptySet<String>(), r.asked)
        val login =
            testLogin("ana", "s1", listOf("library"), "Ana", "ana@example.com").copy(token = "tok", expiresMs = 1234, picture = "https://pictures.example.invalid/ana.png")
        r.server = "https://music.example.com"
        r.login = login
        // A second store over the same shelf reads it back whole.
        val again = Remembered(shelf)
        assertEquals("https://music.example.com", again.server)
        assertEquals(login, again.login)
        again.markAsked("notifications")
        again.markAsked("notifications")
        again.markAsked("contacts")
        assertEquals(setOf("notifications", "contacts"), r.asked)
        // Signing out forgets the login and nothing else.
        r.login = null
        assertNull(again.login)
        assertEquals("https://music.example.com", again.server)
        assertEquals(setOf("notifications", "contacts"), again.asked)
        // A login that no longer parses is forgotten rather than trusted.
        shelf.put("ark.login", "{\"token\":")
        assertNull(again.login)
    }
}
