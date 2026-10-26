// The podcasts and sermons, through the real kernel: a show and its episodes
// as the server's feeds author them, read as the Library reads them — the
// shows by title, a show's episodes newest first, which the songs leave out
// — and where a show's page is, as a route and as a place.
package dev.harken.android

import dev.harken.android.gen.AddEpisode
import dev.harken.android.gen.AddShow
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ShowsTests {
    private fun episode(show: String, title: String, at: Long) =
        AddEpisode(show, title, 1_800_000L, ".feeds/$title.mp3", title, at, "", "", "", 0, 0)

    /** Falsified by reading `library` for a show's episodes: the songs came back instead. */
    @Test
    fun aShowIsItsEpisodesNewestFirstAndNotMusic() {
        val h = alone()
        h.addSong(song(1))
        h.addShow(AddShow("sermon", "Gospel in Life", "Tim Keller", "Sermons.", "", "https://podcast.gospelinlife.com/feed.xml", ""))
        h.addShow(AddShow("podcast", "Full Time Nix", "", "", "", "", ""))
        h.addEpisode(episode("sermon/gospel-in-life", "Living Grace", 2))
        h.addEpisode(episode("sermon/gospel-in-life", "The Dream of the Kingdom", 3))
        h.addEpisode(episode("podcast/full-time-nix", "Bump", 1))

        val shows = Phone.once({ h.shows() }, Show::of)
        assertEquals(listOf("Full Time Nix", "Gospel in Life"), shows.map { it.title })
        assertEquals(listOf(1L, 2L), shows.map { it.episodes })
        val episodes = Phone.once({ Phone.show(h, "sermon/gospel-in-life") }, Track::of)
        assertEquals(listOf("The Dream of the Kingdom", "Living Grace"), episodes.map { it.title })
        assertTrue(episodes.all { it.kind == "sermon" && it.creator == "Gospel in Life" })
        assertEquals(listOf("song"), Phone.once({ Phone.library(h, null) }, Track::of).map { it.kind })
        assertEquals("Gospel in Life", Shows.of(episodes[0], shows)?.title)
        assertNull("a song is of no show", Shows.of(Phone.once({ Phone.library(h, null) }, Track::of)[0], shows))
    }

    @Test
    fun aShowsPageIsARouteAndAPlace() {
        val page = Page.ForShow("podcast/full-time-nix")
        assertEquals(page, Page.parse(page.route))
        assertEquals("#show/podcast%2Ffull-time-nix", page.place(emptyList()))
        assertEquals(page, Page.ofPlace(page.place(emptyList()), emptyList()))
    }

    /** The Library offers Podcasts and Sermons only where there is a show of the kind. */
    @Test
    fun theLibraryOffersOnlyTheShowsThereAre() {
        val none = LibraryData(true, emptyList(), emptyList(), emptyList(), emptyList(), null)
        assertTrue(!offers(LibraryTab.Podcasts, none) && !offers(LibraryTab.Sermons, none))
        val one = none.copy(shows = listOf(Show("sermon/x", "sermon", "X", "", "", "", 1)))
        assertTrue(!offers(LibraryTab.Podcasts, one) && offers(LibraryTab.Sermons, one))
    }
}
