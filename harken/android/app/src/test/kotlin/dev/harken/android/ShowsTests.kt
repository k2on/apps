// The podcasts and sermons, through the real kernel: a show and its episodes
// as the server's feeds author them, read as the Library reads them — the
// shows by title, a show's episodes newest first, which the songs leave out
// — and where a show's page is, as a route and as a place; a feed followed
// from the phone, and an episode whose file is not on the server yet.
package dev.harken.android

import dev.harken.android.gen.AddEpisode
import dev.harken.android.gen.AddShow
import dev.harken.android.gen.DownloadProgress
import dev.harken.android.gen.Downloaded
import dev.harken.android.gen.FeedRead
import dev.harken.android.gen.QueueDownload
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

    /**
     * A feed followed from the phone is a subscription keeping twenty, its show's once the server
     * has read it, with older episodes to fetch; fetched, it keeps forty. Falsified by `hasOlder`
     * reading `episodes` for `items`: there were none to fetch.
     */
    @Test
    fun aFeedIsFollowedAndItsOlderEpisodesFetched() {
        val h = alone()
        val url = "https://feeds.transistor.fm/full-time-nix"
        assertTrue(Phone.subscribe(h, "podcast", url) is Wrote.Entry)
        assertTrue("a feed is a URL", Phone.subscribe(h, "podcast", "feed.xml") is Wrote.Refused)
        h.addShow(AddShow("podcast", "Full Time Nix", "", "", "", url, ""))
        h.feedRead(FeedRead(url, "Full Time Nix", 50, ""))
        val feed = Phone.once({ h.subscriptions() }, Subscription::of).single()
        assertEquals(Triple("podcast/full-time-nix", 20L, 50L), Triple(feed.showId, feed.keep, feed.items))
        assertTrue(feed.hasOlder)
        Phone.fetchOlder(h, url, 20)
        assertEquals(40L, Phone.once({ h.subscriptions() }, Subscription::of).single().keep)
        Phone.unsubscribe(h, url)
        assertTrue(Phone.once({ h.subscriptions() }, Subscription::of).isEmpty())
        assertTrue("the show went with it", Phone.once({ h.shows() }, Show::of).isEmpty())
    }

    /**
     * An episode is listed before its file is on the server: queued, then how far, then gone from
     * the downloads. Falsified by `short` dividing the wrong way: it said 400%.
     */
    @Test
    fun anEpisodeIsListedBeforeItsFileIsHere() {
        val h = alone()
        h.addShow(AddShow("podcast", "Full Time Nix", "", "", "", "", ""))
        h.addEpisode(episode("podcast/full-time-nix", "Bump", 1))
        val file = ".feeds/Bump.mp3"
        h.queueDownload(QueueDownload(file, "https://media/bump.mp3"))
        val bump = Phone.once({ Phone.show(h, "podcast/full-time-nix") }, Track::of).single()
        fun download() = Phone.once({ h.downloads() }, Download::of).singleOrNull()
        assertEquals(bump.id, download()?.mediaId)
        assertEquals("queued" to "Not on the server yet", download()?.let { it.short to it.said })
        h.downloadProgress(DownloadProgress(file, "downloading", 25, 100, ""))
        assertEquals("25%", download()?.short)
        h.downloaded(Downloaded(file))
        assertNull(download())
    }
}
