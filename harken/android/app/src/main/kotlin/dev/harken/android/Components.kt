// The pieces harken's screens are made of that are harken's: a track's row
// and its number, a page's header with Play and Shuffle, what the banner a
// server that cannot be reached puts over everything says, and a catalogue's
// pieces (below). The rest — headings,
// shelves, cards, rows that open something, a row's menu button, what an
// empty list says — is `ark-ui`'s (`dev.arkdb.ui`). Drawn from plain values
// and the theme, so a screen is a function of its data and can be rendered
// without a phone (the screenshots).
package dev.harken.android

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import dev.arkui.Banner
import dev.arkui.PageHeader
import dev.arkui.Cover
import dev.arkui.Glyph
import dev.arkui.LocalPageFrame
import dev.arkui.MoreButton
import dev.arkui.Origin
import dev.arkui.Recompositions
import dev.arkui.RowOrigin
import dev.arkui.Space
import dev.arkui.Tapped
import dev.arkui.quiet
import dev.arkui.rememberHero
import dev.arkui.pressable
import dev.arkui.Press
import uniffi.ark_format.formatClock
import uniffi.ark_format.formatCount
import uniffi.ark_format.formatLength

/** What a screen can ask for. Every one does nothing unless the app says otherwise. */
interface Do {
    fun open(page: Page) {}

    fun play(queue: List<Track>, at: Int, from: Page?) {}

    fun shuffle(queue: List<Track>, from: Page?) {}

    /** Start one performance (its key) from its first movement, the queue its movements: read when pressed. */
    fun playRecording(id: String) {}

    /** Start a work (its key): its most complete performance. */
    fun playWork(id: String) {}

    /** The row's menu, about `track`, on a list that came from `from`. */
    fun more(track: Track, from: Page?) {}

    fun resume(last: LastPlayed) {}

    fun newPlaylist() {}

    /** The sheet that finds a show to follow, as `kind` (`podcast`, `sermon`): the shows page's +. */
    fun findShow(kind: String) {}

    /** Stop following a feed, and let its show go. */
    fun unfollow(feed: Subscription) {}

    fun library(tab: LibraryTab) {}

    fun settings() {}

    /** The account sheet, which the face at the end of a tab's title raises. */
    fun account() {}

    fun back() {}
}

/** A [Do] that does nothing: what a screenshot hands a screen. */
object Idle : Do

/**
 * The files the server has not got yet, by their media id (`Library.downloads`): a row whose file
 * is among them says how its download goes where its length goes.
 */
val LocalDownloads = compositionLocalOf<Map<dev.arkdb.ffi.Id, Download>> { emptyMap() }

/** The chapters of what plays, in order; none for a song or an episode without. */
val LocalChapters = compositionLocalOf<List<Chapter>> { emptyList() }

/** The transcript of what plays, line by line; none for a song or an episode without. */
val LocalTranscript = compositionLocalOf<List<Cue>> { emptyList() }

/**
 * One track, as every list draws it: what stands at its start (a cover, or its number on the
 * release), its title — in the gold while it plays — whoever made it, its length, and its menu.
 * Opening the menu says where the row is (`PageMotion.tapped`), and where its cover is when what
 * stands at its start is the album's art (`art`): a page the menu goes to grows out of them.
 */
