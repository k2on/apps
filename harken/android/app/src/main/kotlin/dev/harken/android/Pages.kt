// The pages a library opens, laid out as a catalogue's (the desktop's,
// `harken/iced/src/view.rs`): an album, its tracks under the works they are
// movements of; a person — one page whether they made songs or wrote works
// — under their banner, their most recorded work set apart, their best
// known works, their records, their songs where they wrote no works, and
// the episodes they are on; a
// work and its performances; a performance, its
// movements, its records and everybody on it; and a playlist, in the order
// it was made. The lists are views held while the page is shown
// (`Library.hold`, `Library.works` and the rest), spliced as the log moves.
package dev.harken.android

import uniffi.harken_domain.Playlist
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.arkdb.ffi.Id
import dev.harken.android.gen.RecordingsEntry
import dev.harken.android.gen.WorkSummary
import dev.harken.android.ui.Cover
import dev.harken.android.ui.CoverCard
import dev.harken.android.ui.Empty
import dev.harken.android.ui.Glyph
import dev.harken.android.ui.MoreButton
import dev.harken.android.ui.PageBar
import dev.harken.android.ui.Recompositions
import dev.harken.android.ui.Shelf
import dev.harken.android.ui.Space
import dev.harken.android.ui.UnderStatusBar
import dev.harken.android.ui.quiet
import dev.harken.android.ui.rememberDestinationListState
import dev.harken.android.ui.rememberDestinationScrollState

private fun facts(vararg parts: String?): String = parts.filterNotNull().filter { it.isNotEmpty() }.joinToString(" · ")

private fun length(tracks: List<Track>): String? = tracks.sumOf { it.durationMs }.takeIf { it > 0 }?.let(Media::spell)

/**
 * A record's tracks under the works they are movements of, as a catalogue lists them: each run of
 * one work under its heading — its composer over its title, its length at its right — and each
 * movement over who played it; a song (of no work) keeps its number, or what `leading` puts there.
 * `creator` is whose the page is: a track by somebody else says whose it is.
 */
private fun LazyListScope.underWorks(
    tracks: List<Track>,
    details: Map<Id, TrackDetail>,
    creator: String,
    playing: String?,
    page: Page,
    act: Do,
    under: (Track, TrackDetail?) -> String = { t, d -> songUnder(t, d, creator) },
    art: Boolean = false,
    leading: @Composable (Track, TrackDetail?) -> Unit = { t, d -> TrackNumber(if (d?.work.isNullOrEmpty()) d?.track ?: 0 else 0, t.id.hex == playing) },
) {
    val runs = Catalogue.runs(tracks) { details[it.id] }
    for ((n, run) in runs.withIndex()) {
        if (run.work.isNotEmpty()) item(key = "work:${run.from}") { WorkHeading(run.composer, run.work, run.ms, first = n == 0) }
        // Under a work in parts — a suite's, an oratorio's — each part where it starts, as the phone
        // has always set them: a heading over one part alone would say its name twice.
        val inRun = tracks.subList(run.from, run.until)
        val parts = inRun.map { details[it.id]?.part.orEmpty() }
        val parted = parts.distinct().size > 1
        itemsIndexed(inRun, key = { _, t -> t.id.hex }) { i, t ->
            val d = details[t.id]
            if (parted && parts[i].isNotEmpty() && (i == 0 || parts[i] != parts[i - 1])) {
                Text(
                    parts[i],
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.padding(start = Space.page, end = Space.page, top = Space.m, bottom = Space.xs),
                )
            }
            TrackRow(
                t,
                under = under(t, d),
                playing = t.id.hex == playing,
                onClick = { act.play(tracks, tracks.indexOf(t), page) },
                onMore = { act.more(t, page) },
                art = art,
            ) { leading(t, d) }
        }
    }
}

// Under a track: who played it — or, on a record of several people's songs, whose this one is.
private fun songUnder(t: Track, d: TrackDetail?, creator: String): String =
    when {
        !d?.performer.isNullOrEmpty() -> d!!.performer
        t.creator != creator -> t.creator
        else -> ""
    }

