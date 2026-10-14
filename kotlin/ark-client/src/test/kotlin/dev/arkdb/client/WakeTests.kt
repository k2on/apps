// A peer driven by its wakes rather than a clock (`docs/plan-reactive.md`), as
// the reference's `wake_tests` hold `rust/ark-client`: at rest nothing rings
// and nothing is published, a burst is one wake and one settle, the only timer
// is the redial a close arms and an open clears, a mutation goes out with no
// pump, and the status is published when it changes and only then. Driven
// over `Queues` with time handed in (`Peer.pumpAt`), so a minute of rest is a
// minute of simulated milliseconds and not one of waiting. What a server
// would send is made the way a server makes it: an `Authority` sequencing
// another peer's intents, each entry a page of its own.
package dev.arkdb.client

import dev.arkdb.Authority
import dev.arkdb.Canon
import dev.arkdb.ServerMsg
import dev.arkdb.Value
import dev.arkdb.localCommit
import java.io.File
import java.util.concurrent.atomic.AtomicInteger
import kotlin.system.exitProcess

object WakeTests {
    private val problems = ArrayList<String>()

    private fun claim(what: String, ok: Boolean) {
        if (!ok) problems.add(what)
    }

    private fun page(
        domain: Domain,
        items: List<Triple<Long, dev.arkdb.LogEntry, dev.arkdb.FactList>>
    ) = Canon.encode(ServerMsg.Batch(items, false, null, domain.hash, null).toValue())

    /** A peer over sockets the test holds, every wake counted. */
    private class Rig(val domain: Domain) {
        val made = ArrayList<Queues>()
        val wakes = AtomicInteger()
        val peer: Peer =
            Peer.open(
                domain,
                MemoryStorage(),
                Options.server("bob", "b", null).copy(autos = Autos.Seeded(3)))

        init {
            peer.setWaker { wakes.incrementAndGet() }
            peer.connect("queues", { _ -> Queues().also { made.add(it) } })
        }

        fun socket(): Queues = made.last()

        /** Dialled, open, served to the end: what a host's wakes come to on a first sign-in. */
        fun linked(now: Long) {
            peer.pumpAt(now)
            socket().push(Event.Opened)
            peer.pumpAt(now)
            socket().push(Event.Frame(WakeTests.page(domain, emptyList())))
            peer.pumpAt(now)
            socket().sent.clear()
        }
    }

    /**
     * Connected and idle, a peer is a minute of nothing: no wake, no deadline for a host to arm,
     * and every pump a host might make anyway publishes nothing — not its status, not its views.
     */
    fun atRest(domain: Domain) {
        val r = Rig(domain)
        r.linked(1_000)
        claim("rest: linked", r.peer.linked)
        claim("rest: caught up, quiet", r.peer.pumpAt(1_000).quiet())
        val woken = r.wakes.get()
        for (s in 1..60L) {
            val p = r.peer.pumpAt(1_000 + s * 1_000)
            claim("rest: second $s published something", p.quiet())
            claim("rest: second $s has a timer, ${p.deadline}", p.deadline == null)
        }
        claim("rest: no deadline while open", r.peer.deadline() == null)
        claim("rest: a minute at rest rang ${r.wakes.get() - woken}", r.wakes.get() == woken)
        println("wake: a connected peer at rest is never woken and publishes nothing")
    }

    /**
     * What the app asks for goes at once: a mutation is pushed by `mutate` itself, with no pump.
     */
    fun mutateSends(domain: Domain) {
        val r = Rig(domain)
        r.linked(1_000)
        r.peer.mutate("create_playlist", mapOf("name" to Value.text("Now")))
        claim("mutate: pushed by the call, ${r.socket().sent.size}", r.socket().sent.size == 1)
        val p = r.peer.pumpAt(2_000)
        claim("mutate: the pump after it has views", p.views)
        claim("mutate: and nothing more to send", r.socket().sent.size == 1)
        println("wake: a mutation is pushed by the call that made it")
    }