@Composable
fun TrackRow(
    track: Track,
    under: String,
    playing: Boolean,
    onClick: () -> Unit,
    onMore: () -> Unit,
    art: Boolean = true,
    leading: @Composable () -> Unit,
) {
    Recompositions.hit("TrackRow")
    Row(
        Modifier.fillMaxWidth().pressable(onClick = onClick, press = Press.Highlight).padding(start = Space.page, end = Space.xs).height(ROW),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(Space.thumb), contentAlignment = Alignment.Center) { leading() }
        Spacer(Modifier.width(Space.m))
        Column(Modifier.weight(1f)) {
            Text(
                track.title,
                style = MaterialTheme.typography.bodyLarge,
                color = if (playing) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(under, style = quiet(MaterialTheme.typography.bodyMedium), maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Text(
            LocalDownloads.current[track.id]?.short ?: formatClock(track.durationMs),
            style = quiet(MaterialTheme.typography.bodySmall),
            modifier = Modifier.padding(horizontal = Space.s),
        )
        MoreButton(track.title, if (art) TrackArt else TrackPlain, onMore)
    }
}

/** A track row's height. */
val ROW = 64.dp

/**
 * Where a track's row and its cover are when its menu opens, from what the row lays out inside its
 * margins (where the menu button stands): the cover `Space.thumb` square, inset by the page's
 * margin and centred down the row, its corner a row cover's. One for a row whose leading picture
 * is the album's art, which a page the menu goes to grows out of, and one for a row whose is not.
 */
private class TrackOrigin(val art: Boolean) : RowOrigin {
    override fun tapped(inner: Rect, density: Density): Tapped =
        with(density) {
            // The row: the margins put back.
            val row = Rect(inner.left - Space.page.toPx(), inner.top, inner.right + Space.xs.toPx(), inner.bottom)
            val side = Space.thumb.toPx()
            val cover = Rect(Offset(row.left + Space.page.toPx(), row.top + (row.height - side) / 2), Size(side, side))
            Tapped(art = if (art) Origin(cover, 4.dp.toPx()) else null, row = Origin(row, 0f, art = false))
        }
}

private val TrackArt = TrackOrigin(art = true)
private val TrackPlain = TrackOrigin(art = false)

/** The number a track has on its release, or the playing mark, in the leading column. */
@Composable
fun TrackNumber(n: Long, playing: Boolean) {
    if (playing) {
        Icon(Glyphs.Playing, contentDescription = "Playing", tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
    } else {
        Text(if (n > 0) n.toString() else "", style = quiet(MaterialTheme.typography.bodyMedium))
    }
}

/**
 * The header of a page that is about something that plays: ArkUI's [PageHeader] — its cover, what
 * it is, its name, who it is by, the numbers true of the whole of it — with Play and Shuffle.
 */
@Composable
fun PlayHeader(
    cover: @Composable () -> Unit,
    kind: String,
    title: String,
    by: String?,
    onBy: (() -> Unit)?,
    facts: String,
    canPlay: Boolean,
    onPlay: () -> Unit,
    onShuffle: () -> Unit,
) {
    PageHeader(kind = kind, title = title, facts = facts, by = by, onBy = onBy, cover = cover) {
        Button(onClick = onPlay, enabled = canPlay, modifier = Modifier.width(148.dp)) {
            Glyph(Glyphs.Play, null, size = 16.dp)
            Spacer(Modifier.width(Space.s))
            Text("Play")
        }
        OutlinedButton(onClick = onShuffle, enabled = canPlay, modifier = Modifier.width(148.dp)) {
            Glyph(Glyphs.Shuffle, null, size = 16.dp)
            Spacer(Modifier.width(Space.s))
            Text("Shuffle")
        }
    }
}

/**
 * Over every tab while the server cannot be reached: which one, and that what is on the phone
 * still works.
 */
@Composable
fun Offline(host: String) {
    Banner(Glyphs.Offline, "Can't reach $host. What's on this phone still plays, and your changes sync when it's back.")
}

// A catalogue's pieces ---------------------------------------------------------------------------
//
// What the pages about a composer, a work, a performance and a record are made of, as the
// desktop's are (`ui/rust/arkui/src/hero.rs`, `harken/iced/src/view.rs`): a header with the cover
// beside the words, the one thing set apart with a Play, a work's heading over its movements,
// and a record's small print. The banner with the name on it, a section's heading, a listing's
// entry over its hairline and the line that opens the rest are ArkUI's (`HeroBanner`,
// `HeroHeading`, `HeroEntry`, `HeroMore`).

/** A Play, small and in the accent's tone: what a header and a featured performance start with. */
@Composable
fun PlayPill(onPlay: () -> Unit, enabled: Boolean = true) {
    FilledTonalButton(onClick = onPlay, enabled = enabled, contentPadding = PaddingValues(horizontal = Space.l), modifier = Modifier.height(36.dp)) {
        Glyph(Glyphs.Play, null, size = 14.dp)
        Spacer(Modifier.width(Space.s))
        Text("Play")
    }
}

/**
 * The header a record's page opens with, as a catalogue sets one: the cover beside a line of small
 * type (`over`, whose it is), the title in bold, who played it in the accent, a small line
 * under (`small`), and Play and Shuffle. A line that goes somewhere opens it.
 */
@Composable
fun CoverHeader(
    seed: String,
    art: String,
    title: String,
    over: String? = null,
    onOver: (() -> Unit)? = null,
    by: String? = null,
    small: String? = null,
    onSmall: (() -> Unit)? = null,
    round: Boolean = false,
    canPlay: Boolean = false,
    onPlay: (() -> Unit)? = null,
    onShuffle: (() -> Unit)? = null,
) {
    Row(Modifier.fillMaxWidth().padding(start = Space.page, end = Space.page, top = Space.s, bottom = Space.l), verticalAlignment = Alignment.CenterVertically) {
        val side = 132.dp
        // Where the art a page grew from lands.
        Box(LocalPageFrame.current?.art?.modifier ?: Modifier) { Cover(seed, art, side, Modifier.size(side), round = round, corner = 6.dp) }
        Spacer(Modifier.width(Space.l))
        Column(Modifier.weight(1f)) {
            if (!over.isNullOrEmpty()) {
                Text(over, style = quiet(MaterialTheme.typography.labelMedium), modifier = if (onOver != null) Modifier.pressable(onClick = onOver) else Modifier)
                Spacer(Modifier.height(Space.xs))
            }
            Text(title, style = MaterialTheme.typography.headlineSmall)
            if (!by.isNullOrEmpty()) Text(by, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(top = Space.xs))
            if (!small.isNullOrEmpty()) {
                Text(small, style = quiet(MaterialTheme.typography.bodySmall), modifier = Modifier.padding(top = Space.xs).then(if (onSmall != null) Modifier.pressable(onClick = onSmall) else Modifier))
            }
            if (onPlay != null) {
                Spacer(Modifier.height(Space.m))
                Row(horizontalArrangement = Arrangement.spacedBy(Space.s), verticalAlignment = Alignment.CenterVertically) {
                    PlayPill(onPlay, enabled = canPlay)
                    if (onShuffle != null) {
                        IconButton(onClick = onShuffle, enabled = canPlay) { Glyph(Glyphs.Shuffle, "Shuffle", size = 20.dp) }
                    }
                }
            }
        }
    }
}

/**
 * One thing set apart at the head of a section, the one to start with: its cover beside `label`
 * in small type, its title, its facts, and a Play. The cover and the title open it, the page
 * growing out of the cover.
 */
@Composable
fun Featured(label: String, seed: String, art: String, title: String, facts: String, onPlay: () -> Unit, onOpen: () -> Unit) {
    val hero = rememberHero(6.dp)
    val open = hero.lifting(onOpen)
    Row(Modifier.fillMaxWidth().padding(start = Space.page, end = Space.page, top = Space.l, bottom = Space.xs), verticalAlignment = Alignment.CenterVertically) {
        Cover(seed, art, 112.dp, Modifier.size(112.dp).then(hero.modifier).pressable(onClick = open), corner = 6.dp)
        Spacer(Modifier.width(Space.l))
        Column(Modifier.weight(1f)) {
            Text(label, style = quiet(MaterialTheme.typography.labelSmall))
            Text(title, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.pressable(onClick = open))
            if (facts.isNotEmpty()) Text(facts, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
            Spacer(Modifier.height(Space.s))
            PlayPill(onPlay)
        }
    }
}

/**
 * A work's heading on a record: its composer in small type over its title, and
 * how long the run of it under the heading lasts, where the tracks' lengths are.
 */
@Composable
fun WorkHeading(composer: String, work: String, ms: Long, first: Boolean) {
    Row(Modifier.fillMaxWidth().padding(start = Space.page, end = Space.xs + 48.dp + Space.s, top = if (first) Space.s else Space.xl, bottom = Space.xs), verticalAlignment = Alignment.Bottom) {
        Column(Modifier.weight(1f)) {
            if (composer.isNotEmpty()) Text(composer, style = quiet(MaterialTheme.typography.labelSmall))
            Text(work, style = MaterialTheme.typography.titleMedium)
        }
        if (ms > 0) Text(formatClock(ms), style = quiet(MaterialTheme.typography.bodySmall), modifier = Modifier.padding(start = Space.s))
    }
}

/** A record's small print: when, how much, on what terms ([Catalogue.licences]), and whose label. */
@Composable
fun Foot(tracks: List<Track>, year: Long, label: String, licences: List<String> = emptyList()) {
    Column(Modifier.fillMaxWidth().padding(start = Space.page, end = Space.page, top = Space.xl)) {
        if (year > 0) Text(year.toString(), style = quiet(MaterialTheme.typography.bodySmall))
        val ms = tracks.sumOf { it.durationMs }
        Text(formatCount(tracks.size.toLong(), "track") + if (ms > 0) ", " + formatLength(ms) else "", style = quiet(MaterialTheme.typography.bodySmall))
        for (terms in licences) Text(terms, style = quiet(MaterialTheme.typography.bodySmall))
        if (label.isNotEmpty()) {
            Spacer(Modifier.height(Space.l))
            Text("Record Label", style = quiet(MaterialTheme.typography.labelSmall))
            Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary)
        }
    }
}

/**
 * A person on a recording: their portrait (ArkUI's round [Cover]), their name, what they did. Opens
 * their page where they have one. Not the account's [dev.arkui.Face]: a performer has a portrait or
 * their derived square, never initials on the accent.
 */
@Composable
fun Credit(name: String, art: String, role: String, onOpen: (() -> Unit)?) {
    val hero = if (onOpen != null) rememberHero(Space.corner, round = true) else null
    Column(
        Modifier.width(104.dp).then(if (onOpen != null && hero != null) Modifier.pressable(onClick = hero.lifting(onOpen)) else Modifier),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Cover(name, art, 88.dp, Modifier.size(88.dp).then(hero?.modifier ?: Modifier), round = true)
        Spacer(Modifier.height(Space.s))
        Text(name, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
        if (role.isNotEmpty()) Text(role, style = quiet(MaterialTheme.typography.bodySmall), textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}
