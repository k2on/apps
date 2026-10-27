// The podcasts and sermons, through the real kernel: a show and its episodes
// as the server's feeds author them, read as the Library reads them — the
// shows by title, a show's episodes newest first, which the songs leave out
// — and where a show's page is, as a route and as a place; a feed found
// in the directory or pasted, and followed from a shows page's +; an
// episode whose file is not on the server yet; what an episode's row
// says — when, what about, and how much is left; who is on an episode, and
// the episodes a guest is on; and an episode's transcript, and which of its
// lines is being said.
package dev.harken.android

import dev.harken.android.gen.AddEpisode
import dev.harken.android.gen.AddShow
import dev.harken.android.gen.ChapterAt
import dev.harken.android.gen.CueAt
import dev.harken.android.gen.PersonOn
import dev.harken.android.gen.ReportProgress
import dev.harken.android.gen.SetChapters
import dev.harken.android.gen.SetPeople
import dev.harken.android.gen.SetTranscript
import dev.harken.android.gen.DownloadProgress
import dev.harken.android.gen.Downloaded
import dev.harken.android.gen.FeedRead
import dev.harken.android.gen.QueueDownload
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ShowsTests {
    private fun episode(show: String, title: String, at: Long, description: String = "") =
        AddEpisode(show, title, 1_800_000L, ".feeds/$title.mp3", title, at, description, "", "", 0, 0)

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

    /**
     * A feed's address pasted into the Podcasts page's sheet is the one row it lists; followed from
     * there, as a podcast, it is a subscription — its row says Following, and it is listed over the
     * podcasts until the server has read it — keeping twenty, its show's once read, with older
     * episodes to fetch; fetched, it keeps forty. Falsified by `hasOlder` reading `episodes` for
     * `items`: there were none to fetch.
     */
    @Test
    fun aFeedIsFollowedAndItsOlderEpisodesFetched() {
        val h = alone()
        val url = "https://feeds.transistor.fm/full-time-nix"
        val row = Directory.rows(" $url ", emptyList()).single()
        assertEquals(url, row.feed)
        assertTrue(Phone.subscribe(h, LibraryTab.Podcasts.kind!!, row.feed) is Wrote.Entry)
        assertTrue("a feed is a URL", Phone.subscribe(h, "podcast", "feed.xml") is Wrote.Refused)
        val followed = Phone.once({ h.subscriptions() }, Subscription::of)
        assertTrue("its Follow says Following", Directory.following(row, followed))
        assertEquals("not read yet, it is over the podcasts", listOf(url), unread("podcast", followed, emptyList()).map { it.url })
        assertTrue("and not over the sermons", unread("sermon", followed, emptyList()).isEmpty())
        h.addShow(AddShow("podcast", "Full Time Nix", "", "", "", url, ""))
        h.feedRead(FeedRead(url, "Full Time Nix", 50, ""))
        val feed = Phone.once({ h.subscriptions() }, Subscription::of).single()
        assertEquals(Triple("podcast/full-time-nix", 20L, 50L), Triple(feed.showId, feed.keep, feed.items))
        assertTrue("read, it is a show", unread("podcast", listOf(feed), Phone.once({ h.shows() }, Show::of)).isEmpty())
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

    /**
     * An episode begun is the listener's place in it, resumed a few seconds back and said as a share;
     * played, it starts again from the top; and its chapters come in order. Falsified by `resumeMs`
     * keeping the place of a played episode: it resumed at its end.
     */
    @Test
    fun anEpisodeKeepsItsListenersPlaceAndItsChapters() {
        val h = alone()
        h.addShow(AddShow("podcast", "Full Time Nix", "", "", "", "", ""))
        h.addEpisode(episode("podcast/full-time-nix", "Bump", 1))
        val bump = Phone.once({ Phone.show(h, "podcast/full-time-nix") }, Track::of).single()
        h.reportProgress(ReportProgress(bump.id, 900_000, 0, "Ada"))
        val mine = Phone.once({ h.myProgress() }, Listened::of).single()
        assertEquals(Triple(897_000L, "50%", "you 50%"), Triple(mine.resumeMs, mine.short, Listened.said(listOf(mine), mine.userId)))
        assertEquals("Ada 50%", Listened.said(listOf(mine), "somebody else"))
        assertTrue(Phone.markPlayed(h, bump.id, true) is Wrote.Entry)
        val played = Phone.once({ Phone.showProgress(h, "podcast/full-time-nix") }, Listened::of).single()
        assertEquals(Triple(true, 0L, "played"), Triple(played.finished, played.resumeMs, played.short))

        h.setChapters(SetChapters(".feeds/Bump.mp3", listOf(ChapterAt(65_000, "Flakes", "", ""), ChapterAt(0, "Intro", "", ""))))
        val chapters = Phone.once({ Phone.chapters(h, bump.id) }, Chapter::of)
        assertEquals(listOf("Intro", "Flakes"), chapters.map { it.title })
        assertEquals("Flakes", Chapter.at(chapters, 70_000)?.title)
        assertEquals("Intro", Chapter.at(chapters, 64_999)?.title)
    }

    /**
     * Who is on an episode is its feed's people, in the feed's order, and its row names whoever is
     * not a host; its transcript comes in the order it is said, each speaker's name where they start
     * speaking. Falsified by `guests` keeping the hosts: the row named the show's own host.
     */
    @Test
    fun anEpisodesPeopleAndTranscriptAreItsFeeds() {
        val h = alone()
        h.addShow(AddShow("podcast", "Full Time Nix", "", "", "", "", ""))
        h.addEpisode(episode("podcast/full-time-nix", "Bump", 1))
        val bump = Phone.once({ Phone.show(h, "podcast/full-time-nix") }, Track::of).single()
        h.setPeople(
            SetPeople(
                ".feeds/Bump.mp3",
                listOf(
                    PersonOn("Shahar \"Dawn\" Or", "Host", "", ""),
                    PersonOn("Jacek Galowicz", "Guest", "https://galowicz.de", ".feeds/jacek.jpg"),
                    PersonOn("Kiara Grouwstra", "Guest", "", ""),
                ),
            ),
        )
        val people = Phone.once({ Phone.people(h, bump.id) }, OnEpisode::of)
        assertEquals(listOf("Shahar \"Dawn\" Or", "Jacek Galowicz", "Kiara Grouwstra"), people.map { it.name })
        assertEquals(listOf(true, false, false), people.map { it.hosts })
        assertEquals(OnEpisode("Jacek Galowicz", "Guest", ".feeds/jacek.jpg", "https://galowicz.de"), people[1])
        assertEquals("Jacek Galowicz and Kiara Grouwstra", OnEpisode.named(OnEpisode.guests(people)))
        assertEquals("Ada, Grace and Edsger", OnEpisode.named(listOf("Ada", "Grace", "Edsger").map { OnEpisode(it, "Guest", "", "") }))

        h.setTranscript(
            SetTranscript(
                ".feeds/Bump.mp3",
                listOf(
                    CueAt(4_000, 6_000, "Jacek", "Thanks for having me."),
                    CueAt(0, 2_000, "Shahar", "Welcome to Full Time Nix."),
                    CueAt(2_000, 4_000, "Shahar", "Today, Jacek."),
                    CueAt(6_000, 9_000, "", "Applause."),
                ),
            ),
        )
        val lines = Phone.once({ Phone.transcript(h, bump.id) }, Cue::of)
        assertEquals(listOf(0L, 2_000L, 4_000L, 6_000L), lines.map { it.startMs })
        assertEquals("Welcome to Full Time Nix.", lines[0].text)
        assertEquals(listOf("Shahar", "", "Jacek", ""), Cue.speakers(lines))
    }

    /**
     * The line a transcript lights is the last to have started — none before the first, the last
     * once it has started however long ago. Falsified by `at` taking the first line that ends after
     * the position: across a gap between two lines it lit the next before it was said.
     */
    @Test
    fun theLineBeingSaidIsTheLastToHaveStarted() {
        val lines = listOf(Cue(1_000, 2_000, "", "One."), Cue(2_000, 3_000, "", "Two."), Cue(5_000, 6_000, "", "Three."))
        assertEquals(-1, Cue.at(lines, 999))
        assertEquals(0, Cue.at(lines, 1_000))
        assertEquals(1, Cue.at(lines, 2_000))
        assertEquals("said until the next starts", 1, Cue.at(lines, 4_000))
        assertEquals(2, Cue.at(lines, 600_000))
        assertEquals(-1, Cue.at(emptyList(), 0))
    }

    /**
     * A guest's page is every episode they are on, newest first, from any show, and nothing of a
     * musician's; the newest names them as the feed does, picture and link. Falsified by reading
     * `artist` for their page: a guest made no songs, so it was empty.
     */
    @Test
    fun aGuestsPageIsEveryEpisodeTheyAreOn() {
        val h = alone()
        h.addSong(song(1))
        h.addShow(AddShow("podcast", "Full Time Nix", "", "", "", "", ""))
        h.addShow(AddShow("podcast", "Nix Freaks", "", "", "", "", ""))
        h.addEpisode(episode("podcast/full-time-nix", "Bump", 1))
        h.addEpisode(episode("podcast/nix-freaks", "Fifty One", 2))
        h.addEpisode(episode("podcast/full-time-nix", "Unrelated", 3))
        val jacek = PersonOn("Jacek Galowicz", "Guest", "https://galowicz.de", ".feeds/jacek.jpg")
        h.setPeople(SetPeople(".feeds/Bump.mp3", listOf(PersonOn("Shahar \"Dawn\" Or", "Host", "", ""), jacek)))
        h.setPeople(SetPeople(".feeds/Fifty One.mp3", listOf(jacek)))
        h.setPeople(SetPeople(".feeds/Unrelated.mp3", listOf(PersonOn("Shahar \"Dawn\" Or", "Host", "", ""))))

        val on = Phone.once({ Phone.appearances(h, "Jacek Galowicz") }, Track::of)
        assertEquals(listOf("Fifty One", "Bump"), on.map { it.title })
        assertEquals(listOf("Nix Freaks", "Full Time Nix"), on.map { it.creator })
        assertTrue("he made no songs", Phone.once({ Phone.artist(h, "Jacek Galowicz") }, Track::of).isEmpty())
        val him = Phone.once({ Phone.people(h, on.first().id) }, OnEpisode::of).single { it.name == "Jacek Galowicz" }
        assertEquals(".feeds/jacek.jpg" to "https://galowicz.de", him.art to him.link)
        assertTrue("a musician is on no episode", Phone.once({ Phone.appearances(h, song(1).artist) }, Track::of).isEmpty())
    }

    /**
     * The directory's answer, as it spells it: a show with no feed cannot be followed, so it is not
     * listed, and one with no large picture keeps its small one; an address is followed as it is,
     * words are searched for, escaped. Falsified by listing every result: the one with no `feedUrl`
     * was there.
     */
    @Test
    fun aSearchListsTheShowsWithFeeds() {
        val json =
            """{"resultCount":3,"results":[
              {"kind":"podcast","collectionName":"Full Time Nix","artistName":"Shahar \"Dawn\" Or",
               "feedUrl":"https://feeds.transistor.fm/full-time-nix",
               "artworkUrl100":"https://a/100x100bb.jpg","artworkUrl600":"https://a/600x600bb.jpg"},
              {"kind":"podcast","collectionName":"No Feed","artistName":"Somebody"},
              {"kind":"podcast","collectionName":"Small","feedUrl":"https://s/feed.xml","artworkUrl100":"https://s/100x100bb.jpg"}]}"""
        assertEquals(
            listOf(
                Directory.Found("Full Time Nix", "Shahar \"Dawn\" Or", "https://feeds.transistor.fm/full-time-nix", "https://a/600x600bb.jpg"),
                Directory.Found("Small", "", "https://s/feed.xml", "https://s/100x100bb.jpg"),
            ),
            Directory.parse(json).getOrThrow(),
        )
        assertTrue(Directory.parse("<html>").isFailure)
        assertTrue(Directory.isFeed(" https://feeds.transistor.fm/full-time-nix") && !Directory.isFeed("full time nix"))
        assertTrue(Directory.searchUrl("gospel & life").endsWith("&term=gospel%20%26%20life"))
        val found = Directory.parse(json).getOrThrow()
        assertEquals("words list what was found", found, Directory.rows("nix", found))
    }

    /**
     * An episode's row: what it is about and when it came out, in the order the show lists them;
     * its length, then once begun the time left and a bar a third along, then Played and no bar;
     * and how its download goes while its file is not on the server. Falsified by `left` counting
     * from the start rather than what is left: it said 10 min left a third of the way in.
     */
    @Test
    fun anEpisodesRowSaysWhenWhatAboutAndHowMuchIsLeft() {
        val h = alone()
        h.addShow(AddShow("podcast", "Full Time Nix", "", "", "", "", ""))
        h.addEpisode(episode("podcast/full-time-nix", "Bump", 1_000, "Flakes, and why they are worth it."))
        h.addEpisode(episode("podcast/full-time-nix", "Nix on macOS", 2_000))
        val shown = Phone.once({ Phone.show(h, "podcast/full-time-nix") }, Track::of)
        val details = Phone.once({ Phone.episodeDetails(h, "podcast/full-time-nix") }, EpisodeDetail::of)
        assertEquals(shown.map { it.id }, details.map { it.mediaId })
        assertEquals(listOf("", "Flakes, and why they are worth it."), details.map { it.description })
        assertEquals(listOf(2_000L, 1_000L), details.map { it.publishedMs })

        val bump = shown.single { it.title == "Bump" }
        assertEquals("30 min", Episodes.left(bump, null, null))
        assertNull(Episodes.progress(bump, null))
        h.reportProgress(ReportProgress(bump.id, 600_000, 0, "Ada"))
        val begun = Phone.once({ h.myProgress() }, Listened::of).single()
        assertEquals("20 min left", Episodes.left(bump, begun, null))
        assertEquals(1f / 3, Episodes.progress(bump, begun)!!, 0.001f)
        assertTrue(Phone.markPlayed(h, bump.id, true) is Wrote.Entry)
        val played = Phone.once({ h.myProgress() }, Listened::of).single()
        assertEquals("Played", Episodes.left(bump, played, null))
        assertNull(Episodes.progress(bump, played))
        assertEquals("queued", Episodes.left(bump, null, Download(bump.id, "queued", 0, 0, "")))
    }

    /** When an episode came out, as its row says it: today, a few days ago, a day, a day of another year. */
    @Test
    fun anEpisodeIsDatedAsAPodcastAppDatesIt() {
        val utc = java.time.ZoneOffset.UTC
        fun on(y: Int, m: Int, d: Int, hour: Int = 9): Long = java.time.ZonedDateTime.of(y, m, d, hour, 0, 0, 0, utc).toInstant().toEpochMilli()
        val now = on(2026, 10, 27, 12)
        assertEquals("TODAY", Episodes.dated(on(2026, 10, 27, 1), now, utc))
        assertEquals("YESTERDAY", Episodes.dated(on(2026, 10, 26, 23), now, utc))
        assertEquals("2 DAYS AGO", Episodes.dated(on(2026, 10, 25), now, utc))
        assertEquals("OCT 20", Episodes.dated(on(2026, 10, 20), now, utc))
        assertEquals("DEC 31, 2025", Episodes.dated(on(2025, 12, 31), now, utc))
        assertEquals("nobody said", "", Episodes.dated(0, now, utc))
    }
}
