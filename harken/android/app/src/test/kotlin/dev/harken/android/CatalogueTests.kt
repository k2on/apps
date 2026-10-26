// What a catalogue's pages say, held to what the desktop's say
// (`harken/iced/src/main.rs`'s `known` and `listed_works`, `view.rs`): a
// composer's best known works and the one set apart, the lines under a work
// and a performance, everybody on a recording, an album's runs of one work —
// and the same read through the real kernel: a seeded library's composers,
// works and performances, the most complete first, and a work's Play reading
// its most complete performance in the order the work goes.
package dev.harken.android

import dev.harken.android.gen.AddSong
import dev.harken.android.gen.CreditsEntry
import dev.harken.android.gen.RecordingsEntry
import dev.harken.android.gen.WorkSummary
import dev.harken.android.gen.Works
import dev.harken.android.onboarding.ArkSession
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CatalogueTests {
    private fun work(title: String, recordings: Long, tracks: Long, catalogue: String = "") =
        WorkSummary("", catalogue, "Johann Sebastian Bach", "", "", "bach/" + title.lowercase(), "", recordings, title, tracks)

    private fun take(performers: String, recorded: Long = 0, tracks: Long = 3, ms: Long = 0, licence: String = "") =
        RecordingsEntry("", ms, "w@" + performers.lowercase(), "", licence, performers, recorded, tracks)

    /** The works view's order is the catalogue's; known reorders by recordings, then tracks, and keeps it between equals. */
    @Test
    fun theBestKnownAreTheMostRecordedThenTheLongest() {
        val works = listOf(work("A", 1, 4), work("B", 3, 2), work("C", 3, 9), work("D", 1, 4), work("E", 0, 30))
        assertEquals(listOf("C", "B", "A", "D", "E"), Catalogue.known(works).map { it.title })
    }

    /** Six of many, best known first; every one by catalogue when few, or when "See All" was pressed. */
    @Test
    fun aComposerListsSixUntilSeeAll() {
        val many = (1..9).map { work("W$it", it.toLong() % 4, it.toLong()) }
        val listed = Catalogue.listed(many, all = false)
        assertEquals(Catalogue.KNOWN_WORKS, listed.size)
        assertEquals(Catalogue.known(many).take(6), listed)
        assertTrue(Catalogue.short(listed, many))
        assertEquals(many, Catalogue.listed(many, all = true))
        val few = many.take(6)
        assertEquals("few are listed by catalogue", few, Catalogue.listed(few, all = false))
        assertTrue(!Catalogue.short(Catalogue.listed(few, all = false), few))
    }

    /** One work is set apart only when it is recorded strictly more than the next. */
    @Test
    fun theMostRecordedIsSetApartOnlyWithoutATie() {
        assertEquals("B", Catalogue.mostRecorded(listOf(work("A", 2, 9), work("B", 3, 1)))?.title)
        assertNull("a tie", Catalogue.mostRecorded(listOf(work("A", 2, 9), work("B", 2, 1))))
        assertNull("one work", Catalogue.mostRecorded(listOf(work("A", 5, 9))))
        assertNull("none", Catalogue.mostRecorded(emptyList()))
        assertEquals("BWV 988 · 3 recordings", Catalogue.recordedFacts(work("Goldberg", 3, 30, "BWV 988")))
        assertEquals("1 recording", Catalogue.recordedFacts(work("X", 1, 1)))
    }

    /**
     * A face without a picture says the first and last words' initials, as the desktop's does
     * (`arkui::account::Who::initials`). Falsified by taking the second word's: `JS`.
     */
    @Test
    fun aFaceSaysWhoByItsInitials() {
        assertEquals("A", Who.initials("Ada"))
        assertEquals("JB", Who.initials("Johann Sebastian Bach"))
        assertEquals("AL", Who.initials("  ada   lovelace "))
        assertEquals("U", Who.initials("user-0123"))
        assertEquals("", Who.initials(""))
        assertEquals("\u00c9", Who.initials("\u00e9mile (\u2014)"))
    }

    @Test
    fun aCatalogueWritesWhoAndWhen() {
        assertEquals("J. S. Bach", Catalogue.initials("Johann Sebastian Bach"))
        assertEquals("L. van Beethoven", Catalogue.initials("Ludwig van Beethoven"))
        assertEquals("Vivaldi", Catalogue.initials("Vivaldi"))
        assertEquals("", Catalogue.initials(""))
        assertEquals("1685–1750", Catalogue.span(1685, 1750))
        assertEquals("1685–", Catalogue.span(1685, 0))
        assertEquals("–1750", Catalogue.span(0, 1750))
        assertEquals("", Catalogue.span(0, 0))
    }

    /** A performance's line: year, tracks, length, licence — each only when known; and nobody named is said. */
    @Test
    fun aPerformanceSaysWhenHowMuchAndHowLong() {
        assertEquals("2016 · 3 tracks · 22 min", Catalogue.takeFacts(take("Kimiko Ishizaka", 2016, 3, 22 * 60_000)))
        assertEquals("1 track", Catalogue.takeFacts(take("X", tracks = 1)))
        assertEquals("4 tracks · CC BY 3.0", Catalogue.takeFacts(take("X", tracks = 4, licence = "CC BY 3.0")))
        assertEquals("Performer not named", Catalogue.who(take("")))
        assertEquals("Most Complete", Catalogue.featuredLabel(2))
        assertEquals("The Recording", Catalogue.featuredLabel(1))
        assertEquals("bach/bwv-988", Catalogue.workOf("bach/bwv-988@kimiko-ishizaka"))
        assertNull(Catalogue.workOf("water-music/air"))
        assertEquals("Goldberg, BWV 988", Catalogue.recordingTitle(work("Goldberg", 1, 1, "BWV 988"), "who"))
        assertEquals("who", Catalogue.recordingTitle(null, "who"))
    }

    /** The composer first, then the credits in billing order, each once with every role they had. */
    @Test
    fun everybodyOnARecordingOnce() {
        val credits = listOf(CreditsEntry("", "Busch Chamber Players", 0, "performer"), CreditsEntry("violin", "Adolf Busch", 1, "performer"), CreditsEntry("", "Adolf Busch", 2, "conductor"))
        assertEquals(
            listOf("Johann Sebastian Bach" to "Composer", "Busch Chamber Players" to "performer", "Adolf Busch" to "performer, violin, conductor"),
            Catalogue.people("Johann Sebastian Bach", credits),
        )
        assertEquals(emptyList<Pair<String, String>>(), Catalogue.people("", emptyList()))
    }

    /** An album's tracks fall into runs of one work, a song's run with no heading; each run says how long it lasts. */
    @Test
    fun anAlbumIsRunsOfOneWork() {
        val water = Demo.album("Water Music")
        val runs = Catalogue.runs(water) { t -> Demo.details[t.id]?.let { it.copy(composer = t.creator, work = it.part) } }
        assertEquals(listOf("Suite No. 2 in D major", "Suite No. 3 in G major", "Suite No. 1 in F major"), runs.map { it.work })
        assertEquals(water.size, runs.last().until)
        assertEquals(water.subList(0, 4).sumOf { it.durationMs }, runs[0].ms)
        assertEquals("one work, its suites its parts", listOf("Water Music"), Catalogue.runs(water) { Demo.details[it.id] }.map { it.work })
        val songs = Catalogue.runs(Demo.album("Goldberg Variations, BWV 988")) { Demo.details[it.id] }
        assertEquals("songs are one run, of no work", listOf(""), songs.map { it.work })
    }

    /** Who played a record, as its page says it; nobody credited, and it is whose it is. */
    @Test
    fun anAlbumIsByWhoPlayedIt() {
        val by = Album("Goldberg", "Johann Sebastian Bach", "", 10, performer = "Kimiko Ishizaka", released = 2016)
        assertEquals("Kimiko Ishizaka" to "JOHANN SEBASTIAN BACH · 2016", Catalogue.albumBy(by, by.creator))
        assertEquals("Kimiko Ishizaka", by.by)
        val pop = Album("Songs", "A Band", "", 3, performer = "A Band")
        assertEquals("a pop record does not name its artist twice", "A Band" to "", Catalogue.albumBy(pop, pop.creator))
        assertEquals("Nobody" to "", Catalogue.albumBy(null, "Nobody"))
        assertEquals("Bach", Album("X", "Bach", "", 1).by)
    }

    @Test
    fun aCataloguePageIsARouteAndAPlace() {
        for (p in listOf(Page.ForArtist("Johann Sebastian Bach"), Page.ForWork("johann-sebastian-bach/bwv-988"), Page.ForRecording("johann-sebastian-bach/bwv-988@kimiko-ishizaka"))) {
            assertEquals(p, Page.parse(p.route))
            assertEquals(p, Page.ofPlace(p.place(emptyList()), emptyList()))
        }
        // A composer's address, as the desktop wrote it, is their one page.
        assertEquals(Page.ForArtist("Johann Sebastian Bach"), Page.ofPlace("#composer/Johann%20Sebastian%20Bach", emptyList()))
        assertEquals("#work/johann-sebastian-bach%2Fbwv-988", Page.ForWork("johann-sebastian-bach/bwv-988").place(emptyList()))
    }

    // Through the kernel ----------------------------------------------------------------------

    private fun movement(composer: String, work: String, catalogue: String, no: Long, performer: String, album: String, ms: Long = 120_000) =
        AddSong(
            title = "$work: $no",
            artist = composer,
            album = album,
            durationMs = ms,
            file = "music/$album/$no.flac",
            track = no,
            part = "",
            catalogue = catalogue,
            performer = performer,
            bpm = 0,
            albumArt = "",
            artistArt = "",
            disc = 1,
            workTitle = work,
            movementNo = no,
            artistBanner = "",
        )

    /**
     * A library of Bach's: the Goldberg Variations three times — the fullest last, its movements out
     * of order — and two works once each. The composers, works and performances read as the pages
     * read them, and a work's Play reads its fullest performance in the order the work goes.
     */
    @Test
    fun aComposersPageReadsTheKernelsViews() {
        val bach = "Johann Sebastian Bach"
        val peer = alone()
        peer.addSong(movement(bach, "Goldberg Variations", "BWV 988", 1, "Glenn Gould", "Goldberg (1955)"))
        for (no in listOf(3L, 1L, 2L)) peer.addSong(movement(bach, "Goldberg Variations", "BWV 988", no, "Kimiko Ishizaka", "Open Goldberg"))
        for (no in listOf(1L, 2L)) peer.addSong(movement(bach, "Goldberg Variations", "BWV 988", no, "Angela Hewitt", "Goldberg (Hewitt)"))
        peer.addSong(movement(bach, "Cello Suite No. 1", "BWV 1007", 1, "Pablo Casals", "Cello Suites"))
        for (no in 1L..4L) peer.addSong(movement(bach, "Mass in B minor", "BWV 232", no, "Monteverdi Choir", "Mass"))
        val session = ArkSession.inline(peer, "https://music.example.com", testLogin("library", roles = listOf("library")))
        val library = Library(session)
        try {
            val deadline = System.currentTimeMillis() + 15_000
            while (!library.complete.value && System.currentTimeMillis() < deadline) Thread.sleep(5)
            assertTrue("the library hydrated", library.complete.value)
            val composer = library.composers.value.single { it.name == bach }
            assertEquals(3L, composer.works)

            val works = library.works(bach).value!!
            assertEquals(works, Phone.once({ peer.works(Works(bach)) }) { it })
            assertEquals(listOf("BWV 1007", "BWV 232", "BWV 988"), works.map { it.catalogue })
            val goldberg = Catalogue.mostRecorded(works)!!
            assertEquals("Goldberg Variations", goldberg.title)
            assertEquals(3L, goldberg.recordings)
            assertEquals(listOf("Goldberg Variations", "Mass in B minor", "Cello Suite No. 1"), Catalogue.known(works).map { it.title })

            val takes = library.recordings(goldberg.id).value!!
            assertEquals(listOf("Kimiko Ishizaka", "Angela Hewitt", "Glenn Gould"), takes.map { it.performers })
            assertEquals(listOf(3L, 2L, 1L), takes.map { it.tracks })
            assertEquals(360_000L, takes[0].durationMs)
            assertEquals("3 tracks · 6 min", Catalogue.takeFacts(takes[0]))

            // What a work's Play queues: its most complete performance, in the order the work goes.
            val played = runBlocking { library.read(Page.ForWork(goldberg.id)) }
            assertEquals(listOf(1L, 2L, 3L).map { "Goldberg Variations: $it" }, played.map { it.title })
            assertEquals(played, runBlocking { library.read(Page.ForRecording(takes[0].id)) })
            assertEquals(goldberg.id, Catalogue.workOf(takes[0].id))

            // An album says who played it.
            assertEquals("Kimiko Ishizaka", library.albums.value.single { it.name == "Open Goldberg" }.performer)

            library.release(worksKey(bach))
            library.release(recordingsKey(goldberg.id))
        } finally {
            library.close()
            session.close()
            peer.peer.close()
        }
    }
}
