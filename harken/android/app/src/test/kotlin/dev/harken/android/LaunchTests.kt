// A launch reads what Home draws first: the songs, the playlists and the
// albums are published before the artists and the songs' details are so
// much as hydrated — over harken.ark, a peer holding thirty songs authored
// through `add_song` as the library account. And, run when
// HARKEN_LAUNCH_BENCH names a library's size, what the process owning the
// session (Kept.kt) costs a launch against opening it the way the activity
// used to.
package dev.harken.android

import dev.arkdb.Authority
import dev.arkdb.Value
import dev.arkdb.localCommit
import dev.arkdb.android.ArkSession
import dev.arkdb.client.Account
import dev.arkdb.client.Autos
import dev.arkdb.client.Domain
import dev.arkdb.client.Login
import dev.arkdb.client.MemoryStorage
import dev.arkdb.client.Options
import dev.arkdb.client.Peer
import java.io.File
import java.nio.file.Files
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test

class LaunchTests {
    private fun harken(): ByteArray {
        var dir: File? = File("").absoluteFile
        while (dir != null) {
            File(dir, "domain/harken.ark").takeIf { it.isFile }?.let { return it.readBytes() }
            File(dir, "harken/domain/harken.ark").takeIf { it.isFile }?.let { return it.readBytes() }
            dir = dir.parentFile
        }
        error("no harken.ark above ${File("").absolutePath}")
    }

    private fun song(i: Int): Map<String, Value> =
        mapOf(
            "title" to Value.text("Song $i"),
            "artist" to Value.text("Artist ${i / 10}"),
            "album" to Value.text("Album ${i / 5}"),
            "duration_ms" to Value.int(180_000),
            "file" to Value.text("music/$i.flac"),
            "track" to Value.int((i % 5 + 1).toLong()),
            "part" to Value.text(""),
            "catalogue" to Value.text(""),
            "performer" to Value.text(""),
            "bpm" to Value.int(0),
            "album_art" to Value.text(""),
            "artist_art" to Value.text(""),
            "disc" to Value.int(1),
            "work_title" to Value.text(""),
            "movement_no" to Value.int(0),
        )

    private data class Seen(val loaded: Boolean, val tracks: Int, val albums: Int, val artists: Int, val details: Int, val complete: Boolean)

    @Test
    fun homeIsPublishedBeforeTheRestIsHydrated() {
        val peer = Peer.open(Domain.fromBytes(harken()), MemoryStorage(), Options.alone("library").copy(autos = Autos.Seeded(3)))
        peer.setRoles(listOf("library"))
        repeat(30) { peer.mutate("add_song", song(it)) }
        val session = ArkSession.over(peer, "https://music.example.com", Login("t", "s", Account("library"), 0))
        // The peer's thread held while the library is made, so that what is queued is in a known
        // order: the first stage, then this look at what a screen sees — and the second stage only
        // when the first has run, behind the look.
        val gate = CountDownLatch(1)
        session.post { gate.await() }
        val library = Library(session)
        val first = CompletableFuture<Seen>()
        session.post {
            first.complete(
                Seen(
                    library.loaded.value,
                    library.tracks.value.size,
                    library.albums.value.size,
                    library.artists.value.size,
                    library.details.value.size,
                    library.complete.value,
                ))
        }
        gate.countDown()
        val seen = first.get(10, TimeUnit.SECONDS)
        assertEquals(Seen(loaded = true, tracks = 30, albums = 6, artists = 0, details = 0, complete = false), seen)
        val deadline = System.currentTimeMillis() + 10_000
        while (!library.complete.value && System.currentTimeMillis() < deadline) Thread.sleep(5)
        assertTrue(library.complete.value)
        assertEquals(3, library.artists.value.size)
        assertEquals(30, library.details.value.size)
        assertFalse(library.tracks.value.isEmpty())
        library.close()
        session.close()
    }

