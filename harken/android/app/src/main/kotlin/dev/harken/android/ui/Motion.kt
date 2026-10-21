// How a page arrives and how it goes. A page grows out of what was tapped —
// the album's cover on a shelf, a playlist's square in a list, a song's row
// when its menu went to the song's artist — to fill the screen, the art
// becoming the page's own header as it grows; the page it came from stays
// where it is and dims a little under it. Going back is the same move run
// the other way: the page shrinks into what it came from, its corners
// rounding as it does. A page with nothing to grow from (one restored after
// the process was killed, a deep link) rises a little way and fades in. The
// tabs switch at once.
//
// The back gesture previews before it commits, the way Android's own does:
// the edge swipe, or a page pulled down from its top, shrinks the page
// toward a limit — free at first, harder as it goes, never past it — with
// the page it goes back to showing, dimmed, around it. Let go past the
// threshold and the page finishes the whole shrink into its card from
// wherever the preview was; short of it, it springs back.
//
// Everything is a spring (`Springs.kt`): a page's size and clip on a
// critically damped one, where it is on one with a whisper of overshoot,
// and the dim under it on the size's.
//
// Drawn, not laid out: the page is laid out once at its full size and the
// move is a scale, an offset and a clip as it is drawn, so a page moving costs
// a frame of drawing and no measuring. Nothing about it is in the lists.
//
// Nothing here knows what a page is about: the app says which routes are its
// tabs, puts a `Hero` on the art that opens a page and a `Spot` where the
// header's art lands, and says when something over the pages (a player)
// takes the back gesture instead.
package dev.harken.android.ui

import androidx.activity.BackEventCompat
import androidx.activity.OnBackPressedDispatcher
import androidx.activity.OnBackPressedDispatcherOwner
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.EnterExitState
import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.snap
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.Stable
import androidx.compose.runtime.State
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.GraphicsLayerScope
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.node.LayoutAwareModifierNode
import androidx.compose.ui.node.ModifierNodeElement
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.lerp
import androidx.lifecycle.Lifecycle
import androidx.navigation.NavBackStackEntry
import kotlin.coroutines.cancellation.CancellationException
import kotlin.math.abs
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/** How dark the page underneath gets while a page covers it. */
public const val PAGE_DIM: Float = 0.25f

/**
 * What one navigation does: a page grows over the one it came from, shrinks back off it, or
 * neither.
 */
public enum class Move {
    Cut,
    Grow,
    Shrink,
}

/**
 * Where a page grows from: a rectangle in the window and its corner there. Art (a cover) is where
 * the page's own header art lands; anything else — a row — is a container the top of the page opens
 * out of.
 */
public class Origin(
    public val bounds: Rect,
    public val corner: Float,
    public val art: Boolean = true
) {
    override fun equals(other: Any?): Boolean =
        other is Origin && other.bounds == bounds && other.corner == corner && other.art == art

    override fun hashCode(): Int =
        (bounds.hashCode() * 31 + corner.hashCode()) * 31 + art.hashCode()

    override fun toString(): String =
        "Origin($bounds, corner $corner, ${if (art) "art" else "container"})"
}

/**
 * What a row's menu was about, as it opened: the row's art and the row, where they were in the
 * window.
 */
public class Tapped(public val art: Origin?, public val row: Origin?)

/**
 * The navigation's motion, one per session: which page is moving and which is under it, the art the
 * next page grows from, and the back gesture's preview. The move is decided by the NavHost's
 * transitions, the one place both ends of a navigation are known, and read by every page's own
 * animation as it starts. `tabs` are the routes that switch at once; every other is a page.
 */
@Stable
public class PageMotion(private val tabs: Set<String> = emptySet()) {
    /** The art the next page opens from, set by the tap and taken by the page it opens. */
    private var pending: Origin? = null

    /** By back stack entry: a page opened from art goes back into the same art. */
    private val origins = HashMap<String, Origin?>()

    /** The move under way, and the entry it moves: the other one is the page underneath. */
    public var move: Move = Move.Cut
        private set

    public var top: String? = null
        private set

    /** Frames drawn with a page part-way through a move: a test's measure of "no transition". */
    public var movingFrames: Int = 0

    /**
     * How open the moving page was last drawn, 1 when none is moving: a test's view of the move.
     */
    public var drawnAt: Float = 1f
        internal set

