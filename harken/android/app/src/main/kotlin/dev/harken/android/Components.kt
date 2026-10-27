// The pieces harken's screens are made of that are harken's: a track's row
// and its number, a page's header with Play and Shuffle, the banner a
// server that cannot be reached puts over everything, and a catalogue's
// pieces (below). The rest — headings,
// shelves, cards, rows that open something, a row's menu button, what an
// empty list says — is `ark-ui`'s (`dev.arkdb.ui`). Drawn from plain values
// and the theme, so a screen is a function of its data and can be rendered
// without a phone (the screenshots).
package dev.harken.android

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.shape.CircleShape
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
import dev.harken.android.ui.ArkGlyphs
import dev.harken.android.ui.Chevron
import dev.harken.android.ui.Cover
import dev.harken.android.ui.Glyph
import dev.harken.android.ui.LocalPageFrame
import dev.harken.android.ui.MoreButton
import dev.harken.android.ui.Origin
import dev.harken.android.ui.Recompositions
import dev.harken.android.ui.RowOrigin
import dev.harken.android.ui.Space
import dev.harken.android.ui.Tapped
import dev.harken.android.ui.quiet
import dev.harken.android.ui.rememberHero

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
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(start = Space.page, end = Space.xs).height(ROW),
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
            LocalDownloads.current[track.id]?.short ?: Media.clock(track.durationMs),
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
 * The header of a page that is about something: its cover, what it is, its name, who it is by (a
 * link, when it goes somewhere), the numbers true of the whole of it, and Play and Shuffle.
 */
@Composable
fun PageHeader(
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
    Column(Modifier.fillMaxWidth().padding(horizontal = Space.page), horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(Space.s))
        // Where the art a page grew from lands.
        Box(LocalPageFrame.current?.art?.modifier ?: Modifier) { cover() }
        Spacer(Modifier.height(Space.l))
        Text(kind.uppercase(), style = quiet(MaterialTheme.typography.labelMedium))
        Spacer(Modifier.height(Space.xs))
        Text(title, style = MaterialTheme.typography.headlineSmall, textAlign = TextAlign.Center)
        if (by != null) {
            Text(
                by,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary,
                textAlign = TextAlign.Center,
                modifier = if (onBy != null) Modifier.clickable(onClick = onBy).padding(Space.xs) else Modifier.padding(Space.xs),
            )
        }
        Text(facts, style = quiet(MaterialTheme.typography.bodySmall), textAlign = TextAlign.Center)
        Spacer(Modifier.height(Space.l))
        Row(horizontalArrangement = Arrangement.spacedBy(Space.m)) {
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
        Spacer(Modifier.height(Space.m))
    }
}

/**
 * Over every tab while the server cannot be reached: which one, and that what is on the phone
 * still works.
 */
