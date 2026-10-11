// The client's driver half, held without a network: the link's backoff and
// its stops (the reference's `a_link_backs_off_doubling_and_resets_when_it_opens`,
// over `Queues`), the JSON dialect a headless peer reads and answers in, the
// sign-in's URLs, and a maintained view over harken's own module — the list a
// screen splices, held after every change to a fresh read.
package dev.arkdb.client

import dev.arkdb.Changes
import dev.arkdb.Id
import dev.arkdb.Json
import dev.arkdb.JsonError
import dev.arkdb.Value
import java.io.File
import kotlin.system.exitProcess

object ClientTests {
    private val problems = ArrayList<String>()

    private fun claim(what: String, ok: Boolean) {
        if (!ok) problems.add(what)
    }

    /** Every dial hands back a fresh `Queues` and records it. */
    fun link() {
        val made = ArrayList<Queues>()
        val dial: Dial = { _ -> Queues().also { made.add(it) } }
        val l = Link("ws://x/sync", dial, Timing())
        l.pollAt(0)
        claim("link: dials at once", made.size == 1)
        made[0].events.add(Event.Closed("refused"))
        val p = l.pollAt(0)
        claim("link: a socket that never opened is not a disconnect", p.closed == null)
        claim("link: waits the first backoff, ${l.state}", l.state == LinkState.Waiting(500))
        l.pollAt(499)
        claim("link: not before the backoff", made.size == 1)
        l.pollAt(500)
        claim("link: dials at the backoff", made.size == 2)
        made[1].events.add(Event.Closed("refused"))
        l.pollAt(500)
        claim("link: the backoff doubled, ${l.state}", l.state == LinkState.Waiting(1500))
        l.pollAt(1500)
        val q = made[2]
        q.events.add(Event.Opened)
        q.events.add(Event.Frame(byteArrayOf(1)))
        val o = l.pollAt(1500)
        claim("link: opened", o.opened && l.opens == 1L)
        claim("link: a frame", o.frames.size == 1 && o.frames[0].contentEquals(byteArrayOf(1)))
        l.send(byteArrayOf(9))
        claim("link: sent on the open socket", q.sent.size == 1 && q.sent[0][0] == 9.toByte())
        q.events.add(Event.Closed("gone"))
        val c = l.pollAt(2000)
        claim("link: a disconnect, ${c.closed}", c.closed == "gone")
        claim(
            "link: the backoff started over when it opened, ${l.state}",
            l.state == LinkState.Waiting(2500))
        claim("link: it was not open", !l.stop("offline"))
        l.pollAt(99_999)
        claim("link: a stopped link does not dial", made.size == 3)
        l.resume()
        l.pollAt(99_999)
        claim("link: resumed, it dials", made.size == 4)
        println("client: the link backs off doubling, resets when it opens, and stops")
    }

    /** The dialect: every wrapper, an escape of each kind, a key order not insertion order. */
    fun json() {
        val v =
            Value.record(
                "b" to
                    Value.list(
                        Value.int(-1),
                        Value.bytes(byteArrayOf(0xab.toByte(), 1)),
                        Value.id(Id.nil)),
                "a" to Value.text("q\"\\\u001f水"),
                "c" to Value.VNull,
                "d" to Value.bool(true))
        val printed = Json.json(v)
        claim(
            "json: printed $printed",
            printed ==
                """{"a":"q\"\\\u001f水","b":[{"${'$'}int":"-1"},{"${'$'}bytes":"ab01"},{"${'$'}id":"00000000-0000-0000-0000-000000000000"}],"c":null,"d":true}""")
        claim("json: read back", Json.decode(printed) == v)
        claim(
            "json: a bare integer",
            Json.decode(" { \"n\" : 3 } ") == Value.record("n" to Value.int(3)))
        claim(
            "json: a surrogate pair",
            Json.decode("\"\\ud83c\\udfb5\"") == Value.text("\ud83c\udfb5"))
        for ((bad, says) in
            listOf(
                "1.5" to "no floats",
                "{\"\$int\":3}" to "wraps text",
                "{\"\$int\":\"x\"}" to "a decimal integer",
                "{\"a\":1,\"a\":2}" to "a key twice",
                "[1,]" to "not a value",
                "\"\\ud83c\"" to "half a surrogate pair",
                "1 2" to "something after the value")) {
            val e = runCatching { Json.decode(bad) }.exceptionOrNull()
            claim(
                "json: $bad is refused with $says, ${e?.message}", e is JsonError && says in e.why)
        }
        println("client: the dialect prints and reads as the reference does")
    }