    /** How far the back preview had shrunk the page when it was last drawn: a test's view of it. */
    public var previewDrawn: Float = 0f
        internal set

    /** Frames drawn of a page rising with nothing to grow from: a test's view of it. */
    public var rose: Int = 0

    /** What the last page to open grew from: a test's view of it. */
    public var taken: Origin? = null
        private set

    /** What the last row whose menu opened was, and where: set by the row, read by its menu. */
    public var tapped: Tapped? = null

    public fun from(origin: Origin?) {
        pending = origin
    }

    internal fun originOf(id: String): Origin? =
        origins.getOrPut(id) {
            pending.also {
                pending = null
                taken = it
            }
        }

    /** A push: a page grows over whatever it came from; a tab arrives at once. */
    public fun push(from: NavBackStackEntry, to: NavBackStackEntry): Unit =
        decide(if (isPage(to)) Move.Grow else Move.Cut, to)

    /** A pop: a page shrinks off whatever is under it; a tab leaves at once. */
    public fun pop(from: NavBackStackEntry, to: NavBackStackEntry): Unit =
        decide(if (isPage(from)) Move.Shrink else Move.Cut, from)

    private fun decide(m: Move, moving: NavBackStackEntry) {
        move = m
        top = if (m == Move.Cut) null else moving.id
    }

    /** How a page's size, clip and dim run for the move under way: not at all, for a cut. */
    public fun size(): FiniteAnimationSpec<Float> = if (move == Move.Cut) snap() else Springs.size()

    /** How a page's place runs for the move under way. */
    public fun place(): FiniteAnimationSpec<Float> =
        if (move == Move.Cut) snap() else Springs.move()

    public fun isPage(entry: NavBackStackEntry): Boolean = entry.destination.route !in tabs

    // The back preview ---------------------------------------------------------------------------

    /**
     * How far the back gesture's preview has come, 0 to [Springs.PREVIEW]: the share of its size
     * the page sheds.
     */
    internal var preview by mutableFloatStateOf(0f)

    /**
     * Which way the preview leans: from the left edge it moves right, from the right left, and a
     * pull down moves it down.
     */
    internal var edge = BackEventCompat.EDGE_LEFT
    internal var down = false

    /** Set by a [Pull] while it is pulling: the gesture it starts is a pull's, and leans down. */
    internal var pulling = false

    /**
     * The preview as it was let go past the threshold, and the page it was of: the shrink into the
     * card starts there.
     */
    internal var released = 0f
    internal var releasedOf: String? = null

    /** Whether a back gesture is between its start and its end. */
    internal var gesturing = false

    /**
     * The spring back from a cancelled preview, which the NavHost hears about only once it is home.
     */
    internal var springing: Job? = null

    /** The activity's dispatcher, and the NavHost's own, which only this hands anything to. */
    internal var real: OnBackPressedDispatcherOwner? = null
    internal var relay: OnBackPressedDispatcherOwner? = null

    /** Whether the NavHost has anything to go back to. */
    internal var relayed by mutableStateOf(false)

    // Back from a tab's root to the first tab
    // ------------------------------------------------------

    /**
     * Whether a back from a tab's root is under way: the first tab is drawn under it while it is.
     */
    public var tabBacking: Boolean by mutableStateOf(false)
        internal set

    /**
     * How far that back's preview has come, 0 to [Springs.PREVIEW]: the share of its size the tab
     * sheds.
     */
    internal var tabPreview by mutableFloatStateOf(0f)

    /** How far the tab has gone once that back was let go past the threshold, 0 to 1. */
    internal var tabGone by mutableFloatStateOf(0f)

    /** The spring back from a cancelled tab back. */
    internal var tabSpringing: Job? = null

    /** The scale the leaving tab was last drawn at, 1 when none is: a test's view of it. */
    public var tabDrawnScale: Float = 1f
        internal set

    /** Frames drawn of a tab leaving for the first: a test's view of it. */
    public var tabFrames: Int = 0

    /**
     * The preview for page `id` drawn now: the live one while the gesture is held, its release
     * shrinking with the page after.
     */
    internal fun previewOf(id: String, shown: Float): Float =
        when {
            top != id -> 0f
            releasedOf == id -> released * shown.coerceIn(0f, 1f)
            else -> preview
        }

