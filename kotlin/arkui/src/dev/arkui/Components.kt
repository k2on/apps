// The pieces a list app's screens are made of, with nothing of any app in
// them: a glyph at a control's size, a section's heading, a tab's title, a
// row that opens something, a row of a sheet, a page's bar, a shelf of
// cards and the card that opens a page out of its cover, a row's menu
// button, the sentence an empty list says, and the quiet line a library on
// its way from a server says. Drawn from plain values and the theme, so a
// screen made of them is a function of its data and can be rendered
// without a phone.
package dev.arkui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.node.CompositionLocalConsumerModifierNode
import androidx.compose.ui.node.ModifierNodeElement
import androidx.compose.ui.node.SemanticsModifierNode
import androidx.compose.ui.node.currentValueOf
import androidx.compose.ui.node.requireLayoutCoordinates
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsPropertyReceiver
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

/** A glyph, at the size a row's controls are. */
@Composable
public fun Glyph(
    icon: ImageVector,
    description: String?,
    modifier: Modifier = Modifier,
    size: Dp = 24.dp
) {
    Icon(icon, contentDescription = description, modifier = modifier.size(size))
}

/** A heading over a section, with an optional action at its end. */
@Composable
public fun Heading(
    title: String,
    modifier: Modifier = Modifier,
    action: String? = null,
    onAction: () -> Unit = {}
) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(start = Space.page, end = Space.s, top = Space.xl, bottom = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
        if (action != null) TextButton(onClick = onAction) { Text(action) }
    }
}

/** A tab's own title, large, with what it offers at the end. */
@Composable
public fun TabTitle(title: String, actions: @Composable RowScope.() -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth()
            .padding(start = Space.page, end = Space.xs, top = Space.l, bottom = Space.xs)
            .height(48.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = MaterialTheme.typography.headlineMedium, modifier = Modifier.weight(1f))
        actions()
    }
}

/** A page's own bar: back, its title, and what else the page offers. */
@Composable
public fun PageBar(
    title: String,
    onBack: () -> Unit,
    actions: @Composable RowScope.() -> Unit = {}
) {
    Row(
        Modifier.fillMaxWidth().height(56.dp).padding(horizontal = Space.xs),
        verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Glyph(ArkGlyphs.Back, "Back") }
            Text(
                title,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f))
            actions()
        }
}