    /**
     * Fifty pages arriving while nobody pumps are one wake — the "drain requested" flag — and the
     * one pump that answers them settles once.
     */
    fun burst(domain: Domain) {
        val r = Rig(domain)
        r.linked(1_000)
        // Alice, on the same server, makes fifty playlists; each is a page to bob.
        val alice =
            Peer.open(
                domain,
                MemoryStorage(),
                Options.server("alice", "a", null).copy(autos = Autos.Seeded(4)))
        repeat(50) { alice.mutate("create_playlist", mapOf("name" to Value.text("p$it"))) }
        val a = Authority(domain.module.schema, domain.closures)
        localCommit(a, alice.replica)
        val entries = a.log.takeEntries()
        claim("burst: fifty entries, ${entries.size}", entries.size == 50)
        val woken = r.wakes.get()
        val q = r.socket()
        for (e in entries) q.push(Event.Frame(page(domain, listOf(e))))
        claim(
            "burst: ${entries.size} frames, ${r.wakes.get() - woken} wakes",
            r.wakes.get() - woken == 1)
        val settled = r.peer.settles
        val p = r.peer.pumpAt(2_000)
        claim("burst: ${r.peer.settles - settled} settles", r.peer.settles - settled == 1L)
        claim("burst: the views moved", p.views)
        claim(
            "burst: fifty playlists, ${r.peer.store.scan("playlist").size}",
            r.peer.store.scan("playlist").size == 50)
        // The next frame after that pump rings again.
        q.push(Event.Frame(page(domain, emptyList())))
        claim("burst: the flag was answered by the pump", r.wakes.get() - woken == 2)
        println("wake: a burst of ${entries.size} frames is one wake and one settle")
    }

    /**
     * A close arms exactly one timer — the redial, at the backoff — and the deadline stays put
     * until it is due, so a host keyed by it arms nothing twice; the dial clears it, and the open
     * after it leaves none.
     */
    fun redial(domain: Domain) {
        val r = Rig(domain)
        r.linked(1_000)
        val woken = r.wakes.get()
        r.socket().push(Event.Closed("gone"))
        claim("redial: the close is a wake", r.wakes.get() == woken + 1)
        val p = r.peer.pumpAt(1_000)
        claim("redial: dropped, ${p.dropped}", p.dropped == "gone")
        claim("redial: one redial, half a second out, ${p.deadline}", p.deadline == 1_500L)
        for (t in listOf(1_100L, 1_250L, 1_499L)) {
            val q = r.peer.pumpAt(t)
            claim("redial: the same deadline at $t, ${q.deadline}", q.deadline == 1_500L)
            claim("redial: quiet at $t", q.quiet())
        }
        claim("redial: not dialled early", r.made.size == 1)
        val d = r.peer.pumpAt(1_500)
        claim("redial: dialled at the deadline", r.made.size == 2)
        claim("redial: dialling, no timer, ${d.deadline}", d.deadline == null)
        r.socket().push(Event.Opened)
        claim("redial: the open is a wake", r.wakes.get() == woken + 2)
        val o = r.peer.pumpAt(1_520)
        claim("redial: opened", o.opened)
        claim("redial: an open cancels it", o.deadline == null && r.peer.deadline() == null)
        println("wake: a close arms one redial and an open clears it")
    }

    /** The status is published when it differs by value, once, and not again until it moves. */
    fun statusByValue(domain: Domain) {
        val r = Rig(domain)
        claim("status: the first pump says it", r.peer.pumpAt(0).status != null)
        r.linked(1_000)
        claim("status: then nothing", r.peer.pumpAt(1_000).status == null)
        r.peer.mutate("create_playlist", mapOf("name" to Value.text("One")))
        val p = r.peer.pumpAt(1_001)
        claim("status: one pending is said, ${p.status?.pending}", p.status?.pending == 1)
        claim("status: and said once", r.peer.pumpAt(1_002).status == null)
        println("wake: the status is published when it changes, and only then")
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val domain =
            Domain.fromBytes(File(args.getOrElse(0) { "../harken/domain/harken.ark" }).readBytes())
        atRest(domain)
        mutateSends(domain)
        burst(domain)
        redial(domain)
        statusByValue(domain)
        if (problems.isNotEmpty()) {
            println("wake: ${problems.size} problem(s)")
            for (p in problems) println("  $p")
            exitProcess(1)
        }
        println("wake: every claim held")
    }
}
