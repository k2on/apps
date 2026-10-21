// A launch reads what Home draws first: the songs, the playlists and the
// albums are published before the artists and the songs' details are so
// much as read — over harken's domain in the kernel, a peer holding thirty
// songs authored through `add_song` as the library account.
package dev.harken.android

import dev.harken.android.onboarding.ArkSession
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LaunchTests {
    private data class Seen(val loaded: Boolean, val tracks: Int, val albums: Int, val artists: Int, val details: Int, val complete: Boolean)

    @Test
    fun homeIsPublishedBeforeTheRestIsHydrated() {
        val h = alone()
        repeat(30) { h.addSong(song(it, album = "Album ${it / 5}")) }
        val session = ArkSession.over(h, "https://music.example.com", testLogin("library", roles = listOf("library")))
        // The session's thread held while the library is made, so that what is queued is in a known
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
