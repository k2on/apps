// A launch reads what Home draws first: the songs, the playlists and the
// albums are published before the artists and the songs' details are so
// much as hydrated — over harken.ark, a peer holding thirty songs authored
// through `add_song` as the library account.
package dev.harken.android

import dev.arkdb.Value
import dev.arkdb.android.ArkSession
import dev.arkdb.client.Account
import dev.arkdb.client.Autos
import dev.arkdb.client.Domain
import dev.arkdb.client.Login
import dev.arkdb.client.MemoryStorage
import dev.arkdb.client.Options
import dev.arkdb.client.Peer
import java.io.File
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
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
}