    // A replica as a phone keeps it, at `root`: thirty songs confirmed, as the server confirms the
    // scanner's — written by the client's own writer, then laid out as the session's directory.
    private fun seeded(root: File, server: String, bytes: ByteArray) {
        val domain = Domain.fromBytes(bytes)
        val mem = MemoryStorage()
        val p = Peer.open(domain, mem, Options.server("library", "scanner", null).copy(autos = Autos.Seeded(2)))
        p.setRoles(listOf("library"))
        repeat(30) { p.mutate("add_song", song(it)) }
        localCommit(Authority(domain.module.schema, domain.closures), p.replica)
        p.persist()
        val dir = ArkSession.directory(root, server)
        dir.mkdirs()
        for (k in mem.keys()) File(dir, k).writeBytes(mem.load(k)!!)
    }

    private fun await(what: String, ok: () -> Boolean) {
        val deadline = System.currentTimeMillis() + 15_000
        while (!ok() && System.currentTimeMillis() < deadline) Thread.sleep(5)
        assertTrue(what, ok())
    }

    /**
     * A remembered launch with its launch views written: Home is published from them before the
     * peer has opened — the peer held shut here — and when it opens the live views find the same
     * lists and publish nothing. A playlist made in that window is made once, after the open.
     */
    @Test
    fun aLaunchViewIsHomeBeforeThePeerOpens() {
        val bytes = harken()
        val root = Files.createTempDirectory("harken").toFile()
        val server = "https://music.example.invalid"
        val login = Login("t", "s", Account("owner"), 0)
        try {
            seeded(root, server, bytes)
            // The first launch: no files yet; they are written when the app goes to the background.
            val first = ArkSession.open({ Domain.fromBytes(bytes) }, root, server, login, {}, {}, module = bytes)
            val one = Library(first)
            await("the first launch hydrated") { one.complete.value }
            val saved = CountDownLatch(1)
            first.saveLaunchViews { saved.countDown() }
            assertTrue(saved.await(10, TimeUnit.SECONDS))
            one.close()
            first.close()

            // The second, with the peer's open held until the test lets it go.
            val gate = CountDownLatch(1)
            val second = ArkSession.open({ gate.await(); Domain.fromBytes(bytes) }, root, server, login, {}, {}, module = bytes)
            val two = Library(second)
            await("Home was published from the launch views") { two.loaded.value }
            assertTrue("provisional", two.provisional.value)
            assertTrue("the splash may go", second.isReady.value)
            assertEquals(30, two.tracks.value.size)
            assertEquals(6, two.albums.value.size)
            val shown = two.tracks.value
            val shownAlbums = two.albums.value
            assertEquals("three lists published, from the files", 3, two.homePublishes.get())
            // A tap before the peer is open: posted, and run after it.
            two.createPlaylist("Tapped")
            gate.countDown()
            await("the live views answered") { !two.provisional.value && two.complete.value }
            await("the playlist is there") { two.playlists.value.any { it.name == "Tapped" } }
            assertSame("the live library equals what was shown: nothing was published", shown, two.tracks.value)
            assertSame("nor the albums", shownAlbums, two.albums.value)
            assertEquals(1, two.playlists.value.count { it.name == "Tapped" })
            // The live views published nothing; the playlists moved once, for the tap.
            assertEquals("publishes after the files: the tap's alone", 4, two.homePublishes.get())
            val pending = runBlocking { second.run { it.replica.pending.size } }
            assertEquals("the tap is one intent", 1, pending)
            two.close()
            second.close()
        } finally {
            root.deleteRecursively()
        }
    }