    public companion object {
        /** How far a page with nothing to grow from rises as it comes. */
        public val RISE: Dp = 24.dp

        /** A rising page is opaque by this much of the way: about 120 ms on the size spring. */
        public const val RISE_FADE: Float = 0.7f

        /**
         * How far the preview moves the page aside at its limit, less this: Android's own `width /
         * 20 − 8 dp`.
         */
        public val PREVIEW_INSET: Dp = 8.dp

        /** The preview's corners at its limit. */
        public val PREVIEW_CORNER: Dp = 28.dp
    }
}

public val LocalPageMotion: ProvidableCompositionLocal<PageMotion?> = staticCompositionLocalOf {
    null
}

/** The page being drawn, so its header can say where its art is. */
public val LocalPageFrame: ProvidableCompositionLocal<PageFrame?> = staticCompositionLocalOf {
    null
}

/** Something laid out, kept by where it was placed and nothing else: no callback, no allocation. */
public class Spot {
    public var coords: LayoutCoordinates? = null
        internal set

    public val modifier: Modifier = SpotElement(this)

    /** Where it is in the window, unclipped; null while it is not laid out. */
    public fun bounds(): Rect? =
        coords
            ?.takeIf { it.isAttached }
            ?.let {
                Rect(it.positionInRoot(), Size(it.size.width.toFloat(), it.size.height.toFloat()))
            }
}

private class SpotElement(val spot: Spot) : ModifierNodeElement<SpotNode>() {
    override fun create() = SpotNode(spot)

    override fun update(node: SpotNode) {
        node.spot = spot
    }

    override fun hashCode() = spot.hashCode()

    override fun equals(other: Any?) = other is SpotElement && other.spot === spot
}

private class SpotNode(var spot: Spot) : Modifier.Node(), LayoutAwareModifierNode {
    override fun onPlaced(coordinates: LayoutCoordinates) {
        spot.coords = coordinates
    }
}

/**
 * The art a tap opens a page from. Put [modifier] on the cover and call [lift] before the page
 * opens: the page grows from where the cover is.
 */
public class Hero(
    private val motion: PageMotion?,
    private val corner: Float,
    private val round: Boolean
) {
    private val spot = Spot()
    public val modifier: Modifier
        get() = spot.modifier

    public fun lift() {
        val m = motion ?: return
        m.from(spot.bounds()?.let { Origin(it, if (round) it.width / 2 else corner) })
    }

    /** A click that lifts the art, then does `then`: what a card that opens a page is given. */
    public fun lifting(then: () -> Unit): () -> Unit = {
        lift()
        then()
    }
}

@Composable
public fun rememberHero(corner: Dp, round: Boolean = false): Hero {
    val motion = LocalPageMotion.current
    val px = with(LocalDensity.current) { corner.toPx() }
    return remember(motion, px, round) { Hero(motion, px, round) }
}

/** A rounded rectangle anywhere in the layer, for a clip that is not the layer's own bounds. */
private class RoundRectAt(val rect: Rect, val radius: Float) : Shape {
    override fun createOutline(
        size: Size,
        layoutDirection: LayoutDirection,
        density: Density
    ): Outline = Outline.Rounded(RoundRect(rect, CornerRadius(radius)))
}

private fun lerp(a: Rect, b: Rect, t: Float) =
    Rect(
        lerp(a.left, b.left, t),
        lerp(a.top, b.top, t),
        lerp(a.right, b.right, t),
        lerp(a.bottom, b.bottom, t))

/**
 * Where a moving page is drawn: scaled by `s` from its corner, offset, and clipped to `clip`, said
 * in its own units.
 */
private class Placing(
    val s: Float,
    val dx: Float,
    val dy: Float,
    val clip: Rect,
    val radius: Float,
    val alpha: Float,
    val lift: Float
) {
    /** The clip as it lands on the screen. */
    val onScreen: Rect
        get() =
            Rect(dx + s * clip.left, dy + s * clip.top, dx + s * clip.right, dy + s * clip.bottom)
}

/**
 * One destination as the navigation draws it: where it is, where its header's art is, and what the
 * move under way does to it. A page that is the one moving is scaled, offset and clipped so that
 * its header's art sits exactly on the art it grew from, and opens out from there; whatever is
 * under it dims. Worked out as the page is drawn, after it is laid out: on the first frame of a
 * move the header has only just been placed, and drawing is the first moment its place is known.
 */
