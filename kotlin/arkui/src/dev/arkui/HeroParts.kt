// A page about somebody, the way a record shop's catalogue opens one — the
// phone's twin of `rust/arkui/src/hero.rs` (`hero::banner`, `heading`,
// `entry`, `more`) and of SwiftUI's `HeroBanner`, `HeroHeading`,
// `HeroEntry` and `HeroMore`: their wide picture across the top with their
// name set on it, the page under it in sections each under a bold heading,
// a listing's entries over their hairlines, and the line that opens the
// rest of a list.
package dev.arkui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** A banner's measures, the desktop's (`hero::ASPECT`, `hero::TALLEST`). */
public object HeroBanner {
    /** The tallest a banner is drawn: its whole shape on a page about 1250 wide. */
    public val TALLEST: Dp = 486.dp

    /** How far below the status bar the shade at a banner's top runs: past its back. */
    public val TOP_SHADE: Dp = 72.dp

    /** A banner's shape, width over height: a painting cut for one (2400×933). */
    public const val ASPECT: Float = 2400f / 933f

    /**
     * How tall a banner `width` wide is drawn on a phone. Its own shape would be a strip there
     * (about 150 dp), so a phone crops it to a taller frame, as a catalogue's phone app does, the
     * name on its lower edge; never past [TALLEST].
     */
    public fun height(width: Dp): Dp = maxOf(width / ASPECT, width * 0.82f).coerceIn(200.dp, TALLEST)
}

/**
 * Somebody's banner, edge to edge and up to the top of the screen: their wide picture ([art], the
 * derived square of [seed] until it arrives) cropped to fill, run up under the status bar (its
 * page says [UnderStatusBar]) and that much taller, so what shows below the bar is the shape
 * [HeroBanner.height] says; [kind] in small type and [name] in bold on its lower edge over a
 * shade that keeps them legible, and a lighter one under the bar's icons and the page's back. A
 * page grows out of a portrait into it, the part under the bar included.
 */
@Composable
public fun HeroBanner(seed: String, art: String, kind: String, name: String) {
    val bar = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    // Words on a picture are white over a shade of the theme's scrim, whatever the theme: the
    // desktop's banner draws them the same.
    val shade = MaterialTheme.colorScheme.scrim
    val clear = shade.copy(alpha = 0f)
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val w = maxWidth
        Box(Modifier.fillMaxWidth().height(bar + HeroBanner.height(w)).then(LocalPageFrame.current?.art?.modifier ?: Modifier)) {
            Cover(seed, art, w, Modifier.fillMaxSize(), corner = 0.dp)
            // Clear at the top, darkest at the foot, where the name is.
            Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0f to clear, 0.45f to clear, 1f to shade.copy(alpha = 0.6f))))
            // And a little shade at the very top, under the bar's icons and back.
            Box(Modifier.fillMaxWidth().height(bar + HeroBanner.TOP_SHADE).background(Brush.verticalGradient(0f to shade.copy(alpha = 0.35f), 1f to clear)))
            Column(Modifier.align(Alignment.BottomStart).padding(start = Space.page, end = Space.page, bottom = Space.xl)) {
                Text(kind, style = MaterialTheme.typography.labelMedium, color = Color.White.copy(alpha = 0.85f))
                Spacer(Modifier.height(Space.xs))
                Text(name, style = MaterialTheme.typography.headlineLarge, color = Color.White)
            }
        }
    }
}

/** A section's heading on a page about somebody, bold. */
@Composable
public fun HeroHeading(title: String, modifier: Modifier = Modifier) {
    Text(title, style = MaterialTheme.typography.headlineSmall, modifier = modifier.fillMaxWidth().padding(start = Space.page, end = Space.page, top = Space.xl, bottom = Space.s))
}

/**
 * One entry of a listing over its hairline: a [picture] at its left when it has one (handed the
 * modifier a page grows out of), a line of small type over its title ([over], who it is by), a
 * line under, a count at its right, and the chevron. The whole entry opens it.
 */
@Composable
public fun HeroEntry(title: String, over: String, under: String, count: String, onOpen: () -> Unit, picture: (@Composable (Modifier) -> Unit)? = null) {
    val hero = if (picture != null) rememberHero(4.dp) else null
    Column(Modifier.fillMaxWidth().pressable(onClick = hero?.lifting(onOpen) ?: onOpen, press = Press.Highlight).padding(start = Space.page)) {
        Row(Modifier.fillMaxWidth().padding(end = Space.m, top = Space.m, bottom = Space.m), verticalAlignment = Alignment.CenterVertically) {
            if (picture != null && hero != null) {
                picture(hero.modifier)
                Spacer(Modifier.width(Space.m))
            }
            Column(Modifier.weight(1f)) {
                if (over.isNotEmpty()) Text(over, style = quiet(MaterialTheme.typography.labelSmall), maxLines = 1)
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
public fun HeroMore(label: String, onOpen: () -> Unit) {
    Row(Modifier.fillMaxWidth().pressable(onClick = onOpen, press = Press.Highlight).padding(start = Space.page, end = Space.m, top = Space.m, bottom = Space.m), verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.primary, modifier = Modifier.weight(1f))
        Chevron()
    }
}