    /**
     * Launch → Home's lists, and launch → everything hydrated, over a replica of N songs with its
     * launch views written: the activity's old way — `ArkSession.open` and a `Library` of its own —
     * and the process's — a hold on `Sessions`, and what is kept with the session — run turn about
     * in one JVM, twelve of each, the first two of each thrown away. HotSpot's numbers, warm: a
     * relative measure of what the ownership adds, not a phone's launch.
     */
    @Test
    fun launchBench() {
        val n = System.getenv("HARKEN_LAUNCH_BENCH")?.toIntOrNull()
        assumeTrue("set HARKEN_LAUNCH_BENCH to a library size to run it", n != null)
        val bytes = harken()
        val root = Files.createTempDirectory("harken-bench").toFile()
        val server = "https://music.example.invalid"
        val login = Login("t", "s", Account("owner"), 0)
        try {
            val domain = Domain.fromBytes(bytes)
            val mem = MemoryStorage()
            val p = Peer.open(domain, mem, Options.server("library", "scanner", null).copy(autos = Autos.Seeded(2)))
            p.setRoles(listOf("library"))
            repeat(n!!) { p.mutate("add_song", song(it)) }
            localCommit(Authority(domain.module.schema, domain.closures), p.replica)
            p.persist()
            val dir = ArkSession.directory(root, server)
            dir.mkdirs()
            for (k in mem.keys()) File(dir, k).writeBytes(mem.load(k)!!)
            val open = { ev: dev.arkdb.android.SessionEvents ->
                ArkSession.open({ Domain.fromVerifiedBytes(bytes) }, root, server, login, ev::denied, ev::signOut, ev::failed, module = bytes)
            }
            val sessions = dev.arkdb.android.Sessions(open = { _, _, ev -> open(ev) })
            val times = mapOf("direct" to ArrayList<Pair<Long, Long>>(), "held" to ArrayList())
            fun once(how: String, i: Int) {
                val t0 = System.nanoTime()
                val close: () -> Unit
                val library: Library
                if (how == "direct") {
                    val s = open(dev.arkdb.android.SessionEvents.None)
                    library = Library(s)
                    close = {
                        library.close()
                        s.close()
                    }
                } else {
                    val h = sessions.hold(server, login, "app")
                    library = Kept.of(h.session, "Phone").library
                    close = { h.release() }
                }
                while (!library.loaded.value) Thread.sleep(0, 100_000)
                val t1 = System.nanoTime()
                while (!library.complete.value) Thread.sleep(0, 100_000)
                val t2 = System.nanoTime()
                close()
                // A closed session writes itself down on its own thread: room for that before the
                // next opens the same replica.
                val rest = if (n >= 10_000) 2_000L else 150L
                Thread.sleep(rest)
                if (i == 0 && how == "direct") {
                    val saved = CountDownLatch(1)
                    // The launch views, written once, as the app going to the background writes them.
                    var failed: Exception? = null
                    val s = open(object : dev.arkdb.android.SessionEvents {
                        override fun failed(e: Exception) {
                            failed = e
                        }
                    })
                    val l = Library(s)
                    val deadline = System.currentTimeMillis() + 120_000
                    while (!l.complete.value && System.currentTimeMillis() < deadline) Thread.sleep(5)
                    assertTrue("the first launch hydrated: ${failed?.stackTraceToString()}", l.complete.value)
                    s.saveLaunchViews { saved.countDown() }
                    assertTrue(saved.await(60, TimeUnit.SECONDS))
                    l.close()
                    s.close()
                    Thread.sleep(rest)
                }
                if (i >= 2) times.getValue(how).add((t1 - t0) / 1000 to (t2 - t0) / 1000)
            }
            for (i in 0 until 12) {
                once("direct", i)
                once("held", i)
            }
            fun med(l: List<Long>) = l.sorted()[l.size / 2]
            for ((how, t) in times)
                println("launch bench, $n songs, $how: Home's lists ${med(t.map { it.first })} µs median ${t.map { it.first }.sorted()}; hydrated ${med(t.map { it.second })} µs median ${t.map { it.second }.sorted()}")
        } finally {
            root.deleteRecursively()
        }
    }
}
