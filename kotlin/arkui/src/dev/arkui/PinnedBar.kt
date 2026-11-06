// A page's bar, pinned over its list rather than scrolling away with it, as
// the iPhone's navigation bar is: back at its start, what the page offers at
// its end, and between them, while the page's own header is in view,
// nothing. Once the header (an album's cover and title, a person's face or
// banner and name, a show's picture) has gone up under the bar, a small copy
// of it comes up into the bar — its own art, 28 dp, and its title on one
// line — and goes back down as the header comes back: Apple's large title
// handing over to the inline one. Tapped, the copy takes the list back to
// its top.
//
// It follows where the list is, not a one-shot: whether the header is gone
// is read from the list as it scrolls, and the copy springs toward that,
// so a scroll back and forth past the fold turns it around wherever it is.
// The art and the title are one object and move as one, on `Springs.move`
// (critically damped: nothing threw them, so nothing overshoots), a rise of
// [RISE] (the theme's `rise`) and a fade, transform and alpha only. Under
// reduced motion (`reducedMotion`) they only fade, on `Springs.fade`. The
// bar's ground is decoration: it only fades, solid under the bar and then a
// soft edge into the list below it rather than a hairline, and only while
// something is under it.
//
// The list keeps its place at rest: it is given [PinnedBar.HEIGHT] of top
// padding where the bar used to be its first item, so nothing has moved
// until it scrolls. A page whose first thing is a picture under the status
// bar (`under`) is not padded; its bar sits below the status bar, its back
// on a shade over the picture until the picture has gone, and its ground
// comes in with the copy, when the picture has gone up past it.
package dev.arkui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

public object PinnedBar {
    /** How tall the bar is below the status bar: what a list under it is padded by at its top. */
    public val HEIGHT: Dp = 56.dp

    /** The side of the header's art in the bar. */
    public val ART: Dp = 28.dp

    /** The corners of a square cover at [ART]; a face is round. */
    public val CORNER: Dp = 4.dp

    /** How far the copy rises as it comes in: arkui/theme.toml's `rise`. */
    public val RISE: Dp = MotionTokens.RISE.dp

    /** How far the soft edge under the bar runs into the list. */
    public val EDGE: Dp = 16.dp

    /**
     * Whether the list's first item, its header, has gone up past `foot`, the bar's foot in the
     * list's own offsets (0 for a padded list; the status bar and the bar for one under it).
     */
    public fun gone(list: LazyListState, foot: Int): Boolean {
        val info = list.layoutInfo
        val head = info.visibleItemsInfo.firstOrNull { it.index == 0 }
        // Before its first layout, where it was left; laid out, where the header is.
        return if (head == null) list.firstVisibleItemIndex > 0 else head.offset + head.size <= foot
    }

    /** Whether anything of the list is under the bar: it has been scrolled at all. */
    public fun scrolled(list: LazyListState): Boolean =
        list.firstVisibleItemIndex > 0 || list.firstVisibleItemScrollOffset > 0
}

/**
 * A page's bar over `list`: back, then — once the list's first item, the page's header, has
 * scrolled up out of view — `art` (drawn at [PinnedBar.ART]), where the header has any, and
 * `title`, which take the list back to its top; then `actions`. Put it over the list in a `Box`, and give the list
 * `PaddingValues(top = PinnedBar.HEIGHT)` — or, for a page drawn [under] the status bar, none.
 */
