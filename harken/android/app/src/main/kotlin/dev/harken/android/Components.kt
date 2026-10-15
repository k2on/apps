// The pieces harken's screens are made of that are harken's: a track's row
// and its number, a page's header with Play and Shuffle, and the banner a
// server that cannot be reached puts over everything. The rest — headings,
// shelves, cards, rows that open something, a row's menu button, what an
// empty list says — is `ark-ui`'s (`dev.arkdb.ui`). Drawn from plain values
// and the theme, so a screen is a function of its data and can be rendered
// without a phone (the screenshots).
package dev.harken.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import dev.arkdb.ui.Glyph
import dev.arkdb.ui.LocalPageFrame
import dev.arkdb.ui.MoreButton
import dev.arkdb.ui.Origin
import dev.arkdb.ui.Recompositions
import dev.arkdb.ui.RowOrigin
import dev.arkdb.ui.Space
import dev.arkdb.ui.Tapped
import dev.arkdb.ui.quiet

/** What a screen can ask for. Every one does nothing unless the app says otherwise. */
interface Do {
    fun open(page: Page) {}

    fun play(queue: List<Track>, at: Int, from: Page?) {}

    fun shuffle(queue: List<Track>, from: Page?) {}

    /** The row's menu, about `track`, on a list that came from `from`. */
    fun more(track: Track, from: Page?) {}

    fun resume(last: LastPlayed) {}

    fun newPlaylist() {}

    fun library(tab: LibraryTab) {}

    fun settings() {}

    fun back() {}
}

/** A [Do] that does nothing: what a screenshot hands a screen. */
object Idle : Do

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
        Text(Media.clock(track.durationMs), style = quiet(MaterialTheme.typography.bodySmall), modifier = Modifier.padding(horizontal = Space.s))
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
