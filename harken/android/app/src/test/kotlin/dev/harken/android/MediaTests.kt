// The two things about a track's bytes a phone could get wrong without
// anybody noticing until a song would not play: where they are (`Media.url`,
// the desktop's `media_url`, each segment percent-encoded), and how a cover
// is shrunk before it is decoded (`Covers.sampleSize`). The derived square's
// hash is `ark-ui`'s, and pinned there (`ArtTests`).
package dev.harken.android

import org.junit.Assert.assertEquals
import org.junit.Test

class MediaTests {
    @Test
    fun aTracksFileIsJoinedToItsServer() {
        assertEquals("https://h/media/music/Bach/air.flac", Media.url("https://h", "music/Bach/air.flac"))
        assertEquals("https://h/media/music/Bach/air.flac", Media.url("https://h/", "music/Bach/air.flac"))
        // Every segment encoded: a space, a `#` that would end the path, a `?`, and UTF-8.
        assertEquals(
            "http://10.0.2.2:8787/media/music/Water%20Music/No.%201%20%23%3F/Bourr%C3%A9e.mp3",
            Media.url("http://10.0.2.2:8787", "music/Water Music/No. 1 #?/Bourrée.mp3"))
        // A whole URL is already an answer.
        assertEquals("https://upload.example/a.mp3", Media.url("https://h", "https://upload.example/a.mp3"))
        assertEquals("", Media.url("https://h", ""))
    }

    @Test
    fun lengthsAsTheyAreSaid() {
        assertEquals("3:49", Media.clock(229_000))
        assertEquals("1:02:03", Media.clock(3_723_000))
        assertEquals("48 min", Media.spell(48 * 60_000L))
        assertEquals("2 hr 30 min", Media.spell(150 * 60_000L))
        assertEquals("1 hr", Media.spell(60 * 60_000L))
        assertEquals("1 song", Media.count(1, "song"))
        assertEquals("12 songs", Media.count(12, "song"))
    }

    @Test
    fun aCoverIsShrunkByPowersOfTwoAndNeverBelowWhatIsWanted() {
        assertEquals(1, Covers.sampleSize(500, 600))
        assertEquals(1, Covers.sampleSize(1000, 600))
        assertEquals(2, Covers.sampleSize(1200, 600))
        assertEquals(4, Covers.sampleSize(3000, 600))
    }
}