public class PageFrame
internal constructor(
    private val motion: PageMotion,
    private val id: String,
    private val origin: Origin?,
    private val density: Density
) {
    internal val outer = Spot()
    internal val inner = Spot()

    /** The header's art: what lands on the tapped art. */
    public val art: Spot = Spot()

    private val rise = with(density) { PageMotion.RISE.toPx() }
    private val inset = with(density) { PageMotion.PREVIEW_INSET.toPx() }
    private val previewCorner = with(density) { PageMotion.PREVIEW_CORNER.toPx() }

    // The grow: `e` is how open the page's size is, `p` where it is (the one that may overshoot).
    private fun growing(size: Size, e: Float, p: Float): Placing? {
        if (e >= 1f && p == 1f) return null
        val w = size.width
        val h = size.height
        val page = Rect(0f, 0f, w, h)
        val from =
            origin
                ?.bounds
                ?.translate(
                    -(outer.coords?.takeIf { it.isAttached }?.positionInRoot() ?: Offset.Zero))
        // Nothing to grow from: the page rises a little way and fades in.
        if (from == null) {
            motion.rose++
            return Placing(
                1f, 0f, rise * (1f - p), page, 0f, (e / PageMotion.RISE_FADE).coerceIn(0f, 1f), 0f)
        }
        // What lands on the origin: the header's own art where the page has laid it out, or,
        // scrolled past it, the top of the page; for a container, the top of the page in its shape.
        val hero =
            if (origin!!.art)
                inner.coords
                    ?.takeIf { it.isAttached }
                    ?.let { i ->
                        art.coords
                            ?.takeIf { it.isAttached }
                            ?.let { i.localBoundingBoxOf(it, clipBounds = false) }
                    }
                    ?.takeIf { it.width > 0f } ?: Rect(0f, 0f, w, w)
            else Rect(0f, 0f, w, w * from.height / from.width.coerceAtLeast(1f))
        val s0 = from.width / hero.width
        val s = lerp(s0, 1f, e)
        // The clip opens from the art to the page; its corner goes from the art's to none, said in
        // the page's own units, which the scale then shrinks.
        return Placing(
            s,
            lerp(from.left - s0 * hero.left, 0f, p),
            lerp(from.top - s0 * hero.top, 0f, p),
            lerp(hero, page, e),
            lerp(origin.corner, 0f, e) / s,
            1f,
            e * (1f - e))
    }

    // The grow, then the back preview around it: scaled about the page's middle by what the preview
    // has shed, moved aside toward where the finger is going, its corners rounded.
    private fun placing(size: Size, shown: Float, placed: Float): Placing? {
        if (motion.top != id || size.width <= 0f || size.height <= 0f) return null
        val e = shown.coerceIn(0f, 1f)
        val g = growing(size, e, placed)
        val v = motion.previewOf(id, e)
        if (v <= 0f) return g
        val q = v / Springs.PREVIEW
        val sp = 1f - v
        val aside = q * (size.width / 20f - inset).coerceAtLeast(0f)
        val tx =
            if (motion.down) 0f
            else if (motion.edge == BackEventCompat.EDGE_RIGHT) -aside else aside
        val ty = if (motion.down) q * (size.height / 20f - inset).coerceAtLeast(0f) else 0f
        val s = g?.s ?: 1f
        val big = sp * s
        return Placing(
            big,
            size.width / 2f * (1f - sp) + sp * (g?.dx ?: 0f) + tx,
            size.height / 2f * (1f - sp) + sp * (g?.dy ?: 0f) + ty,
            g?.clip ?: Rect(0f, 0f, size.width, size.height),
            maxOf(g?.radius ?: 0f, q * previewCorner / big),
            g?.alpha ?: 1f,
            maxOf(g?.lift ?: 0f, q / 3f),
        )
    }

    // The layer: what can only be a layer's, a shadow under the moving card — lifted while it
    // moves and flat at both ends, so its first frame casts none — and, as it rises, its alpha.
    internal fun layer(g: GraphicsLayerScope, shown: Float, placed: Float) {
        val m = placing(g.size, shown, placed) ?: return
        with(g) {
            alpha = m.alpha
            shape = RoundRectAt(m.onScreen, m.radius * m.s)
            shadowElevation = 24f * m.lift * density
        }
    }

    internal fun draw(
        d: ContentDrawScope,
        shown: Float,
        placed: Float,
        leaving: Boolean,
        scrim: Color
    ) =
        with(d) {
            if (motion.top == id) {
                motion.drawnAt = shown.coerceIn(0f, 1f)
                motion.previewDrawn = motion.previewOf(id, shown)
            }
            // A tab left at once is not drawn for the frame it is still there: what arrives is.
            if (motion.move == Move.Cut && leaving) return
            val m = placing(size, shown, placed)
            if (m == null) drawContent()
            else {
                motion.movingFrames++
                withTransform({
                    translate(m.dx, m.dy)
                    scale(m.s, m.s, Offset.Zero)
                }) {
                    clipPath(
                        Path().apply { addRoundRect(RoundRect(m.clip, CornerRadius(m.radius))) }) {
                            this@with.drawContent()
                        }
                }
            }
            val dim = dim(shown)
            if (dim > 0f) {
                motion.movingFrames++
                drawRect(scrim, alpha = dim)
            }
        }

    /** How much darker this destination is drawn: only while another page moves over it. */
    private fun dim(shown: Float): Float {
        if (motion.move == Move.Cut || motion.top == id || shown >= 1f) return 0f
        return PAGE_DIM * (1f - shown.coerceIn(0f, 1f))
    }
}

