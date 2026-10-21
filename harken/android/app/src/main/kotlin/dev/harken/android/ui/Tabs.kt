// Tabs, the way a phone's tab bar behaves: each tab keeps its own stack, a
// tab switch is instant whatever is on that stack, the tab you are already
// on goes back to its root — or, at its root, scrolls its list to the top —
// and back at the root of any tab but the first goes to the first, with the
// back gesture's own preview. Material's bottom-navigation guidance, and
// what iOS's tab bars, YouTube and Instagram do.
//
// Each tab is its own NavHost over its own NavController (`TabStack`): a
// switch is not a navigation, so no NavHost transition runs and no page
// grows — what was on that tab's stack is simply there again, scrolled where
// it was, because each tab's state is saved while it is not shown
// (`rememberSaveableStateHolder`). Only the tab shown is composed, except
// while back from a tab's root is under way: then the first tab is drawn
// under it, dimmed, and the tab leaving is drawn in the page preview's
// shape — smaller, aside, its corners rounded — and, let go, a little
// smaller still and fading.
package dev.harken.android.ui

import androidx.activity.BackEventCompat
import androidx.activity.OnBackPressedDispatcher
import androidx.activity.OnBackPressedDispatcherOwner
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.movableContentOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlinx.coroutines.flow.drop

/**
 * A tab's root, asked to go to its top: the tab bar's item tapped while the tab is shown and at its
 * root. [ScrollsToTop] on a list answers it.
 */
@Stable
public class Reselect {
    internal var asked by mutableIntStateOf(0)

    /** Ask the tab's root to go to its top. */
    public fun ask() {
        asked++
    }
}

/** The [Reselect] of the tab being drawn, or none outside a [TabStack]. */
public val LocalReselect: ProvidableCompositionLocal<Reselect?> = staticCompositionLocalOf { null }

/** Scroll `state` to its top, quickly, each time the tab it is in is reselected at its root. */
@Composable
public fun ScrollsToTop(state: LazyListState) {
    val r = LocalReselect.current ?: return
    LaunchedEffect(r, state) {
        snapshotFlow { r.asked }.drop(1).collect { state.animateScrollToItem(0) }
    }
}

/** …and a grid. */
@Composable
public fun ScrollsToTop(state: LazyGridState) {
    val r = LocalReselect.current ?: return
    LaunchedEffect(r, state) {
        snapshotFlow { r.asked }.drop(1).collect { state.animateScrollToItem(0) }
    }
}

/**
 * The tabs' content: `content(tab)` for the tab shown, its state kept while another is, and its
 * [Reselect] in [LocalReselect]. `first` is the tab back goes to from any other tab's root
 * ([PageBack]'s `toFirst`); while that back is under way, it is drawn under the tab shown.
 *
 * **A tab is one composition wherever it is drawn.** In front, under the tab leaving, and in front
 * again when that back lands, a tab's content is the same movable content, called from the same
 * place with the same wrappers: only the values handed to it differ — which back dispatcher it
 * hears and which lifecycle it sees. So the first tab under a back preview is the composition that
 * becomes the tab shown when it lands, not a second one, and what it shows under the preview —
 * every list where it was left — is what it shows after. The tab shown hears the back gesture
 * through [Relayed]'s dispatcher; the first tab, drawn under it, hears nothing — its NavHost has a
 * dispatcher of its own that nothing dispatches to.
 *
 * **And each tab has a lifecycle of its own**, under the one this is drawn in: resumed while it is
 * the tab shown and nothing is moving, started while back from a root is under way (the tab leaving
 * and the first tab under it alike: both drawn, neither taking a touch), and created while it is
 * not drawn. A tab's NavHost hands it on to its entries, so a destination's content —
 * `collectAsStateWithLifecycle`, `LifecycleResumeEffect` — knows whether it is in front.
 */