@Composable
fun Offline(host: String) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = RoundedCornerShape(Space.corner), modifier = Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.s)) {
        Row(Modifier.padding(Space.m), verticalAlignment = Alignment.CenterVertically) {
            Icon(Glyphs.Offline, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(Space.m))
            Text(
                "Can't reach $host. What's on this phone still plays, and your changes sync when it's back.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// A catalogue's pieces ---------------------------------------------------------------------------
//
// What the pages about a composer, a work, a performance and a record are made of, as the
// desktop's are (`rust/arkui/src/hero.rs`, `harken/iced/src/view.rs`): a banner with the name on
// it, a header with the cover beside the words, a section's heading, the one thing
// set apart with a Play, a listing's entry over its hairline, a work's heading over its
// movements, and a record's small print.

/** The tallest a banner is drawn, the desktop's: its whole shape on a page about 1250 wide. */
val BANNER_TALLEST = 486.dp

/** How far below the status bar the shade at a banner's top runs: past its back. */
val BANNER_TOP_SHADE = 72.dp

/** A banner's shape, width over height: a painting cut for one (2400×933). */
const val BANNER_ASPECT = 2400f / 933f

/**
 * How tall a banner `width` wide is drawn on a phone. Its own shape would be a strip there (about
 * 150 dp), so a phone crops it to a taller frame, as a catalogue's phone app does, the name on its
 * lower edge; never past [BANNER_TALLEST].
 */
fun bannerHeight(width: Dp): Dp = maxOf(width / BANNER_ASPECT, width * 0.82f).coerceIn(200.dp, BANNER_TALLEST)

/**
 * Somebody's banner, edge to edge and up to the top of the screen: their wide picture cropped to
 * fill, run up under the status bar (its page says [dev.harken.android.ui.UnderStatusBar]) and that
 * much taller, so what shows below the bar is the shape [bannerHeight] says; `kind` in small
 * capitals and `name` in bold on its lower edge over a shade that keeps them legible, a lighter
 * one under the bar's icons, and back over its top, below the bar. Until the picture arrives the
 * derived square is drawn instead, so the name is already where it will be. A page grows out of a
 * portrait into it, the part under the bar included.
 */
@Composable
fun Banner(seed: String, art: String, kind: String, name: String, onBack: () -> Unit) {
    val bar = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val w = maxWidth
        Box(Modifier.fillMaxWidth().height(bar + bannerHeight(w)).then(LocalPageFrame.current?.art?.modifier ?: Modifier)) {
            Cover(seed, art, w, Modifier.fillMaxSize(), corner = 0.dp)
            // Clear at the top, darkest at the foot, where the name is.
            Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0f to Palette.Clear, 0.45f to Palette.Clear, 1f to Palette.Shade)))
            // And a little shade at the very top, under the bar's icons and back.
            Box(Modifier.fillMaxWidth().height(bar + BANNER_TOP_SHADE).background(Brush.verticalGradient(0f to Palette.Shade.copy(alpha = 0.35f), 1f to Palette.Clear)))
            Column(Modifier.align(Alignment.BottomStart).padding(start = Space.page, end = Space.page, bottom = Space.xl)) {
                Text(kind.uppercase(), style = MaterialTheme.typography.labelMedium, color = Palette.OnShade.copy(alpha = 0.85f))
                Spacer(Modifier.height(Space.xs))
                Text(name, style = MaterialTheme.typography.headlineLarge, color = Palette.OnShade)
            }
            Box(Modifier.padding(top = bar).padding(Space.xs).size(48.dp).clip(CircleShape).background(Palette.Shade).clickable(onClick = onBack), contentAlignment = Alignment.Center) {
                Icon(ArkGlyphs.Back, contentDescription = "Back", tint = Palette.OnShade, modifier = Modifier.size(24.dp))
            }
        }
    }
}