/**
 * One destination of the NavHost, drawn in the navigation's motion. A page (anything that is not a
 * tab) grows, shrinks and is pulled down to go back; a tab only dims under a page. Both are opaque,
 * so that whatever moves over them hides them.
 */
@Composable
public fun AnimatedVisibilityScope.Moving(
    motion: PageMotion,
    entry: NavBackStackEntry,
    content: @Composable () -> Unit
) {
    val page = motion.isPage(entry)
    // Two numbers per destination, 1 while it is the page shown and 0 while it is not: how open its
    // size is, and where it is. A page that is the one moving is drawn as open as they say; one
    // under
    // a page is darker by what the size lacks. They are the navigation's own transition, so
    // whatever
    // runs or seeks the transition runs them.
    val shown =
        transition.animateFloat({ motion.size() }, label = "shown") {
            if (it == EnterExitState.Visible) 1f else 0f
        }
    val placed: State<Float>? =
        if (page)
            transition.animateFloat({ motion.place() }, label = "placed") {
                if (it == EnterExitState.Visible) 1f else 0f
            }
        else null
    val transition = transition
    val density = LocalDensity.current
    val frame =
        remember(entry.id) {
            PageFrame(motion, entry.id, if (page) motion.originOf(entry.id) else null, density)
        }
    val real = motion.real
    val pull = remember(real) { real?.let { Pull(it.onBackPressedDispatcher, motion) } }
    val scrim = MaterialTheme.colorScheme.scrim
    Box(
        Modifier.fillMaxSize()
            .then(frame.outer.modifier)
            .then(
                if (page && pull != null)
                    Modifier.pointerInput(pull) { pull.watch(this) }.nestedScroll(pull)
                else Modifier)
            .then(
                if (page) Modifier.graphicsLayer { frame.layer(this, shown.value, placed!!.value) }
                else Modifier)
            .drawWithContent {
                frame.draw(
                    this,
                    shown.value,
                    placed?.value ?: 1f,
                    transition.targetState != EnterExitState.Visible,
                    scrim)
            }
            .background(MaterialTheme.colorScheme.background)
            .then(frame.inner.modifier)) {
            // One call, whatever the destination is and wherever it is drawn: what differs is
            // only what is handed to it (`Destination.kt`). The page's own back — a pull, a
            // sheet's — is the activity's, not the NavHost's.
            val locals =
                listOfNotNull(
                    LocalPageFrame provides (if (page) frame else null),
                    real?.let { LocalOnBackPressedDispatcherOwner provides it },
                )
            CompositionLocalProvider(*locals.toTypedArray()) { content() }
        }
}