    /** The sign-in's URLs, as `ark_auth` spells them. */
    fun auth() {
        claim(
            "auth: login url",
            Auth.loginUrl("http://h:1/", "http://127.0.0.1:9/", "a b") ==
                "http://h:1/auth/login?redirect=http%3A%2F%2F127.0.0.1%3A9%2F&user=a%20b")
        claim("auth: socket of http", Auth.socketUrl("http://h:1") == "ws://h:1/sync")
        claim("auth: socket of https", Auth.socketUrl("https://h/") == "wss://h/sync")
        claim("auth: a code", Auth.queryValue("?code=a%2Bb&x=1", "code") == "a+b")
        val l = Login("t", "s", Account("alice", roles = listOf("library")), 7)
        claim("auth: a login read back", Login.parse(l.toJson()) == l)
        claim(
            "auth: no roles, no key",
            !Login("t", "s", Account("bob"), 7).toJson().contains("roles"))
        println("client: the sign-in's URLs and its login as ark_auth writes them")
    }

    /**
     * A peer alone over harken.ark: `playlists` and `library` held as views, and after every
     * mutation the patches spliced into a list of the screen's own are the fresh read — through a
     * create, an add, a remove and a reopen of the view.
     */
    fun views(harken: File) {
        val domain = Domain.fromBytes(harken.readBytes())
        val p =
            Peer.open(domain, MemoryStorage(), Options.alone("me").copy(autos = Autos.Seeded(7)))
        val playlists = p.view("playlists", emptyMap())
        val shown = ArrayList(playlists.rows)
        var patched = 0
        fun step(what: String) {
            when (val u = playlists.update(p, p.takeChanges())) {
                is Update.Patched -> {
                    patched++
                    ClientView.splice(shown, u.patches) { it }
                }
                is Update.Reset -> {
                    shown.clear()
                    shown.addAll(playlists.rows)
                }
                is Update.Unchanged -> {}
            }
            val fresh = p.query("playlists", emptyMap()).asList()
            claim("views: $what: the view is the fresh read", playlists.rows == fresh)
            claim("views: $what: the spliced list is the fresh read", shown == fresh)
        }
        p.mutate("create_playlist", mapOf("name" to Value.text("Road trip")))
        step("a playlist made")
        p.mutate("create_playlist", mapOf("name" to Value.text("Road trip")))
        step("the same name again")
        val id = playlists.rows[0].field("id")
        val songs = p.view("playlist", mapOf("playlist_id" to id))
        claim("views: an empty playlist", songs.rows.isEmpty())
        // Nothing to put on it: the library is the scanner's, and a peer alone
        // has none. `add_to_playlist` of a missing medium is a no-op, and says so
        // by changing nothing.
        p.mutate("add_to_playlist", mapOf("playlist_id" to id, "media_id" to Value.id(Id.nil)))
        val u = songs.update(p, Changes.Applied(emptyList()))
        claim("views: nothing moved, nothing patched", u == Update.Unchanged)
        step("an add of nothing")
        claim("views: patched, not re-read: $patched", patched == 2)
        val refused = runCatching { p.view("playlist", mapOf("playlist_id" to Value.id(Id.nil))) }
        claim(
            "views: a view the middleware refuses is refused at open",
            refused.exceptionOrNull() is Refused)
        println("client: views over harken.ark patch as a fresh read does ($patched patched)")
    }

    @JvmStatic
    fun main(args: Array<String>) {
        link()
        json()
        auth()
        views(File(args.getOrElse(0) { "../harken/domain/harken.ark" }))
        if (problems.isNotEmpty()) {
            println("client: ${problems.size} problem(s)")
            for (p in problems) println("  $p")
            exitProcess(1)
        }
        println("client: every claim held")
    }
}