@Composable
public fun PinnedBar(
    list: LazyListState,
    title: String,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    under: Boolean = false,
    art: (@Composable (Dp) -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    val density = LocalDensity.current
    val top = if (under) WindowInsets.statusBars.asPaddingValues().calculateTopPadding() else 0.dp
    val foot = with(density) { if (under) (top + PinnedBar.HEIGHT).roundToPx() else 0 }
    val still = reducedMotion()
    val scope = rememberCoroutineScope()
    val gone by remember(list, foot) { derivedStateOf { PinnedBar.gone(list, foot) } }
    // A page under the bar has its picture there until the header goes, so its ground comes with
    // the copy; any other has the list under it as soon as it scrolls.
    val edged by remember(list, foot, under) { derivedStateOf { if (under) PinnedBar.gone(list, foot) else PinnedBar.scrolled(list) } }
    val shown = animateFloatAsState(if (gone) 1f else 0f, if (still) Springs.fade() else Springs.move(), label = "compact")
    val ground = animateFloatAsState(if (edged) 1f else 0f, Springs.fade(), label = "ground")
    // Composed only while any of it shows: at rest the bar is what it always was.
    val present by remember(list, foot) { derivedStateOf { gone || shown.value > 0f } }
    val bg = MaterialTheme.colorScheme.background
    val rise = with(density) { PinnedBar.RISE.toPx() }

    Box(modifier.fillMaxWidth()) {
        // Solid under the bar, then soft into the list: no hairline.
        val solid = (top + PinnedBar.HEIGHT) / (top + PinnedBar.HEIGHT + PinnedBar.EDGE)
        Box(
            Modifier.fillMaxWidth()
                .height(top + PinnedBar.HEIGHT + PinnedBar.EDGE)
                .graphicsLayer { alpha = ground.value }
                .background(Brush.verticalGradient(0f to bg, solid to bg, 1f to bg.copy(alpha = 0f))),
        )
        Row(
            Modifier.fillMaxWidth().padding(top = top).height(PinnedBar.HEIGHT).padding(horizontal = Space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (under) ShadedBack(onBack, ground) else IconButton(onClick = onBack) { Glyph(ArkGlyphs.Back, "Back") }
            Box(Modifier.weight(1f)) {
                if (present) {
                    Row(
                        Modifier
                            .graphicsLayer {
                                val p = shown.value
                                alpha = p
                                translationY = if (still) 0f else (1f - p) * rise
                            }
                            .pressable(
                                onClick = { scope.launch { if (still) list.scrollToItem(0) else list.animateScrollToItem(0) } },
                                onClickLabel = "Scroll to top",
                                role = Role.Button,
                            )
                            .padding(vertical = Space.s, horizontal = Space.xs),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (art != null) {
                            Box(Modifier.size(PinnedBar.ART)) { art(PinnedBar.ART) }
                            Spacer(Modifier.width(Space.s))
                        }
                        Text(
                            title,
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
            actions()
        }
    }
}

// A back over a picture: light on a shade while the picture is under it, the bar's own once its
// ground has come in. Two layers cross-fading, so only alpha moves. The whole 48 dp circle is the
// button, where the banner's own back was.
@Composable
private fun ShadedBack(onBack: () -> Unit, ground: State<Float>) {
    Box(
        Modifier.size(48.dp).clip(CircleShape).pressable(onClick = onBack, role = Role.Button).semantics { contentDescription = "Back" },
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.matchParentSize().graphicsLayer { alpha = 1f - ground.value }.background(MaterialTheme.colorScheme.scrim.copy(alpha = SHADE)))
        Icon(ArkGlyphs.Back, contentDescription = null, tint = Color.White, modifier = Modifier.size(24.dp).graphicsLayer { alpha = 1f - ground.value })
        Icon(ArkGlyphs.Back, contentDescription = null, tint = MaterialTheme.colorScheme.onSurface, modifier = Modifier.size(24.dp).graphicsLayer { alpha = ground.value })
    }
}

// The shade a picture's back sits on: the scheme's scrim, as a banner's top is shaded.
private const val SHADE: Float = 0.6f

/**
 * A page that is a list under its [PinnedBar]: the list, padded where the bar is (or, [under] the
 * status bar, not), and the bar over it, its back, `actions`, and once the list's first item — the
 * page's header — has gone up under it, `title` beside `art`. What an app's page about something
 * (a record, a person, an account) is drawn in.
 */
@Composable
public fun PinnedPage(
    list: LazyListState,
    title: String,
    onBack: () -> Unit,
    art: (@Composable (Dp) -> Unit)? = null,
    under: Boolean = false,
    actions: @Composable RowScope.() -> Unit = {},
    content: LazyListScope.() -> Unit,
) {
    Box(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(top = if (under) 0.dp else PinnedBar.HEIGHT), content = content)
        PinnedBar(list, title, onBack, under = under, art = art, actions = actions)
    }
}