/**
 * The back gesture, for the pages: the preview it draws, and when the NavHost hears about it. Put
 * it where it is composed before anything that takes back ahead of the pages — a player over them —
 * so that registers after it and is asked first, and say [blocked] while such a thing is up; put
 * [Relayed] around the NavHost.
 *
 * At the root of a tab that is not the first, back goes to the first (`toFirst`), previewed the
 * same way: the tab sheds its size toward the limit with the first tab, dimmed, behind it
 * ([TabStack] draws both), and let go past the threshold it shrinks a little further and fades out
 * on [Springs.fade] before the first tab is the one shown; short of it, it springs back. At the
 * first tab's root nothing here takes back, so the system's own back runs.
 *
 * The NavHost would seek its transition along the gesture's progress, which is the page following
 * the finger all the way into its card. It is handed a dispatcher of its own instead, and told only
 * that a gesture began (at progress 0, so the page it goes back to is composed and shows under the
 * preview), and then either that it finished — and the page shrinks the rest of the way into its
 * card on its spring, from the preview — or, once the preview has sprung back, that it did not.
 */
@Composable
public fun PageBack(
    motion: PageMotion,
    blocked: () -> Boolean = { false },
    toFirst: (() -> Unit)? = null,
) {
    val owner = LocalOnBackPressedDispatcherOwner.current ?: return
    val relay =
        remember(owner) {
            Relay(owner.lifecycle) { motion.relayed = it }
                .also { motion.relayed = it.onBackPressedDispatcher.hasEnabledCallbacks() }
        }
    motion.real = owner
    motion.relay = relay
    val off by remember { derivedStateOf(blocked) }
    val scope = rememberCoroutineScope()
    // At a tab's root the NavHost has nothing to go back to; at the first tab's, nothing here
    // takes back at all, and the system's own back runs.
    PredictiveBackHandler(enabled = (motion.relayed || toFirst != null) && !off) { events ->
        val first = toFirst
        if (!motion.relayed && first != null) {
            motion.backToFirst(events, scope, first)
            return@PredictiveBackHandler
        }
        val to = relay.onBackPressedDispatcher
        var started = false
        try {
            events.collect { e ->
                if (!started) {
                    started = true
                    motion.begin(e, to)
                }
                motion.edge = e.swipeEdge
                motion.preview = Springs.preview(e.progress)
            }
            if (started) motion.release()
            motion.gesturing = false
            to.onBackPressed()
        } catch (c: CancellationException) {
            if (started) motion.springBack(scope, to)
            throw c
        }
    }
}

// Back from a tab's root: the tab previews as a page does, with the first tab under it, and let go
// past the threshold it shrinks a little further and fades, and the first tab is the one shown.
private suspend fun PageMotion.backToFirst(
    events: kotlinx.coroutines.flow.Flow<BackEventCompat>,
    scope: CoroutineScope,
    first: () -> Unit,
) {
    val back = tabSpringing
    tabSpringing = null
    back?.cancel()
    tabBacking = true
    tabGone = 0f
    try {
        events.collect { e ->
            edge = e.swipeEdge
            tabPreview = Springs.preview(e.progress)
        }
    } catch (c: CancellationException) {
        tabSpringing =
            scope.launch {
                animate(tabPreview, 0f, animationSpec = Springs.size()) { v, _ -> tabPreview = v }
                tabBacking = false
                tabDrawnScale = 1f
            }
        throw c
    }
    // Not in the handler's own coroutine: going to the first tab stands the handler down.
    scope.launch {
        animate(0f, 1f, animationSpec = Springs.fade()) { v, _ -> tabGone = v }
        first()
        tabBacking = false
        tabPreview = 0f
        tabGone = 0f
        tabDrawnScale = 1f
    }
}

/**
 * The NavHost, handed the dispatcher [PageBack] relays to: the only one it hears the back gesture
 * from.
 */
@Composable
public fun Relayed(motion: PageMotion, content: @Composable () -> Unit) {
    val relay = motion.relay
    if (relay == null) content()
    else CompositionLocalProvider(LocalOnBackPressedDispatcherOwner provides relay) { content() }
}

private class Relay(override val lifecycle: Lifecycle, changed: (Boolean) -> Unit) :
    OnBackPressedDispatcherOwner {
    override val onBackPressedDispatcher = OnBackPressedDispatcher(null) { changed(it) }
}

