// The pieces every screen is made of: a track's row, a shelf of covers, a
// section's heading, a page's header, the sentence an empty list says, and
// the banner a server that cannot be reached puts over everything. Drawn
// from plain values and the theme, so a screen is a function of its data
// and can be rendered without a phone (the screenshots).
package dev.harken.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

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

/** A glyph, at the size a row's controls are. */
@Composable
fun Glyph(icon: ImageVector, description: String?, modifier: Modifier = Modifier, size: Dp = 24.dp) {
    Icon(icon, contentDescription = description, modifier = modifier.size(size))
}

/** A heading over a section, with an optional action at its end. */
@Composable
fun Heading(title: String, modifier: Modifier = Modifier, action: String? = null, onAction: () -> Unit = {}) {
    Row(
        modifier.fillMaxWidth().padding(start = Space.page, end = Space.s, top = Space.xl, bottom = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
        if (action != null) TextButton(onClick = onAction) { Text(action) }
    }
}

/**
 * One track, as every list draws it: what stands at its start (a cover, or its number on the
 * release), its title — in the gold while it plays — whoever made it, its length, and its menu.
 */
@Composable
fun TrackRow(
    track: Track,
    under: String,
    playing: Boolean,
    onClick: () -> Unit,
    onMore: () -> Unit,
    leading: @Composable () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(start = Space.page, end = Space.xs).height(64.dp),
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
        IconButton(onClick = onMore) { Glyph(Glyphs.More, "More for ${track.title}", size = 20.dp) }
    }
}

/** The number a track has on its release, or the playing mark, in the leading column. */
@Composable
fun TrackNumber(n: Long, playing: Boolean) {
    if (playing) {
        Icon(Glyphs.Playing, contentDescription = "Playing", tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
    } else {
        Text(if (n > 0) n.toString() else "", style = quiet(MaterialTheme.typography.bodyMedium))
    }
}

/** One record on a shelf or in a grid: its cover, its name, and a line under it. */
@Composable
fun RecordCard(seed: String, art: String, title: String, under: String, side: Dp, onClick: () -> Unit, round: Boolean = false) {
    Column(Modifier.width(side).clickable(onClick = onClick)) {
        Cover(seed, art, side, Modifier.size(side), round = round)
        Spacer(Modifier.height(Space.s))
        Text(
            title,
            style = MaterialTheme.typography.titleSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            textAlign = if (round) TextAlign.Center else TextAlign.Start,
            modifier = Modifier.fillMaxWidth(),
        )
        if (under.isNotEmpty()) {
            Text(
                under,
                style = quiet(MaterialTheme.typography.bodySmall),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                textAlign = if (round) TextAlign.Center else TextAlign.Start,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

/** A row of cards that scrolls sideways, inset like the rest of the page. */
@Composable
fun <T> Shelf(items: List<T>, key: (T) -> Any, card: @Composable (T) -> Unit) {
    LazyRow(
        contentPadding = PaddingValues(horizontal = Space.page),
        horizontalArrangement = Arrangement.spacedBy(Space.l),
    ) {
        items(items, key = key) { card(it) }
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
        cover()
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

/** A page's own bar: back, and what else the page offers. */
@Composable
fun PageBar(title: String, onBack: () -> Unit, actions: @Composable RowScope.() -> Unit = {}) {
    Row(Modifier.fillMaxWidth().height(56.dp).padding(horizontal = Space.xs), verticalAlignment = Alignment.CenterVertically) {
        IconButton(onClick = onBack) { Glyph(Glyphs.Back, "Back") }
        Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        actions()
    }
}

/** A tab's own title, large, with what it offers at the end. */
@Composable
fun TabTitle(title: String, actions: @Composable RowScope.() -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth().padding(start = Space.page, end = Space.xs, top = Space.l, bottom = Space.xs).height(48.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = MaterialTheme.typography.headlineMedium, modifier = Modifier.weight(1f))
        actions()
    }
}

/** What an empty list says: what it is waiting for, and what to do about it. */
@Composable
fun Empty(icon: ImageVector, title: String, sentence: String, modifier: Modifier = Modifier, action: String? = null, onAction: () -> Unit = {}) {
    Column(
        modifier.fillMaxWidth().padding(horizontal = Space.xxl, vertical = 48.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(40.dp))
        Spacer(Modifier.height(Space.l))
        Text(title, style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
        Spacer(Modifier.height(Space.s))
        Text(sentence, style = quiet(MaterialTheme.typography.bodyMedium), textAlign = TextAlign.Center)
        if (action != null) {
            Spacer(Modifier.height(Space.l))
            Button(onClick = onAction) { Text(action) }
        }
    }
}

/** How long [Fetching] waits before it says anything. */
const val FETCHING_DELAY_MS = 600L

/**
 * The library is not on this phone yet — the replica has never had the server's snapshot — and is
 * on its way from `host`: said quietly, and only after a beat, so a snapshot that lands at once
 * never shows it. A remembered launch with the library here never draws it: the splash is up until
 * Home has its lists.
 */
@Composable
fun Fetching(host: String, delayMs: Long = FETCHING_DELAY_MS) {
    var shown by remember { mutableStateOf(delayMs <= 0) }
    LaunchedEffect(Unit) {
        if (!shown) {
            delay(delayMs)
            shown = true
        }
    }
    if (!shown) return
    Column(Modifier.fillMaxWidth().padding(horizontal = Space.xxl, vertical = 48.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text("Getting your library from $host…", style = quiet(MaterialTheme.typography.bodyMedium), textAlign = TextAlign.Center)
        Spacer(Modifier.height(Space.l))
        LinearProgressIndicator(Modifier.width(96.dp), color = MaterialTheme.colorScheme.primary, trackColor = MaterialTheme.colorScheme.surfaceVariant)
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

/** A row in a sheet or a menu: a glyph and what it does. */
@Composable
fun SheetRow(icon: ImageVector, label: String, onClick: () -> Unit, trailing: @Composable () -> Unit = {}, emphasis: Boolean = false) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = Space.xl).height(52.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, contentDescription = null, tint = if (emphasis) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(Space.l))
        Text(label, style = MaterialTheme.typography.bodyLarge, fontWeight = if (emphasis) FontWeight.SemiBold else null, modifier = Modifier.weight(1f))
        trailing()
    }
}