/**
 * An album's page, laid out as a catalogue lays one out: the cover beside its title and who played
 * it, then its tracks under the works they are movements of, and at the foot when it came out, how
 * long it runs and who put it out. A record of no works (a pop record) is the same page with no
 * work headings, and its track numbers instead.
 */
@Composable
fun AlbumPage(name: String, album: Album?, tracks: List<Track>, details: Map<Id, TrackDetail>, playing: String?, act: Do) {
    Recompositions.hit("AlbumPage")
    val page = Page.ForAlbum(name)
    val creator = album?.creator ?: tracks.firstOrNull()?.creator ?: ""
    val (by, small) = Catalogue.albumBy(album, creator)
    LazyColumn(Modifier.fillMaxSize(), state = rememberDestinationListState("album")) {
        item { PageBar("", onBack = act::back) }
        item {
            CoverHeader(
                seed = name,
                art = album?.art ?: "",
                title = name,
                over = "Album",
                by = by,
                small = small,
                onSmall = if (creator.isNotEmpty()) ({ act.open(Page.ForArtist(creator)) }) else null,
                canPlay = tracks.isNotEmpty(),
                onPlay = { act.play(tracks, 0, page) },
                onShuffle = { act.shuffle(tracks, page) },
            )
        }
        underWorks(tracks, details, creator, playing, page, act)
        item { Foot(tracks, album?.released ?: 0, album?.label ?: "", Catalogue.licences(tracks.map { details[it.id] })) }
        item { Spacer(Modifier.height(Space.xl)) }
    }
}

/**
 * A person's page, one for everybody — whoever made a song and whoever wrote a work: their banner
 * (or, for somebody with no wide picture, their portrait beside their name), what they are and
 * when they lived; their most recorded work set apart with a Play, when one is; their best known
 * works and a "See All" that lists every one by catalogue (`all`); the records that are theirs;
 * for somebody who wrote no works, their songs as an album lists them; and the episodes they are on
 * (`appearances`), newest first, played as a show's are. Each section only when it has something in
 * it. Somebody only ever on episodes — a podcast's guest — is what the newest of them called them,
 * under their picture there (`person`), with their own page on the web where they have one.
 */
