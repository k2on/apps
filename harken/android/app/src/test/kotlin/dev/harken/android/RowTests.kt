// What a row asks for as it enters a list, held to what it asked for before
// the asking was made cheap: the album a song is on, found by name in a map
// rather than by a scan of every album, gives the same album for every song
// of a seeded library — its own row, the made-up one while the albums have
// not caught up, and none for a song on no album; and an id's hex is spelt
// once and is the same spelling.
package dev.harken.android

import dev.arkdb.Hex
import dev.arkdb.Id
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
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class RowTests {
    private fun harken(): ByteArray {
        var dir: File? = File("").absoluteFile
        while (dir != null) {
            File(dir, "domain/harken.ark").takeIf { it.isFile }?.let { return it.readBytes() }
            File(dir, "harken/domain/harken.ark").takeIf { it.isFile }?.let { return it.readBytes() }
            dir = dir.parentFile
        }
        error("no harken.ark above ${File("").absolutePath}")
    }

    // Seven albums of six songs, and every seventh song on none.
    private fun song(i: Int): Map<String, Value> =
        mapOf(
            "title" to Value.text("Song $i"),
            "artist" to Value.text("Artist ${i / 10}"),
            "album" to Value.text(if (i % 7 == 6) "" else "Album ${i / 7}"),
            "duration_ms" to Value.int(180_000),
            "file" to Value.text("music/$i.flac"),
            "track" to Value.int((i % 7 + 1).toLong()),
            "part" to Value.text(""),
            "catalogue" to Value.text(""),
            "performer" to Value.text(""),
            "bpm" to Value.int(0),
            "album_art" to Value.text("covers/${i / 7}.jpg"),
            "artist_art" to Value.text(""),
            "disc" to Value.int(1),
            "work_title" to Value.text(""),
            "movement_no" to Value.int(0),
        )

    /** What `albumOf` was: a scan of every album, per song. */
    private fun scanned(t: Track, details: Map<Id, TrackDetail>, albums: List<Album>): Album? =
        details[t.id]?.album?.takeIf { it.isNotEmpty() }?.let { name -> albums.firstOrNull { it.name == name } ?: Album(name, t.creator, "", 0) }

    @Test
    fun theAlbumByNameIsTheAlbumTheScanFound() {
        val peer = Peer.open(Domain.fromBytes(harken()), MemoryStorage(), Options.alone("library").copy(autos = Autos.Seeded(9)))
        peer.setRoles(listOf("library"))
        repeat(49) { peer.mutate("add_song", song(it)) }
        val session = ArkSession.inline(peer, "https://music.example.com", Login("t", "s", Account("library"), 0))
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
            // album's row has not been read: then the song's album is made of its name.
            for (shown in listOf(albums, albums.drop(1))) {
                val byName = shown.associateBy { it.name }
                for (t in tracks) assertEquals("the album of ${t.title}", scanned(t, details, shown), Album.ofTrack(t, details, byName))
            }
            assertEquals("songs on no album", 7, tracks.count { Album.ofTrack(it, details, albums.associateBy { a -> a.name }) == null })
            val gone = albums.first()
            val one = tracks.first { details[it.id]?.album == gone.name }
            assertEquals(Album(gone.name, one.creator, "", 0), Album.ofTrack(one, details, emptyMap()))
            assertNull(Album.ofTrack(one, emptyMap(), albums.associateBy { it.name }))
        } finally {
            library.close()
            session.close()
            peer.close()
        }
    }

    @Test
    fun anIdsHexIsSpeltOnce() {
        val bytes = ByteArray(16) { (it * 17 + 3).toByte() }
        val id = Id(bytes)
        val hex = id.hex
        assertEquals(Hex.encode(bytes), hex)
        // Kept: the same string the second time, not one equal to it.
        assertSame(hex, id.hex)
        assertEquals("03142536-4758-697a-8b9c-adbecfe0f102", id.text)
        assertEquals(hex, id.text.replace("-", ""))
        assertEquals(id, Id.ofHex(hex))
        assertEquals(id, Id.ofText(id.text))
    }
}