/** A section's heading, bold, with what opens the rest of it at its end. */
@Composable
fun SectionHeading(title: String, modifier: Modifier = Modifier) {
    Text(title, style = MaterialTheme.typography.headlineSmall, modifier = modifier.fillMaxWidth().padding(start = Space.page, end = Space.page, top = Space.xl, bottom = Space.s))
}

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
 * capitals (`over`, whose it is), the title in bold, who played it in the accent, a small line
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
                Text(over.uppercase(), style = quiet(MaterialTheme.typography.labelMedium), modifier = if (onOver != null) Modifier.clickable(onClick = onOver) else Modifier)
                Spacer(Modifier.height(Space.xs))
            }
            Text(title, style = MaterialTheme.typography.headlineSmall)
            if (!by.isNullOrEmpty()) Text(by, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(top = Space.xs))
            if (!small.isNullOrEmpty()) {
                Text(small, style = quiet(MaterialTheme.typography.bodySmall), modifier = Modifier.padding(top = Space.xs).then(if (onSmall != null) Modifier.clickable(onClick = onSmall) else Modifier))
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
 * in small capitals, its title, its facts, and a Play. The cover and the title open it, the page
 * growing out of the cover.
 */
@Composable
fun Featured(label: String, seed: String, art: String, title: String, facts: String, onPlay: () -> Unit, onOpen: () -> Unit) {
    val hero = rememberHero(6.dp)
    val open = hero.lifting(onOpen)
    Row(Modifier.fillMaxWidth().padding(start = Space.page, end = Space.page, top = Space.l, bottom = Space.xs), verticalAlignment = Alignment.CenterVertically) {
        Cover(seed, art, 112.dp, Modifier.size(112.dp).then(hero.modifier).clickable(onClick = open), corner = 6.dp)
        Spacer(Modifier.width(Space.l))
        Column(Modifier.weight(1f)) {
            Text(label.uppercase(), style = quiet(MaterialTheme.typography.labelSmall))
            Text(title, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.clickable(onClick = open))
            if (facts.isNotEmpty()) Text(facts, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
            Spacer(Modifier.height(Space.s))
            PlayPill(onPlay)
        }
    }
}

/**
 * One entry of a catalogue's listing over its hairline: a picture at its left when it has one,
 * a line of small capitals over its title (who it is by), a line under, a count at its right, and
 * the chevron. The whole entry opens it.
 */
@Composable
fun Entry(title: String, over: String, under: String, count: String, onOpen: () -> Unit, picture: (@Composable (Modifier) -> Unit)? = null) {
    val hero = if (picture != null) rememberHero(4.dp) else null
    Column(Modifier.fillMaxWidth().clickable(onClick = hero?.lifting(onOpen) ?: onOpen).padding(start = Space.page)) {
        Row(Modifier.fillMaxWidth().padding(end = Space.m, top = Space.m, bottom = Space.m), verticalAlignment = Alignment.CenterVertically) {
            if (picture != null && hero != null) {
                picture(hero.modifier)
                Spacer(Modifier.width(Space.m))
            }
            Column(Modifier.weight(1f)) {
                if (over.isNotEmpty()) Text(over.uppercase(), style = quiet(MaterialTheme.typography.labelSmall), maxLines = 1)
                Text(title, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
                if (under.isNotEmpty()) Text(under, style = quiet(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            if (count.isNotEmpty()) Text(count, style = quiet(MaterialTheme.typography.bodyMedium), modifier = Modifier.padding(horizontal = Space.s))
            Chevron()
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
    }
}

/** The line under a section that opens the rest of it — "See All 40 Works" — in the accent. */
@Composable
fun MoreLine(label: String, onOpen: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onOpen).padding(start = Space.page, end = Space.m, top = Space.m, bottom = Space.m), verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.primary, modifier = Modifier.weight(1f))
        Chevron()
    }
}

/**
 * A work's heading on a record: its composer in small capitals over its title, and
 * how long the run of it under the heading lasts, where the tracks' lengths are.
 */
@Composable
fun WorkHeading(composer: String, work: String, ms: Long, first: Boolean) {
    Row(Modifier.fillMaxWidth().padding(start = Space.page, end = Space.xs + 48.dp + Space.s, top = if (first) Space.s else Space.xl, bottom = Space.xs), verticalAlignment = Alignment.Bottom) {
        Column(Modifier.weight(1f)) {
            if (composer.isNotEmpty()) Text(composer.uppercase(), style = quiet(MaterialTheme.typography.labelSmall))
            Text(work, style = MaterialTheme.typography.titleMedium)
        }
        if (ms > 0) Text(Media.clock(ms), style = quiet(MaterialTheme.typography.bodySmall), modifier = Modifier.padding(start = Space.s))
    }
}

/** A record's small print: when, how much, on what terms ([Catalogue.licences]), and whose label. */
@Composable
fun Foot(tracks: List<Track>, year: Long, label: String, licences: List<String> = emptyList()) {
    Column(Modifier.fillMaxWidth().padding(start = Space.page, end = Space.page, top = Space.xl)) {
        if (year > 0) Text(year.toString(), style = quiet(MaterialTheme.typography.bodySmall))
        val ms = tracks.sumOf { it.durationMs }
        Text(Media.count(tracks.size, "track") + if (ms > 0) ", " + Media.spell(ms) else "", style = quiet(MaterialTheme.typography.bodySmall))
        for (terms in licences) Text(terms, style = quiet(MaterialTheme.typography.bodySmall))
        if (label.isNotEmpty()) {
            Spacer(Modifier.height(Space.l))
            Text("RECORD LABEL", style = quiet(MaterialTheme.typography.labelSmall))
            Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary)
        }
    }
}

/** A person on a recording, as a face: their portrait, their name, what they did. Opens their page where they have one. */
@Composable
fun Face(name: String, art: String, role: String, onOpen: (() -> Unit)?) {
    val hero = if (onOpen != null) rememberHero(Space.corner, round = true) else null
    Column(
        Modifier.width(104.dp).then(if (onOpen != null && hero != null) Modifier.clickable(onClick = hero.lifting(onOpen)) else Modifier),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Cover(name, art, 88.dp, Modifier.size(88.dp).then(hero?.modifier ?: Modifier), round = true)
        Spacer(Modifier.height(Space.s))
        Text(name, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
        if (role.isNotEmpty()) Text(role, style = quiet(MaterialTheme.typography.bodySmall), textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}