@Composable
fun ArtistPage(
    name: String,
    artist: Artist?,
    tracks: List<Track>,
    albums: List<Album>,
    albumOf: (Track) -> Album?,
    playing: String?,
    act: Do,
    details: Map<Id, TrackDetail> = emptyMap(),
    composer: Composer? = null,
    works: List<WorkSummary> = emptyList(),
    all: Boolean = false,
    onAll: (Boolean) -> Unit = {},
    appearances: List<Track> = emptyList(),
    person: OnEpisode? = null,
    mine: Map<Id, Listened> = emptyMap(),
) {
    Recompositions.hit("ArtistPage")
    val page = Page.ForArtist(name)
    val counts =
        facts(
            if (albums.isEmpty()) null else Media.count(albums.size, "album"),
            if (tracks.isEmpty() && appearances.isNotEmpty()) null else Media.count(tracks.size, "song"),
            length(tracks),
            if (appearances.isEmpty()) null else Media.count(appearances.size, "episode"),
        )
    // What they are and when they lived, as a catalogue heads a person; nobody's years known, and
    // it is how much of them there is here.
    val life = artist?.let { Catalogue.span(it.born, it.died) }?.takeIf { it.isNotEmpty() } ?: composer?.let { Catalogue.span(it.born, it.died) } ?: ""
    val kind =
        when {
            works.isNotEmpty() -> "Composer"
            tracks.isEmpty() && appearances.isNotEmpty() -> person?.role?.ifEmpty { null } ?: "Guest"
            else -> "Artist"
        }
    // What their header's Play plays: their songs, or else the episodes they are on.
    val plays = tracks.ifEmpty { appearances }
    val listed = Catalogue.listed(works, all)
    val short = Catalogue.short(listed, works)
    val best = Catalogue.mostRecorded(works)
    val by = Catalogue.initials(name)
    // Their wide picture across the page, or else their portrait cropped to it: a person's page
    // always opens under them; only somebody with no picture at all has the round header.
    val banner = listOf(artist?.banner, composer?.banner, artist?.art, composer?.art).firstOrNull { !it.isNullOrEmpty() }.orEmpty()
    val list = rememberDestinationListState("artist")
    val bar = WindowInsets.statusBars
    val density = LocalDensity.current
    // Under the status bar, up to the top of the screen; the bar over the picture until the banner,
    // the first item, has gone up past it.
    if (banner.isNotEmpty()) {
        UnderStatusBar {
            val first = list.layoutInfo.visibleItemsInfo.firstOrNull()
            first != null && first.index == 0 && first.offset + first.size > bar.getTop(density)
        }
    }
    LazyColumn(Modifier.fillMaxSize(), state = list) {
        if (banner.isNotEmpty()) {
            item { Banner(name, banner, facts(kind, life.ifEmpty { counts }), name, onBack = act::back) }
            if (works.isEmpty() && tracks.isNotEmpty()) {
                item {
                    Row(Modifier.padding(start = Space.page, end = Space.page, top = Space.l), verticalAlignment = Alignment.CenterVertically) {
                        PlayPill(onPlay = { act.play(tracks, 0, page) })
                        IconButton(onClick = { act.shuffle(tracks, page) }) { Glyph(Glyphs.Shuffle, "Shuffle", size = 20.dp) }
                    }
                }
            }
        } else {
            item { PageBar("", onBack = act::back) }
            item {
                CoverHeader(
                    seed = name,
                    art = artist?.art?.ifEmpty { null } ?: composer?.art?.ifEmpty { null } ?: person?.art.orEmpty(),
                    title = name,
                    over = facts(kind, life.ifEmpty { counts }),
                    round = true,
                    canPlay = plays.isNotEmpty(),
                    onPlay = { act.play(plays, 0, page) },
                    onShuffle = { act.shuffle(plays, page) },
                )
            }
        }
        // Their own page on the web, as the feed gives it.
        val link = person?.link.orEmpty()
        if (link.isNotEmpty()) item(key = "link") { WebLink(link) }
        // The most recorded work, set apart with a Play — when one is.
        if (best != null) {
            item(key = "featured") {
                Featured("Most Recorded", best.id, best.art, best.title, Catalogue.recordedFacts(best), onPlay = { act.playWork(best.id) }, onOpen = { act.open(Page.ForWork(best.id)) })
            }
        }
        if (works.isNotEmpty()) {
            item { SectionHeading(if (short) "Well-Known Works" else "Works") }
            // Each work under who it is by, its catalogue number under it, and how many performances
            // of it there are at its right.
            items(listed, key = { "work:" + it.id }) { w -> Entry(w.title, by, w.catalogue, w.recordings.toString(), onOpen = { act.open(Page.ForWork(w.id)) }) }
            if (short) item { MoreLine("See All ${works.size} Works") { onAll(true) } }
        }
        if (albums.isNotEmpty()) {
            item { SectionHeading("Albums") }
            item {
                Shelf("albums", albums, key = { it.name }) { a ->
                    CoverCard(a.name, a.art, a.name, Media.count(a.tracks.toInt(), "track"), 136.dp, onClick = { act.open(Page.ForAlbum(a.name)) }, opens = true)
                }
            }
        }
        // Somebody's songs, where they wrote no works: a composer's are their works' performances.
        if (works.isEmpty() && tracks.isNotEmpty()) {
            item { SectionHeading("Songs") }
            underWorks(
                tracks,
                details,
                name,
                playing,
                page,
                act,
                // A song says which record it is on, beside that record's cover; a movement, who played it.
                under = { t, d -> if (d?.work.isNullOrEmpty()) albumOf(t)?.name ?: t.creator else songUnder(t, d, name) },
                art = true,
            ) { t, _ ->
                val album = albumOf(t)
                Cover(album?.name ?: t.title, album?.art ?: "", Space.thumb, Modifier.size(Space.thumb), corner = 4.dp)
            }
        }
        // The episodes they are on, newest first, each saying whose show it is.
        if (appearances.isNotEmpty()) {
            item { SectionHeading("Episodes") }
            item { HorizontalDivider(Modifier.padding(start = Space.page), color = MaterialTheme.colorScheme.outlineVariant) }
            itemsIndexed(appearances, key = { _, t -> "episode:" + t.id.hex }) { i, t ->
                EpisodeRow(
                    t,
                    null,
                    mine = mine[t.id],
                    others = emptyList(),
                    download = LocalDownloads.current[t.id],
                    playing = t.id.hex == playing,
                    now = 0,
                    onClick = { act.play(appearances, i, page) },
                    onMore = { act.more(t, page) },
                    show = t.creator,
                )
            }
        }
        item { Spacer(Modifier.height(Space.xl)) }
    }
}