@Composable
public fun <T : Any> TabStack(
    motion: PageMotion,
    shown: T,
    first: T,
    reselect: (T) -> Reselect,
    content: @Composable (T) -> Unit,
) {
    val holder = rememberSaveableStateHolder()
    val tabs = if (motion.tabBacking && shown != first) listOf(first, shown) else listOf(shown)
    val density = LocalDensity.current
    val inset = with(density) { PageMotion.PREVIEW_INSET.toPx() }
    val corner = with(density) { PageMotion.PREVIEW_CORNER.toPx() }
    val scrim = MaterialTheme.colorScheme.scrim
    val parent = LocalLifecycleOwner.current
    val deaf = remember(parent) { Deaf(parent.lifecycle) }
    val heard = LocalOnBackPressedDispatcherOwner.current
    val latest by rememberUpdatedState(content)
    // One movable content and one lifecycle per tab, for as long as this is composed.
    val bodies = remember { HashMap<T, @Composable () -> Unit>() }
    val lifecycles = remember(parent) { HashMap<T, CappedLifecycle>() }
    val backing = motion.tabBacking
    fun capOf(t: T): Lifecycle.State =
        when {
            t !in tabs -> Lifecycle.State.CREATED
            backing -> Lifecycle.State.STARTED
            else -> Lifecycle.State.RESUMED
        }
    SideEffect { for ((t, l) in lifecycles) l.cap = capOf(t) }
    DisposableEffect(lifecycles) { onDispose { for (l in lifecycles.values) l.stop() } }
    Box(Modifier.fillMaxSize()) {
        for (t in tabs) {
            key(t) {
                val front = t == shown
                val life = lifecycles.getOrPut(t) { CappedLifecycle(parent.lifecycle, capOf(t)) }
                DisposableEffect(life) {
                    life.start()
                    onDispose { life.stop() }
                }
                val layer =
                    if (!front) {
                        // The first tab, under the one leaving: dimmed as a page's underneath is,
                        // and brighter as the leaving one goes.
                        Modifier.fillMaxSize().drawWithContent {
                            drawContent()
                            drawRect(scrim, alpha = PAGE_DIM * (1f - motion.tabGone))
                        }
                    } else if (!motion.tabBacking) Modifier.fillMaxSize()
                    else
                    // A layer only while it is leaving: at rest the tab is drawn as it always was.
                    Modifier.fillMaxSize().graphicsLayer {
                            val v = motion.tabPreview
                            val c = motion.tabGone
                            if (v <= 0f && c <= 0f) {
                                motion.tabDrawnScale = 1f
                                return@graphicsLayer
                            }
                            motion.tabFrames++
                            val q = v / Springs.PREVIEW
                            val s = 1f - v - TAB_GOING * c
                            motion.tabDrawnScale = s
                            val aside = q * (size.width / 20f - inset).coerceAtLeast(0f)
                            scaleX = s
                            scaleY = s
                            translationX =
                                if (motion.edge == BackEventCompat.EDGE_RIGHT) -aside else aside
                            shape = RoundedCornerShape(q * corner / s)
                            clip = true
                            shadowElevation = 24f * (q / 3f) * density.density
                            alpha = 1f - c
                        }
                // What the tab hears back from: one call, whichever it is.
                val ears: OnBackPressedDispatcherOwner? = if (front) motion.relay ?: heard else deaf
                val locals =
                    listOfNotNull(
                        LocalReselect provides reselect(t),
                        LocalLifecycleOwner provides life,
                        ears?.let { LocalOnBackPressedDispatcherOwner provides it },
                    )
                Box(layer) {
                    CompositionLocalProvider(*locals.toTypedArray()) {
                        bodies.getOrPut(t) {
                            movableContentOf { holder.SaveableStateProvider(t) { latest(t) } }
                        }()
                    }
                }
            }
        }
    }
}

/** How much more of its size a tab sheds as it goes, past where its preview had it. */
private const val TAB_GOING = 0.05f

/**
 * A dispatcher nothing dispatches to: the first tab's NavHost, drawn under a tab leaving, hears no
 * back.
 */
private class Deaf(override val lifecycle: androidx.lifecycle.Lifecycle) :
    OnBackPressedDispatcherOwner {
    override val onBackPressedDispatcher = OnBackPressedDispatcher(null)
}
