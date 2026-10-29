// What a row asks for as it enters a list, held to what it asked for before
// the asking was made cheap: the album a song is on, found by its key in a map
// rather than by a scan of every album, gives the same album for every song
// of a seeded library — its own row, the made-up one while the albums have
// not caught up, and none for a song on no album.
package dev.harken.android

import dev.arkdb.ffi.Id
import dev.harken.android.onboarding.ArkSession
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RowTests {
    /** What `albumOf` was: a scan of every album, per song. */
    private fun scanned(t: Track, details: Map<Id, TrackDetail>, albums: List<Album>): Album? =
        details[t.id]?.takeIf { it.albumId.isNotEmpty() }?.let { d -> albums.firstOrNull { it.id == d.albumId } ?: Album(d.albumId, d.album, t.creator, "", 0) }

    @Test
    fun theAlbumByKeyIsTheAlbumTheScanFound() {
        val peer = alone()
        // Seven albums of six songs, and every seventh song on none.
        repeat(49) { peer.addSong(song(it, album = if (it % 7 == 6) "" else "Album ${it / 7}")) }
        val session = ArkSession.inline(peer, "https://music.example.com", testLogin("library", roles = listOf("library")))
        val library = Library(session)
        try {
            val deadline = System.currentTimeMillis() + 15_000
            while (!library.complete.value && System.currentTimeMillis() < deadline) Thread.sleep(5)
            assertTrue("the library hydrated", library.complete.value)
            val tracks = library.tracks.value
            val details = library.details.value
            val albums = library.albums.value
            assertEquals(49, tracks.size)
            assertEquals(7, albums.size)
            // Every album as the module answers it; and again with one missing, as while a new
            // album's row has not been read: then the song's album is made of its key and name.
            for (shown in listOf(albums, albums.drop(1))) {
                val byId = shown.associateBy { it.id }
                for (t in tracks) assertEquals("the album of ${t.title}", scanned(t, details, shown), Album.ofTrack(t, details, byId))
            }
            assertEquals("songs on no album", 7, tracks.count { Album.ofTrack(it, details, albums.associateBy { a -> a.id }) == null })
            val gone = albums.first()
            val one = tracks.first { details[it.id]?.albumId == gone.id }
            assertEquals(Album(gone.id, gone.name, one.creator, "", 0), Album.ofTrack(one, details, emptyMap()))
            assertNull(Album.ofTrack(one, emptyMap(), albums.associateBy { it.id }))
        } finally {
            library.close()
            session.close()
            peer.peer.close()
        }
    }
}