// Somebody's own page on the web, by its host, opened in the browser.
@Composable
private fun WebLink(link: String) {
    val uri = LocalUriHandler.current
    val host = link.substringAfter("://").substringBefore('/').removePrefix("www.").ifEmpty { link }
    Row(
        Modifier.padding(horizontal = Space.page - Space.s).clip(RoundedCornerShape(50)).clickable { runCatching { uri.openUri(link) } }.padding(horizontal = Space.s, vertical = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Glyphs.Link, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(16.dp))
        Spacer(Modifier.width(Space.s))
        Text(host, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/**
 * A work's page, as a catalogue sets one: its composer over its title — the way back to them —
 * what it is under, a hairline; then the fullest performance set apart with a Play, and every
 * performance — cover, who played it, when, how long — the same music told apart by who played it.
 * A work of one performance has only that one, set apart.
 */
@Composable
fun WorkPage(id: String, work: WorkSummary?, recordings: List<RecordingsEntry>, act: Do) {
    Recompositions.hit("WorkPage")
    val composer = work?.composer.orEmpty()
    LazyColumn(Modifier.fillMaxSize(), state = rememberDestinationListState("work")) {
        item { PageBar("", onBack = act::back) }
        item {
            Column(Modifier.fillMaxWidth().padding(start = Space.page, end = Space.page, bottom = Space.l)) {
                if (composer.isNotEmpty()) {
                    Text(
                        composer.uppercase() + "  ›",
                        style = quiet(MaterialTheme.typography.labelLarge),
                        modifier = Modifier.clickable { act.open(Page.ForArtist(composer)) }.padding(vertical = Space.xs),
                    )
                }
                Text(work?.title.orEmpty(), style = MaterialTheme.typography.headlineMedium)
                val facts = work?.let(Catalogue::workFacts).orEmpty()
                if (facts.isNotEmpty()) Text(facts, style = quiet(MaterialTheme.typography.bodyMedium), modifier = Modifier.padding(top = Space.xs))
            }
        }
        item { HorizontalDivider(Modifier.padding(horizontal = Space.page), color = MaterialTheme.colorScheme.outlineVariant) }
        if (recordings.isEmpty()) {
            item { Text("No recordings of it yet.", style = quiet(MaterialTheme.typography.bodyMedium), modifier = Modifier.padding(Space.page)) }
            return@LazyColumn
        }
        val first = recordings[0]
        item(key = "featured") {
            Featured(
                Catalogue.featuredLabel(recordings.size),
                first.id,
                first.art,
                Catalogue.who(first),
                Catalogue.takeFacts(first),
                onPlay = { act.playRecording(first.id) },
                onOpen = { act.open(Page.ForRecording(first.id)) },
            )
        }
        // Every performance, under the one set apart — the only one too.
        if (recordings.isNotEmpty()) {
            item { SectionHeading("Recordings") }
            items(recordings, key = { "take:" + it.id }) { r ->
                Entry(Catalogue.who(r), "", Catalogue.takeFacts(r), "", onOpen = { act.open(Page.ForRecording(r.id)) }) { m ->
                    Cover(r.id, r.art, 64.dp, Modifier.size(64.dp).then(m), corner = 4.dp)
                }
            }
        }
        item { Spacer(Modifier.height(Space.xl)) }
    }
}

/** Somebody on a recording, as its page shows them: what they did, their portrait, and their page if they have one. */
data class OnRecording(val name: String, val role: String, val art: String, val page: Page?)

/**
 * One performance's page, laid out as its album's is: the cover beside the work and who played it,
 * the movements, the small print — then the records it is on, and everybody on it, the composer
 * first.
 */
@Composable
fun RecordingPage(
    id: String,
    work: WorkSummary?,
    take: RecordingsEntry?,
    tracks: List<Track>,
    details: Map<Id, TrackDetail>,
    on: List<Album>,
    people: List<OnRecording>,
    playing: String?,
    act: Do,
) {
    Recompositions.hit("RecordingPage")
    val page = Page.ForRecording(id)
    val composer = work?.composer.orEmpty()
    val who = take?.let(Catalogue::who).orEmpty()
    val year = take?.recorded ?: 0
    LazyColumn(Modifier.fillMaxSize(), state = rememberDestinationListState("recording")) {
        item { PageBar("", onBack = act::back) }
        item {
            CoverHeader(
                seed = id,
                art = take?.art ?: "",
                title = Catalogue.recordingTitle(work, who),
                over = composer,
                onOver = if (composer.isNotEmpty()) ({ act.open(Page.ForArtist(composer)) }) else null,
                by = who,
                small = if (year > 0) year.toString() else null,
                canPlay = tracks.isNotEmpty(),
                onPlay = { act.play(tracks, 0, page) },
                onShuffle = { act.shuffle(tracks, page) },
            )
        }
        // In the order the work goes, so no headings: the page is one work.
        itemsIndexed(tracks, key = { _, t -> t.id.hex }) { i, t ->
            val d = details[t.id]
            TrackRow(
                t,
                under = songUnder(t, d, ""),
                playing = t.id.hex == playing,
                onClick = { act.play(tracks, i, page) },
                onMore = { act.more(t, page) },
                art = false,
            ) { TrackNumber(0, t.id.hex == playing) }
        }
        item { Foot(tracks, year, take?.label ?: "", Catalogue.licences(tracks.map { details[it.id] })) }
        if (on.isNotEmpty()) {
            item { SectionHeading(if (on.size == 1) "From the Album" else "From the Albums") }
            items(on, key = { "album:" + it.name }) { a ->
                Entry(a.name, "", a.by, "", onOpen = { act.open(Page.ForAlbum(a.name)) }) { m ->
                    Cover(a.name, a.art, 64.dp, Modifier.size(64.dp).then(m), corner = 4.dp)
                }
            }
        }
        if (people.isNotEmpty()) {
            item { SectionHeading("On This Recording") }
            item {
                Shelf("people", people, key = { it.name }) { p ->
                    Face(p.name, p.art, p.role, p.page?.let { to -> { act.open(to) } })
                }
            }
        }
        item { Spacer(Modifier.height(Space.xl)) }
    }
}

/**
 * A podcast's or a series of sermons' page, as a podcast app lays one out: its picture, who makes
 * it, how many episodes, Play and Shuffle, who hosts it, and what it is about; why its feed's last
 * read failed, and how many of its episodes are not on the server yet; then its episodes newest
 * first ([EpisodeRow]), each with whoever was on it besides the hosts, and at their foot the older
 * ones fetched while the feed has more than the library keeps. A followed show's own settings — its
 * Unfollow — are in its bar's menu.
 */
@Composable
fun ShowPage(
    id: String,
    show: Show?,
    episodes: List<Track>,
    playing: String?,
    act: Do,
    feed: Subscription? = null,
    downloads: Map<Id, Download> = emptyMap(),
    onFetchOlder: (Subscription, Long) -> Unit = { _, _ -> },
    listened: List<Listened> = emptyList(),
    me: String = "",
    details: Map<Id, EpisodeDetail> = emptyMap(),
    onUnfollow: (Subscription) -> Unit = {},
    /** Who makes it: the hosts its newest episode names. */
    hosts: List<OnEpisode> = emptyList(),
    /** Who is on each episode, by its media id: the show's, read once (`Library.showPeople`). */
    people: Map<Id, List<OnEpisode>> = emptyMap(),
) {
    Recompositions.hit("ShowPage")
    val page = Page.ForShow(id)
    val title = show?.title ?: "Show"
    // What "2 days ago" is counted from: the page's opening, not every frame.
    val now = remember { System.currentTimeMillis() }
    // Each row's, found once rather than by every row as it scrolls in.
    val heardBy = remember(listened) { listened.groupBy { it.mediaId } }
    val guestsBy = remember(people) { people.mapValues { OnEpisode.guests(it.value) } }
    LazyColumn(Modifier.fillMaxSize(), state = rememberDestinationListState("show")) {
        item { PageBar("", onBack = act::back) { if (feed != null) ShowMenu(title, feed, onUnfollow) } }
        item {
            PageHeader(
                cover = { Cover(title, show?.art ?: "", Space.header, Modifier.size(Space.header)) },
                kind = if (id.startsWith("sermon/")) "Sermons" else "Podcast",
                title = title,
                by = show?.author?.takeIf { it.isNotEmpty() },
                onBy = null,
                facts = facts(Media.count(episodes.size, "episode"), length(episodes)),
                canPlay = episodes.isNotEmpty(),
                onPlay = { act.play(episodes, 0, page) },
                onShuffle = { act.shuffle(episodes, page) },
            )
        }
        // Its hosts, named once here rather than on every episode.
        if (hosts.isNotEmpty()) item(key = "hosts") { Hosts(hosts, onOpen = { act.open(Page.ForArtist(it.name)) }) }
        val about = show?.description.orEmpty()
        if (about.isNotEmpty()) {
            item {
                Text(
                    about,
                    style = quiet(MaterialTheme.typography.bodyMedium),
                    maxLines = 6,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(horizontal = Space.page, vertical = Space.s),
                )
            }
        }
        // Why its feed's last read failed, and how many of its episodes are not on the server yet:
        // the desktop's.
        val waiting = episodes.mapNotNull { downloads[it.id] }
        val error = feed?.error.orEmpty()
        if (waiting.isNotEmpty() || error.isNotEmpty()) {
            item {
                Column(Modifier.padding(horizontal = Space.page, vertical = Space.s)) {
                    if (error.isNotEmpty()) Text("Could not read it: $error", style = quiet(MaterialTheme.typography.bodySmall))
                    if (waiting.isNotEmpty()) {
                        val failed = waiting.count { it.state == "failed" }
                        val going = waiting.firstOrNull { it.state == "downloading" }
                        val said =
                            listOfNotNull(
                                "${waiting.size} not on the server yet",
                                going?.said,
                                if (failed > 0) "$failed could not be downloaded" else null,
                            ).joinToString(" · ")
                        Text(said, style = quiet(MaterialTheme.typography.bodySmall))
                    }
                }
            }
        }
        if (show == null) {
            item { Empty(if (id.startsWith("sermon/")) Glyphs.Pulpit else Glyphs.Show, "This show is gone", "It was taken off this server.") }
        } else if (episodes.isEmpty()) {
            item { Empty(if (id.startsWith("sermon/")) Glyphs.Pulpit else Glyphs.Show, "No episodes yet", "The server downloads a feed's newest episodes; they appear here as they arrive.") }
        }
        if (episodes.isNotEmpty()) item { HorizontalDivider(Modifier.padding(start = Space.page), color = MaterialTheme.colorScheme.outlineVariant) }
        itemsIndexed(episodes, key = { _, t -> t.id.hex }) { i, t ->
            // The signed-in person's place in it is the row's; anybody else's is said beside it.
            val heard = heardBy[t.id].orEmpty()
            EpisodeRow(
                t,
                details[t.id],
                mine = heard.firstOrNull { it.userId == me },
                others = heard.filter { it.userId != me },
                download = downloads[t.id],
                playing = t.id.hex == playing,
                now = now,
                onClick = { act.play(episodes, i, page) },
                onMore = { act.more(t, page) },
                guests = guestsBy[t.id].orEmpty(),
                onPerson = { act.open(Page.ForArtist(it.name)) },
            )
        }
        // Older ones, twenty at a time, while the feed has more than the library keeps.
        if (feed != null && feed.hasOlder) {
            item(key = "older") {
                OutlinedButton(onClick = { onFetchOlder(feed, 20) }, modifier = Modifier.padding(start = Space.page, end = Space.page, top = Space.l)) {
                    Text("Fetch ${minOf(feed.items - feed.keep, 20L)} older")
                }
            }
        }
        item { Spacer(Modifier.height(Space.xl)) }
    }
}

// A followed show's settings, behind its bar's menu: its Unfollow.
@Composable
private fun ShowMenu(title: String, feed: Subscription, onUnfollow: (Subscription) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) { Glyph(Glyphs.More, "More for $title") }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text("Unfollow") },
                leadingIcon = { Glyph(Glyphs.Close, null, size = 20.dp) },
                onClick = {
                    open = false
                    onUnfollow(feed)
                },
            )
        }
    }
}

