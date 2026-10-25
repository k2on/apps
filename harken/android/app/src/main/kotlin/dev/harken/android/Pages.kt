// The pages a library opens, laid out as a catalogue's (the desktop's,
// `harken/iced/src/view.rs`): an album, its tracks under the works they are
// movements of; a person — one page whether they made songs or wrote works
// — under their banner, their most recorded work set apart, their best
// known works, their records, and their songs where they wrote no works; a
// work and its performances; a performance, its
// movements, its records and everybody on it; and a playlist, in the order
// it was made. The lists are views held while the page is shown
// (`Library.hold`, `Library.works` and the rest), spliced as the log moves.
package dev.harken.android

import uniffi.harken_domain.Playlist
import androidx.compose.foundation.layout.Spacer
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
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import dev.arkdb.ffi.Id
import dev.harken.android.gen.RecordingsEntry
import dev.harken.android.gen.WorkSummary
import dev.harken.android.ui.Cover
import dev.harken.android.ui.CoverCard
import dev.harken.android.ui.Empty
import dev.harken.android.ui.Glyph
import dev.harken.android.ui.PageBar
import dev.harken.android.ui.Recompositions
import dev.harken.android.ui.Shelf
import dev.harken.android.ui.Space
import dev.harken.android.ui.quiet
import dev.harken.android.ui.rememberDestinationListState

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
        !d?.performer.isNullOrEmpty() -> Catalogue.credit(d!!.performer, d.licence)
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
        item { Foot(tracks, album?.released ?: 0, album?.label ?: "") }
        item { Spacer(Modifier.height(Space.xl)) }
    }
}

/**
 * A person's page, one for everybody — whoever made a song and whoever wrote a work: their banner
 * (or, for somebody with no wide picture, their portrait beside their name), what they are and
 * when they lived; their most recorded work set apart with a Play, when one is; their best known
 * works and a "See All" that lists every one by catalogue (`all`); the records that are theirs; and,
 * for somebody who wrote no works, their songs as an album lists them. Each section only when it has
 * something in it.
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
) {
    Recompositions.hit("ArtistPage")
    val page = Page.ForArtist(name)
    val counts = facts(if (albums.isEmpty()) null else Media.count(albums.size, "album"), Media.count(tracks.size, "song"), length(tracks))
    // What they are and when they lived, as a catalogue heads a person; nobody's years known, and
    // it is how much of them there is here.
    val life = artist?.let { Catalogue.span(it.born, it.died) }?.takeIf { it.isNotEmpty() } ?: composer?.let { Catalogue.span(it.born, it.died) } ?: ""
    val kind = if (works.isNotEmpty()) "Composer" else "Artist"
    val listed = Catalogue.listed(works, all)
    val short = Catalogue.short(listed, works)
    val best = Catalogue.mostRecorded(works)
    val by = Catalogue.initials(name)
    LazyColumn(Modifier.fillMaxSize(), state = rememberDestinationListState("artist")) {
        // Their wide picture across the page, or else their portrait cropped to it: a person's page
        // always opens under them; only somebody with no picture at all has the round header.
        val banner = listOf(artist?.banner, composer?.banner, artist?.art, composer?.art).firstOrNull { !it.isNullOrEmpty() }.orEmpty()
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
                    art = artist?.art?.ifEmpty { null } ?: composer?.art.orEmpty(),
                    title = name,
                    over = facts(kind, life.ifEmpty { counts }),
                    round = true,
                    canPlay = tracks.isNotEmpty(),
                    onPlay = { act.play(tracks, 0, page) },
                    onShuffle = { act.shuffle(tracks, page) },
                )
            }
        }
        // The most recorded work, set apart with a Play — when one is.
        if (best != null) {
            item(key = "featured") {
                Featured("Most Recorded", best.id, best.art, best.title, Catalogue.recordedFacts(best), onPlay = { act.playWork(best.id) }, onOpen = { act.open(Page.ForWork(best.id)) })
            }
        }
        if (works.isNotEmpty()) {
            item { SerifHeading(if (short) "Well-Known Works" else "Works") }
            // Each work under who it is by, its catalogue number under it, and how many performances
            // of it there are at its right.
            items(listed, key = { "work:" + it.id }) { w -> Entry(w.title, by, w.catalogue, w.recordings.toString(), onOpen = { act.open(Page.ForWork(w.id)) }) }
            if (short) item { MoreLine("See All ${works.size} Works") { onAll(true) } }
        }
        if (albums.isNotEmpty()) {
            item { SerifHeading("Albums") }
            item {
                Shelf("albums", albums, key = { it.name }) { a ->
                    CoverCard(a.name, a.art, a.name, Media.count(a.tracks.toInt(), "track"), 136.dp, onClick = { act.open(Page.ForAlbum(a.name)) }, opens = true)
                }
            }
        }
        // Somebody's songs, where they wrote no works: a composer's are their works' performances.
        if (works.isEmpty() && tracks.isNotEmpty()) {
            item { SerifHeading("Songs") }
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
        item { Spacer(Modifier.height(Space.xl)) }
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
                Text(work?.title.orEmpty(), style = SerifType.title)
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
        if (recordings.size > 1) {
            item { SerifHeading("Recordings") }
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
        item { Foot(tracks, year, take?.label ?: "") }
        if (on.isNotEmpty()) {
            item { SerifHeading(if (on.size == 1) "From the Album" else "From the Albums") }
            items(on, key = { "album:" + it.name }) { a ->
                Entry(a.name, "", a.by, "", onOpen = { act.open(Page.ForAlbum(a.name)) }) { m ->
                    Cover(a.name, a.art, 64.dp, Modifier.size(64.dp).then(m), corner = 4.dp)
                }
            }
        }
        if (people.isNotEmpty()) {
            item { SerifHeading("On This Recording") }
            item {
                Shelf("people", people, key = { it.name }) { p ->
                    Face(p.name, p.art, p.role, p.page?.let { to -> { act.open(to) } })
                }
            }
        }
        item { Spacer(Modifier.height(Space.xl)) }
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
