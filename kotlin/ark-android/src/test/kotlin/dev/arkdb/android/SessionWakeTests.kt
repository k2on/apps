// The session runs no loop (`docs/plan-reactive.md`): its peer's thread
// drains when the socket wakes it, after a post, and at the one deadline the
// peer asks for, and at no other time. Over the real `ArkSession`, its real
// thread, and a socket the test holds: open and idle, nothing runs and
// nothing is published; a burst is one drain; a close arms one timer, which
// a dial cancels; a mutation goes out inside the call that made it; the
// status is said when it changes.
package dev.arkdb.android

import dev.arkdb.Authority
import dev.arkdb.Canon
import dev.arkdb.ServerMsg
import dev.arkdb.Value
import dev.arkdb.client.Account
import dev.arkdb.client.Autos
import dev.arkdb.client.Domain
import dev.arkdb.client.Event
import dev.arkdb.client.Login
import dev.arkdb.client.MemoryStorage
import dev.arkdb.client.Options
import dev.arkdb.client.Peer
import dev.arkdb.client.Queues
import dev.arkdb.client.Socket
import dev.arkdb.client.Timing
import dev.arkdb.client.Wake
import dev.arkdb.localCommit
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionWakeTests {
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

    /** A socket the test holds, counting the polls — one per pump the link makes of it. */
    private class Counted : Socket {
        val q = Queues()
        val polls = AtomicInteger()

        override fun send(frame: ByteArray) = q.send(frame)

        override fun poll(): List<Event> {
            polls.incrementAndGet()
            return q.poll()
        }

        override fun close() = q.close()

        override fun setWaker(wake: Wake) = q.setWaker(wake)
    }

    private fun page(
        items: List<Triple<Long, dev.arkdb.LogEntry, dev.arkdb.FactList>> = emptyList()
    ) = Canon.encode(ServerMsg.Batch(items, false, null, domain.hash, null).toValue())

    /** A session over bob's peer, its sockets, and everything it tells counted. */
    private inner class Rig(timing: Timing = Timing()) {
        val made = CopyOnWriteArrayList<Counted>()
        val peer: Peer =
            Peer.open(
                domain,
                MemoryStorage(),
                Options.server("bob", "b", null).copy(autos = Autos.Seeded(3), timing = timing))
        val told = AtomicInteger()
        val statuses = AtomicInteger()
        val session: ArkSession
        private val scope = CoroutineScope(Dispatchers.Default)

        init {
            peer.connect("queues", { _ -> Counted().also { made.add(it) } })
            session =
                ArkSession.over(
                    peer, "https://music.example.com", Login("t", "b", Account("bob"), 0))
            session.onChange { _, _ -> told.incrementAndGet() }
            scope.launch(Dispatchers.Unconfined) {
                session.status.collect { statuses.incrementAndGet() }
            }
        }

        fun socket(): Counted = made.last()

        fun polls(): Int = made.sumOf { it.polls.get() }

        /** Dialled, opened and served: linked, and quiet after. */
        fun linked() {
            await("dialled") { made.isNotEmpty() }
            socket().q.push(Event.Opened)
            await("linked") { session.status.value.linked }
            socket().q.push(Event.Frame(page()))
            await("synced") { session.status.value.synced }
            quiet()
        }

        /** Wait until the peer's thread has run everything asked of it so far. */
        fun quiet() {
            runBlocking { session.run {} }
            Thread.sleep(100)
            runBlocking { session.run {} }
        }

        fun close() {
            scope.cancel()
            session.close()
        }
    }

    private fun await(what: String, ok: () -> Boolean) {
        val until = System.nanoTime() + 5_000_000_000
        while (!ok()) {
            if (System.nanoTime() > until) throw AssertionError("never $what")
            Thread.sleep(5)
        }
    }

    /**
     * Open, linked and idle, a session runs nothing for two seconds: no drain, no listener, no
     * status, no timer.
     */
    @Test
    fun aSessionAtRestRunsNothingAndPublishesNothing() {
        val r = Rig()
        try {
            r.linked()
            val (drains, told, statuses, polls) =
                listOf(r.session.drains.get(), r.told.get(), r.statuses.get(), r.polls())
            Thread.sleep(2_000)
            assertEquals("drains at rest", drains, r.session.drains.get())
            assertEquals("pumps at rest", polls, r.polls())
            assertEquals("listeners told at rest", told, r.told.get())
            assertEquals("statuses published at rest", statuses, r.statuses.get())
            assertEquals("no timer while open", false, r.session.timed)
        } finally {
            r.close()
        }
    }

    /** Fifty pages arriving while the thread is busy are one drain, one settle and one change. */
    @Test
    fun aBurstOfFramesIsOneDrain() {
        val r = Rig()
        try {
            r.linked()
            val alice =
                Peer.open(
                    domain,
                    MemoryStorage(),
                    Options.server("alice", "a", null).copy(autos = Autos.Seeded(4)))
            repeat(50) { alice.mutate("create_playlist", mapOf("name" to Value.text("p$it"))) }
            val a = Authority(domain.module.schema, domain.closures)
            localCommit(a, alice.replica)
            val entries = a.log.takeEntries()
            val drains = r.session.drains.get()
            val told = r.told.get()
            val settles = r.peer.settles
            // The thread held, so that every frame lands before any drain can start.
            val held = CountDownLatch(1)
            r.session.post { held.await(5, TimeUnit.SECONDS) }
            for (e in entries) r.socket().q.push(Event.Frame(page(listOf(e))))
            held.countDown()
            r.quiet()
            assertEquals("drains for ${entries.size} frames", 1, r.session.drains.get() - drains)
            assertEquals("settles", 1L, r.peer.settles - settles)
            assertEquals("listeners told", 1, r.told.get() - told)
            assertEquals(50, runBlocking { r.session.run { it.store.scan("playlist").size } })
        } finally {
            r.close()
        }
    }

    /**
     * A close arms one timer at the backoff, which fires once and dials; nothing more after the
     * open.
     */
    @Test
    fun aCloseArmsOneRedialThatDials() {
        val r = Rig(Timing(firstBackoffMs = 200))
        try {
            r.linked()
            val armed = r.session.armed.get()
            val drains = r.session.drains.get()
            r.socket().q.push(Event.Closed("gone"))
            await("redialled") { r.made.size == 2 }
            r.quiet()
            assertEquals("one timer for the redial", 1, r.session.armed.get() - armed)
            assertEquals("the close, and the timer", 2, r.session.drains.get() - drains)
            assertEquals("dialling, no timer", false, r.session.timed)
            r.socket().q.push(Event.Opened)
            await("linked again") { r.session.status.value.linked }
            r.quiet()
            val after = r.session.drains.get()
            Thread.sleep(1_000)
            assertEquals("open: nothing more", after, r.session.drains.get())
            assertEquals("no timer armed since", 1, r.session.armed.get() - armed)
        } finally {
            r.close()
        }
    }

    /** A dial before the deadline cancels its timer: it never fires. */
    @Test
    fun aDialCancelsTheTimer() {
        val r = Rig(Timing(firstBackoffMs = 60_000))
        try {
            r.linked()
            r.socket().q.push(Event.Closed("gone"))
            await("the timer") { r.session.timed }
            // Back online by hand, long before the minute: dialled at once, the timer gone.
            r.session.post {
                it.disconnect()
                it.reconnect()
            }
            await("dialled") { r.made.size == 2 }
            r.quiet()
            assertEquals("the timer cancelled", false, r.session.timed)
            assertEquals("one timer, ever", 1, r.session.armed.get())
        } finally {
            r.close()
        }
    }

    /**
     * `mutate` sends inside the call, with no drain; the status says one pending once, then
     * nothing.
     */
    @Test
    fun aMutationSendsAtOnceAndTheStatusIsSaidOnce() {
        val r = Rig()
        try {
            r.linked()
            val drains = r.session.drains.get()
            val statuses = r.statuses.get()
            val sent = runBlocking {
                r.session.run { p ->
                    r.socket().q.sent.clear()
                    p.mutate("create_playlist", mapOf("name" to Value.text("Now")))
                    r.socket().q.sent.size
                }
            }
            assertEquals("pushed by mutate, in a read that drains nothing", 1, sent)
            assertEquals("no drain for it", drains, r.session.drains.get())
            r.session.post {}
            r.quiet()
            assertEquals("pending said", 1, r.session.status.value.pending)
            assertEquals("said once", 1, r.statuses.get() - statuses)
            val told = r.told.get()
            r.session.post {}
            r.quiet()
            assertEquals("a post that changes nothing tells nobody", told, r.told.get())
            assertEquals("nor publishes a status", 1, r.statuses.get() - statuses)
        } finally {
            r.close()
        }
    }

    /**
     * Not a claim but a measurement (`docs/plan-reactive.md`, "Landed: the Kotlin half"): a session
     * open, linked and idle for ten seconds, and the pumps and listener calls it made — printed.
     */
    @Test
    fun measureTenSecondsAtRest() {
        if (System.getenv("ARK_MEASURE") == null) return
        val r = Rig()
        try {
            r.linked()
            val (drains, told, polls) = listOf(r.session.drains.get(), r.told.get(), r.polls())
            Thread.sleep(10_000)
            println(
                "measure: 10 s at rest: ${r.polls() - polls} pumps, ${r.session.drains.get() - drains} drains, " +
                    "${r.told.get() - told} listener calls")
            assertTrue(true)
        } finally {
            r.close()
        }
    }
}