// "Hosted by" under a show's header, each host a face and a name that opens their page.
@Composable
private fun Hosts(hosts: List<OnEpisode>, onOpen: (OnEpisode) -> Unit) {
    Box(Modifier.fillMaxWidth().padding(horizontal = Space.page), contentAlignment = Alignment.Center) {
        Row(Modifier.horizontalScroll(rememberDestinationScrollState("hosts")), verticalAlignment = Alignment.CenterVertically) {
            Text("Hosted by", style = quiet(MaterialTheme.typography.labelLarge), maxLines = 1)
            for (h in hosts) {
                Row(
                    Modifier.clip(RoundedCornerShape(50)).clickable { onOpen(h) }.padding(horizontal = Space.s, vertical = Space.xs),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Cover(h.name, h.art, 24.dp, Modifier.size(24.dp), round = true)
                    Spacer(Modifier.width(Space.s))
                    Text(h.name, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary, maxLines = 1)
                }
            }
        }
    }
}

// Who else was on an episode, under what it is about: their faces overlapping, and "with" their
// names. Somebody alone opens their page; several are a menu of them, each opening theirs.
@Composable
private fun Guests(guests: List<OnEpisode>, onOpen: (OnEpisode) -> Unit) {
    var choosing by remember { mutableStateOf(false) }
    Box(Modifier.padding(top = Space.s)) {
        Row(
            Modifier.clip(RoundedCornerShape(50)).clickable { if (guests.size == 1) onOpen(guests[0]) else choosing = true }.padding(vertical = Space.xs, horizontal = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val faces = guests.take(3)
            Box {
                for ((i, g) in faces.withIndex()) {
                    // Each over the last, ringed in the ground so the overlap reads.
                    Box(Modifier.padding(start = 14.dp * i).size(22.dp).clip(CircleShape).background(MaterialTheme.colorScheme.background).padding(1.5.dp)) {
                        Cover(g.name, g.art, 22.dp, Modifier.fillMaxSize(), round = true)
                    }
                }
            }
            Spacer(Modifier.width(Space.s))
            Text(
                "with " + OnEpisode.named(guests),
                style = quiet(MaterialTheme.typography.labelMedium),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        DropdownMenu(expanded = choosing, onDismissRequest = { choosing = false }) {
            for (g in guests) {
                DropdownMenuItem(
                    text = { Text(if (g.role.isEmpty()) g.name else "${g.name} · ${g.role}", maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    leadingIcon = { Cover(g.name, g.art, 28.dp, Modifier.size(28.dp), round = true) },
                    onClick = {
                        choosing = false
                        onOpen(g)
                    },
                )
            }
        }
    }
}

/**
 * One episode, as a podcast app lists them: when it came out, in small capitals; its title, bold;
 * what it is about, a few lines of it; and under them its play and how much of it there is — its
 * length, the time left once begun, Played, or how its download to the server goes while the file
 * is not there yet — with anybody else's place beside it. Begun, a bar across the row says how far
 * in the listener is. Whoever else was on it (`guests`) is under what it is about, their faces and
 * their names, opening their page. The whole row plays it; its menu is at its top right. Off its
 * show's page, the small capitals name the `show` too.
 */
@Composable
fun EpisodeRow(
    track: Track,
    detail: EpisodeDetail?,
    mine: Listened?,
    others: List<Listened>,
    download: Download?,
    playing: Boolean,
    now: Long,
    onClick: () -> Unit,
    onMore: () -> Unit,
    guests: List<OnEpisode> = emptyList(),
    onPerson: (OnEpisode) -> Unit = {},
    show: String = "",
) {
    Recompositions.hit("EpisodeRow")
    val dated = facts(show.uppercase(), Episodes.dated(detail?.publishedMs ?: 0, now))
    val about = detail?.description.orEmpty()
    val progress = Episodes.progress(track, mine)
    Column(Modifier.fillMaxWidth().clickable(onClick = onClick)) {
        Row(Modifier.fillMaxWidth().padding(start = Space.page, end = Space.xs, top = Space.m), verticalAlignment = Alignment.Top) {
            Column(Modifier.weight(1f).padding(top = Space.xs)) {
                if (dated.isNotEmpty()) Text(dated, style = quiet(MaterialTheme.typography.labelSmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    track.title,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                    color = if (playing) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                if (about.isNotEmpty()) {
                    Text(about, style = quiet(MaterialTheme.typography.bodyMedium), maxLines = 3, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = Space.xs))
                }
                if (guests.isNotEmpty()) Guests(guests, onPerson)
            }
            MoreButton(track.title, null, onMore)
        }
        Row(Modifier.fillMaxWidth().padding(start = Space.page, end = Space.page, top = Space.s), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(28.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHigh), contentAlignment = Alignment.Center) {
                Icon(if (playing) Glyphs.Playing else Glyphs.Play, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(14.dp))
            }
            Spacer(Modifier.width(Space.s))
            Text(Episodes.left(track, mine, download), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary, maxLines = 1)
            if (others.isNotEmpty()) {
                Text(
                    " · " + Listened.said(others, ""),
                    style = quiet(MaterialTheme.typography.labelMedium),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
            }
        }
        if (progress != null) {
            // The whole row's width is the whole episode.
            Box(
                Modifier.fillMaxWidth()
                    .padding(start = Space.page, end = Space.page, top = Space.s)
                    .height(4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(MaterialTheme.colorScheme.surfaceVariant),
            ) {
                Box(Modifier.fillMaxWidth(progress).fillMaxHeight().background(MaterialTheme.colorScheme.primary))
            }
        }
        Spacer(Modifier.height(Space.m))
        HorizontalDivider(Modifier.padding(start = Space.page), color = MaterialTheme.colorScheme.outlineVariant)
    }
}

@Composable
fun PlaylistPage(id: Id, playlist: Playlist?, tracks: List<Track>, albumOf: (Track) -> Album?, playing: String?, act: Do) {
    Recompositions.hit("PlaylistPage")
    val page = Page.ForPlaylist(id)
    val name = playlist?.name ?: "Playlist"
    LazyColumn(Modifier.fillMaxSize(), state = rememberDestinationListState("playlist")) {
        item { PageBar("", onBack = act::back) }
        item {
            PageHeader(
                cover = { Cover(name, "", Space.header, Modifier.size(Space.header)) },
                kind = "Playlist",
                title = name,
                by = null,
                onBy = null,
                facts = facts(Media.count(tracks.size, "song"), length(tracks)),
                canPlay = tracks.isNotEmpty(),
                onPlay = { act.play(tracks, 0, page) },
                onShuffle = { act.shuffle(tracks, page) },
            )
        }
        if (playlist == null) {
            item { Empty(Glyphs.Playlist, "This playlist is gone", "It was taken off this server, or it isn't yours.") }
        } else if (tracks.isEmpty()) {
            item { Empty(Glyphs.Playlist, "Nothing on it yet", "Put a song on it from the song's menu, anywhere in your library.") }
        }
        itemsIndexed(tracks, key = { _, t -> t.id.hex }) { i, t ->
            val album = albumOf(t)
            TrackRow(
                t,
                under = listOfNotNull(t.creator, album?.name).joinToString(" · "),
                playing = t.id.hex == playing,
                onClick = { act.play(tracks, i, page) },
                onMore = { act.more(t, page) },
            ) { Cover(album?.name ?: t.title, album?.art ?: "", Space.thumb, Modifier.size(Space.thumb), corner = 4.dp) }
        }
        item { Spacer(Modifier.height(Space.xl)) }
    }
}