private fun PageMotion.begin(e: BackEventCompat, to: OnBackPressedDispatcher) {
    releasedOf = null
    gesturing = true
    down = pulling
    val back = springing
    springing = null
    // Caught on its way back: the NavHost was never told it ended, so it is the same gesture still.
    if (back != null && back.isActive) {
        back.cancel()
        return
    }
    val start = BackEventCompat(e.touchX, e.touchY, 0f, e.swipeEdge)
    to.dispatchOnBackStarted(start)
    to.dispatchOnBackProgressed(start)
}

private fun PageMotion.release() {
    released = preview
    releasedOf = top
    preview = 0f
}

private fun PageMotion.springBack(scope: CoroutineScope, to: OnBackPressedDispatcher) {
    gesturing = false
    springing =
        scope.launch {
            animate(preview, 0f, animationSpec = Springs.size()) { v, _ -> preview = v }
            to.dispatchOnBackCancelled()
        }
}

/**
 * A page pulled down from its top goes back, the way the back gesture takes it: the pull is said to
 * the activity's back dispatcher as a back gesture's progress, so the page previews under the
 * finger exactly as it does for the edge swipe, and letting go far enough — or fast enough —
 * finishes it; short of that it springs back. Only once the page's list is at its top and has
 * nothing more to give, only for a downward drag that is mostly vertical, and never for a fling
 * that reaches the top on its own.
 */
public class Pull(
    private val back: OnBackPressedDispatcher,
    private val motion: PageMotion? = null
) : NestedScrollConnection {
    private var down = Offset.Zero
    private var now = Offset.Zero
    private var pulling = false
    private var pulled = 0f
    private var height = 1f

    /** How far the pull has come, 0 to just short of 1, as the back gesture's progress. */
    public val progress: Float
        get() = if (pulling) (pulled / (height * REACH)).coerceIn(0f, 0.99f) else 0f

    // Where the finger went down and where it is: the angle of the drag so far. Watched on the
    // way in, consuming nothing, so the list scrolls exactly as it did.
    internal suspend fun watch(scope: PointerInputScope) {
        height = scope.size.height.toFloat().coerceAtLeast(1f)
        scope.awaitEachGesture {
            down =
                awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial).position
            now = down
            while (true) {
                val e = awaitPointerEvent(PointerEventPass.Initial)
                val c = e.changes.firstOrNull() ?: break
                now = c.position
                if (!c.pressed) break
            }
        }
    }

    private val vertical: Boolean
        get() = (now.y - down.y) > 0f && abs(now.y - down.y) >= 2f * abs(now.x - down.x)

    private fun event() = BackEventCompat(now.x, now.y, progress, BackEventCompat.EDGE_LEFT)

    override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
        if (!pulling || source != NestedScrollSource.UserInput) return Offset.Zero
        // Mid-pull, every vertical move is the pull's: up gives it back, and the list stays put.
        pulled = (pulled + available.y).coerceAtLeast(0f)
        back.dispatchOnBackProgressed(event())
        return Offset(0f, available.y)
    }

    override fun onPostScroll(
        consumed: Offset,
        available: Offset,
        source: NestedScrollSource
    ): Offset {
        if (pulling || source != NestedScrollSource.UserInput || available.y <= 0f || !vertical)
            return Offset.Zero
        pulling = true
        motion?.pulling = true
        pulled = available.y
        back.dispatchOnBackStarted(BackEventCompat(now.x, now.y, 0f, BackEventCompat.EDGE_LEFT))
        back.dispatchOnBackProgressed(event())
        return Offset(0f, available.y)
    }

    override suspend fun onPreFling(available: Velocity): Velocity {
        if (!pulling) return Velocity.Zero
        val far = progress >= COMMIT || available.y >= FLICK * height
        pulling = false
        pulled = 0f
        if (far) back.onBackPressed() else back.dispatchOnBackCancelled()
        motion?.pulling = false
        // The fling was the pull's: the list does not go on to scroll with it.
        return available
    }

    private companion object {
        /** How far down, as a share of the page's height, a pull runs for a progress of 1. */
        const val REACH = 0.6f

        /** A pull let go past this much of the way goes back. */
        const val COMMIT = 0.2f

        /** Or one let go moving down faster than this many page heights a second. */
        const val FLICK = 1.2f
    }
}