/** What an empty list says: what it is waiting for, and what to do about it. */
@Composable
public fun Empty(
    icon: ImageVector,
    title: String,
    sentence: String,
    modifier: Modifier = Modifier,
    action: String? = null,
    onAction: () -> Unit = {}
) {
    Column(
        modifier.fillMaxWidth().padding(horizontal = Space.xxl, vertical = 48.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            icon,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.primary,
            modifier = Modifier.size(40.dp))
        Spacer(Modifier.height(Space.l))
        Text(title, style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
        Spacer(Modifier.height(Space.s))
        Text(
            sentence,
            style = quiet(MaterialTheme.typography.bodyMedium),
            textAlign = TextAlign.Center)
        if (action != null) {
            Spacer(Modifier.height(Space.l))
            Button(onClick = onAction) { Text(action) }
        }
    }
}

/**
 * A row of cards that scrolls sideways, inset like the rest of the page. Where it was scrolled to
 * is the destination's, as `section` ([rememberDestinationListState]): scrolled out of its list and
 * back, or left and come back to, it is where it was.
 */
@Composable
public fun <T> Shelf(
    section: String,
    items: List<T>,
    key: (T) -> Any,
    card: @Composable (T) -> Unit
) {
    LazyRow(
        state = rememberDestinationListState("shelf:$section"),
        contentPadding = PaddingValues(horizontal = Space.page),
        horizontalArrangement = Arrangement.spacedBy(Space.l),
    ) {
        items(items, key = key) { card(it) }
    }
}

/**
 * One thing on a shelf or in a grid: its cover, its name, and a line under it. One that `opens` a
 * page opens it out of its cover ([rememberHero]); one that does something else is only a card.
 */
@Composable
public fun CoverCard(
    seed: String,
    art: String,
    title: String,
    under: String,
    side: Dp,
    onClick: () -> Unit,
    round: Boolean = false,
    opens: Boolean = false
) {
    Recompositions.hit("CoverCard")
    val hero = if (opens) rememberHero(Space.corner, round = round) else null
    Column(
        Modifier.width(side)
            .clickable(onClick = if (hero == null) onClick else hero.lifting(onClick))) {
            Cover(
                seed,
                art,
                side,
                Modifier.size(side).then(hero?.modifier ?: Modifier),
                round = round)
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

/**
 * A row that opens something: its picture, its name, a line under it, and what stands at its end —
 * a chevron, unless the caller says otherwise.
 */
@Composable
public fun ListRow(
    onClick: () -> Unit,
    title: String,
    under: String?,
    trailing: @Composable () -> Unit = { Chevron() },
    picture: @Composable () -> Unit
) {
    Row(
        Modifier.fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = Space.page, vertical = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        picture()
        Spacer(Modifier.width(Space.l))
        Column(Modifier.weight(1f)) {
            Text(
                title,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis)
            if (under != null)
                Text(under, style = quiet(MaterialTheme.typography.bodyMedium), maxLines = 1)
        }
        trailing()
    }
}

/** The chevron at the end of a row that opens something. */
@Composable
public fun Chevron() {
    Icon(
        ArkGlyphs.Chevron,
        contentDescription = null,
        tint = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.size(18.dp))
}

/** A row in a sheet or a menu: a glyph and what it does. */
@Composable
public fun SheetRow(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
    trailing: @Composable () -> Unit = {},
    emphasis: Boolean = false
) {
    Row(
        Modifier.fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = Space.xl)
            .height(52.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            icon,
            contentDescription = null,
            tint =
                if (emphasis) MaterialTheme.colorScheme.primary
                else MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(Space.l))
        Text(
            label,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = if (emphasis) FontWeight.SemiBold else null,
            modifier = Modifier.weight(1f))
        trailing()
    }
}

/** How long [Fetching] waits before it says anything. */
public const val FETCHING_DELAY_MS: Long = 600L

/**
 * What the app shows is not on this phone yet and is on its way from `host`: said quietly, and only
 * after a beat, so an answer that lands at once never shows it.
 */
@Composable
public fun Fetching(host: String, delayMs: Long = FETCHING_DELAY_MS) {
    var shown by remember { mutableStateOf(delayMs <= 0) }
    LaunchedEffect(Unit) {
        if (!shown) {
            delay(delayMs)
            shown = true
        }
    }
    if (!shown) return
    Column(
        Modifier.fillMaxWidth().padding(horizontal = Space.xxl, vertical = 48.dp),
        horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                "Getting your library from $host…",
                style = quiet(MaterialTheme.typography.bodyMedium),
                textAlign = TextAlign.Center)
            Spacer(Modifier.height(Space.l))
            LinearProgressIndicator(
                Modifier.width(96.dp),
                color = MaterialTheme.colorScheme.primary,
                trackColor = MaterialTheme.colorScheme.surfaceVariant)
        }
}

// A row's menu ---------------------------------------------------------------------------------

/**
 * The menu's glyph, drawn: one painter for every row of every list, which the app's theme makes
 * once ([ArkGlyphs.More], or its own). A vector's painter is a tree of the vector's paths, and a
 * row making its own was the most a row allocated after its text.
 */
public val LocalMoreGlyph: ProvidableCompositionLocal<Painter?> = staticCompositionLocalOf { null }

/**
 * What a row says about itself as its menu opens: given the rectangle the row lays out inside its
 * margins (where the button stands), in the window, what a page the menu goes to grows out of
 * ([PageMotion.tapped]). The row's own geometry — its margins, where its picture stands — is its
 * owner's, so the owner says it; a singleton per kind of row costs a row nothing.
 */
public fun interface RowOrigin {
    public fun tapped(inner: Rect, density: Density): Tapped?
}

/**
 * A row's menu: the glyph in a 48 dp target, Material's minimum, with an unbounded ripple, and the
 * role and the label a screen reader says — "More for `about`", spelt only when one asks. What
 * `IconButton` is, less what a list pays for on every row entering it: a clip layer, a colour
 * lookup, and a layout to reach the minimum it could simply be.
 *
 * Opening the menu tells the [PageMotion] where the row is ([origin]), asked of the button's own
 * node on the tap and never before: a row entering a list pays nothing for it.
 */
@Composable
public fun MoreButton(about: String, origin: RowOrigin?, onMore: () -> Unit) {
    val at = MoreAt()
    Box(
        Modifier.size(48.dp)
            .clickable(
                interactionSource = null,
                indication = ripple(bounded = false, radius = 20.dp),
                role = Role.Button) {
                    at.tap(origin, onMore)
                }
            .then(MoreElement(about, at)),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            LocalMoreGlyph.current ?: rememberVectorPainter(ArkGlyphs.More),
            contentDescription = null,
            modifier = Modifier.size(20.dp))
    }
}

/**
 * What a row says as its menu opens: where the row is, asked of the button's own node on the tap
 * and never before, and that node is the one that already gave the button its label: a row entering
 * a list pays nothing for it (the fling bench's bytes per entering row are what they were; a node
 * of its own on the row cost about 270).
 */
private class MoreAt {
    var node: MoreNode? = null

    fun tap(origin: RowOrigin?, onMore: () -> Unit) {
        node?.let { n ->
            n.motion()?.tapped = n.inner()?.let { r -> origin?.tapped(r, n.density()) }
        }
        onMore()
    }
}

/**
 * The menu button's label for a screen reader — spelt only when one asks — and, on a tap, where it
 * is.
 */
private class MoreElement(val about: String, val at: MoreAt) : ModifierNodeElement<MoreNode>() {
    override fun create() = MoreNode(about, at)

    override fun update(node: MoreNode) {
        node.about = about
        node.at = at
        at.node = node
    }

    override fun hashCode() = about.hashCode() * 31 + at.hashCode()

    override fun equals(other: Any?) =
        other is MoreElement && other.about == about && other.at === at
}

private class MoreNode(var about: String, var at: MoreAt) :
    Modifier.Node(), SemanticsModifierNode, CompositionLocalConsumerModifierNode {
    override fun onAttach() {
        at.node = this
    }

    override fun SemanticsPropertyReceiver.applySemantics() {
        contentDescription = "More for $about"
    }

    fun motion(): PageMotion? = if (isAttached) currentValueOf(LocalPageMotion) else null

    fun density(): Density = currentValueOf(LocalDensity)

    // What the row lays out inside its margins, which is where the button stands, in the window.
    fun inner(): Rect? {
        if (!isAttached) return null
        val inner = requireLayoutCoordinates().parentLayoutCoordinates ?: return null
        return Rect(
            inner.positionInRoot(), Size(inner.size.width.toFloat(), inner.size.height.toFloat()))
    }
}
