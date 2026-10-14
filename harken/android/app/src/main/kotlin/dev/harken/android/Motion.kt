// How a page arrives and how it goes. A page grows out of the art that was
// tapped — the album's cover on a shelf, a playlist's square in a list — to
// fill the screen, the art becoming the page's own header as it grows; the
// page it came from stays where it is and dims a little under it. Going
// back is the same move run the other way: the page shrinks into its card,
// its corners rounding as it does. A page opened from no art at all (an
// artist from a row's menu, settings) grows from the middle of the screen
// in the same way. The three tabs switch at once.
//
// It is the navigation's own transition, so everything that seeks it moves
// it: the system's back gesture on Android 14 and later, which follows the
// finger with the page underneath showing, and a page pulled down from its
// top, which says the same thing to the same dispatcher. One transition
// serves both gestures and the back button.
//
// Drawn, not laid out: the page is laid out once at its full size and the
// move is a scale, an offset and a clip as it is drawn, so a page moving costs
// a frame of drawing and no measuring. Nothing about it is in the lists.
package dev.harken.android

import androidx.activity.BackEventCompat
import androidx.activity.OnBackPressedDispatcher
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.EnterExitState
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Stable
import androidx.compose.runtime.remember
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
import androidx.navigation.NavBackStackEntry
import kotlin.math.abs

/** How long a page takes to grow or shrink, when nothing is holding it. */
const val PAGE_MS = 400

/** How dark the page underneath gets while a page covers it. */
const val PAGE_DIM = 0.25f

/**
 * The curve a page moves on: close to in step with its progress for the first third, so a back
 * gesture that seeks it moves the page with the finger, then slowing into place, so a page opened
 * by a tap lands rather than stops.
 */
val Settle = CubicBezierEasing(0.5f, 0.5f, 0.2f, 1f)

/** What one navigation does: a page grows over the one it came from, shrinks back off it, or neither. */
enum class Move {
    Cut,
    Grow,
    Shrink,
}

/** Where a page grows from: the tapped art's bounds in the window, and its corner there. */
class Origin(val bounds: Rect, val corner: Float)

/**
 * The navigation's motion, one per session: which page is moving and which is under it, and the art
 * the next page grows from. The move is decided by the NavHost's transitions, the one place both
 * ends of a navigation are known, and read by every page's own animation as it starts.
 */
@Stable
class PageMotion {
    /** The art the next page opens from, set by the tap and taken by the page it opens. */
    private var pending: Origin? = null

    /** By back stack entry: a page opened from art goes back into the same art. */
    private val origins = HashMap<String, Origin?>()

    /** The move under way, and the entry it moves: the other one is the page underneath. */
    var move = Move.Cut
        private set

    var top: String? = null
        private set

    /** Frames drawn with a page part-way through a move: a test's measure of "no transition". */
    var movingFrames = 0

    /** How open the moving page was last drawn, 1 when none is moving: a test's view of the move. */
    var drawnAt = 1f
        internal set

    fun from(origin: Origin?) {
        pending = origin
    }

    internal fun originOf(id: String): Origin? = origins.getOrPut(id) { pending.also { pending = null } }

    /** A push: a page grows over whatever it came from; a tab arrives at once. */
    fun push(from: NavBackStackEntry, to: NavBackStackEntry) = decide(if (isPage(to)) Move.Grow else Move.Cut, to)

    /** A pop: a page shrinks off whatever is under it; a tab leaves at once. */
    fun pop(from: NavBackStackEntry, to: NavBackStackEntry) = decide(if (isPage(from)) Move.Shrink else Move.Cut, from)

    private fun decide(m: Move, moving: NavBackStackEntry) {
        move = m
        top = if (m == Move.Cut) null else moving.id
    }

    /** How a page's animation runs for the move under way: not at all, for a cut. */
    fun <T> spec(): FiniteAnimationSpec<T> = if (move == Move.Cut) snap() else tween(PAGE_MS, easing = Settle)

    companion object {
        fun isPage(entry: NavBackStackEntry): Boolean = Tab.entries.none { it.route == entry.destination.route }
    }
}

val LocalPageMotion = staticCompositionLocalOf<PageMotion?> { null }

/** The page being drawn, so its header can say where its art is. */
val LocalPageFrame = staticCompositionLocalOf<PageFrame?> { null }

/** Something laid out, kept by where it was placed and nothing else: no callback, no allocation. */
class Spot {
    var coords: LayoutCoordinates? = null
        internal set

    val modifier: Modifier = SpotElement(this)

    /** Where it is in the window, unclipped; null while it is not laid out. */
    fun bounds(): Rect? = coords?.takeIf { it.isAttached }?.let { Rect(it.positionInRoot(), Size(it.size.width.toFloat(), it.size.height.toFloat())) }
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
 * The art a tap opens a page from. Put [modifier] on the cover and call [lift] before the page opens:
 * the page grows from where the cover is.
 */
class Hero(private val motion: PageMotion?, private val corner: Float, private val round: Boolean) {
    private val spot = Spot()
    val modifier: Modifier
        get() = spot.modifier

    fun lift() {
        val m = motion ?: return
        m.from(spot.bounds()?.let { Origin(it, if (round) it.width / 2 else corner) })
    }

    /** A click that lifts the art, then does `then`: what a card that opens a page is given. */
    fun lifting(then: () -> Unit): () -> Unit = {
        lift()
        then()
    }
}

@Composable
fun rememberHero(corner: Dp = Space.corner, round: Boolean = false): Hero {
    val motion = LocalPageMotion.current
    val px = with(LocalDensity.current) { corner.toPx() }
    return remember(motion, px, round) { Hero(motion, px, round) }
}

/** A rounded rectangle anywhere in the layer, for a clip that is not the layer's own bounds. */
private class RoundRectAt(val rect: Rect, val radius: Float) : Shape {
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline =
        Outline.Rounded(RoundRect(rect, CornerRadius(radius)))
}

private fun lerp(a: Rect, b: Rect, t: Float) = Rect(lerp(a.left, b.left, t), lerp(a.top, b.top, t), lerp(a.right, b.right, t), lerp(a.bottom, b.bottom, t))

/** Where a moving page is drawn: scaled by `s` from its corner, offset, and clipped to `clip`, said in its own units. */
private class Placing(val s: Float, val dx: Float, val dy: Float, val clip: Rect, val radius: Float, val alpha: Float) {
    /** The clip as it lands on the screen. */
    val onScreen: Rect
        get() = Rect(dx + s * clip.left, dy + s * clip.top, dx + s * clip.right, dy + s * clip.bottom)
}

/**
 * One destination as the navigation draws it: where it is, where its header's art is, and what the
 * move under way does to it. A page that is the one moving is scaled, offset and clipped so that its
 * header's art sits exactly on the art it grew from, and opens out from there; whatever is under it
 * dims. Worked out as the page is drawn, after it is laid out: on the first frame of a move the
 * header has only just been placed, and drawing is the first moment its place is known.
 */
class PageFrame internal constructor(private val motion: PageMotion, private val id: String, private val origin: Origin?, private val centreCorner: Float) {
    internal val outer = Spot()
    internal val inner = Spot()

    /** The header's art: what lands on the tapped art. */
    val art = Spot()

    private fun placing(size: Size, shown: Float): Placing? {
        if (motion.top != id) return null
        val e = shown.coerceIn(0f, 1f)
        if (e >= 1f || size.width <= 0f || size.height <= 0f) return null
        val w = size.width
        val h = size.height
        val page = Rect(0f, 0f, w, h)
        val at = outer.coords?.takeIf { it.isAttached }?.positionInRoot() ?: Offset.Zero
        val from = origin?.bounds?.translate(-at)
        // What lands on the art: the header's own art where the page has laid it out, or, scrolled
        // past it, the top of the page. Without any art to land on, the whole page, from the middle.
        val hero =
            if (from == null) page
            else
                inner.coords?.takeIf { it.isAttached }?.let { i ->
                    art.coords?.takeIf { it.isAttached }?.let { i.localBoundingBoxOf(it, clipBounds = false) }
                }?.takeIf { it.width > 0f } ?: Rect(0f, 0f, w, w)
        val o = from ?: Rect(page.center - Offset(w, h) * (CENTRE / 2), Size(w * CENTRE, h * CENTRE))
        val s0 = o.width / hero.width
        val s = lerp(s0, 1f, e)
        // The clip opens from the art to the page; its corner goes from the art's to none, said in
        // the page's own units, which the scale then shrinks.
        return Placing(
            s,
            lerp(o.left - s0 * hero.left, 0f, e),
            lerp(o.top - s0 * hero.top, 0f, e),
            lerp(if (from == null) page else hero, page, e),
            lerp(origin?.corner ?: centreCorner, 0f, e) / s,
            if (from == null) (e / 0.35f).coerceIn(0f, 1f) else 1f,
        )
    }

    // The layer: what can only be a layer's, a shadow under the moving card — lifted while it
    // moves and flat at both ends, so its first frame casts none — and, from the middle, its alpha.
    internal fun layer(g: GraphicsLayerScope, shown: Float) {
        val m = placing(g.size, shown) ?: return
        val e = shown.coerceIn(0f, 1f)
        with(g) {
            alpha = m.alpha
            shape = RoundRectAt(m.onScreen, m.radius * m.s)
            shadowElevation = 24f * e * (1f - e) * density
        }
    }

    internal fun draw(d: ContentDrawScope, shown: Float, leaving: Boolean, scrim: Color) =
        with(d) {
            if (motion.top == id) motion.drawnAt = shown.coerceIn(0f, 1f)
            // A tab left at once is not drawn for the frame it is still there: what arrives is.
            if (motion.move == Move.Cut && leaving) return
            val m = placing(size, shown)
            if (m == null) drawContent()
            else {
                motion.movingFrames++
                withTransform({
                    translate(m.dx, m.dy)
                    scale(m.s, m.s, Offset.Zero)
                }) {
                    clipPath(Path().apply { addRoundRect(RoundRect(m.clip, CornerRadius(m.radius))) }) { this@with.drawContent() }
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

    private companion object {
        /** How large a page with no art to grow from starts, of the screen. */
        const val CENTRE = 0.8f
    }
}

/**
 * One destination of the NavHost, drawn in the navigation's motion. A page (anything that is not a
 * tab) grows, shrinks and is pulled down to go back; a tab only dims under a page. Both are opaque,
 * so that whatever moves over them hides them.
 */
@Composable
fun AnimatedVisibilityScope.Moving(motion: PageMotion, entry: NavBackStackEntry, content: @Composable () -> Unit) {
    val page = PageMotion.isPage(entry)
    // One number per destination: 1 while it is the page shown, 0 while it is not. A page that is
    // the one moving is drawn as open as this says; one under a page is darker by what it lacks.
    // It is the navigation's own transition, so a gesture that seeks the transition seeks this.
    val shown = transition.animateFloat({ motion.spec() }, label = "shown") { if (it == EnterExitState.Visible) 1f else 0f }
    val transition = transition
    val density = LocalDensity.current
    val frame = remember(entry.id) { PageFrame(motion, entry.id, if (page) motion.originOf(entry.id) else null, with(density) { 28.dp.toPx() }) }
    val back = LocalOnBackPressedDispatcherOwner.current?.onBackPressedDispatcher
    val pull = remember(back) { back?.let(::Pull) }
    val scrim = MaterialTheme.colorScheme.scrim
    Box(
        Modifier.fillMaxSize()
            .then(frame.outer.modifier)
            .then(if (page && pull != null) Modifier.pointerInput(pull) { pull.watch(this) }.nestedScroll(pull) else Modifier)
            .then(if (page) Modifier.graphicsLayer { frame.layer(this, shown.value) } else Modifier)
            .drawWithContent { frame.draw(this, shown.value, transition.targetState != EnterExitState.Visible, scrim) }
            .background(MaterialTheme.colorScheme.background)
            .then(frame.inner.modifier)
    ) {
        if (page) CompositionLocalProvider(LocalPageFrame provides frame) { content() } else content()
    }
}

/**
 * A page pulled down from its top goes back, the way the back gesture takes it: the pull is said to
 * the activity's back dispatcher as a back gesture's progress, so the page shrinks toward its card
 * under the finger, and letting go far enough — or fast enough — finishes it; short of that it
 * springs back. Only once the page's list is at its top and has nothing more to give, only for a
 * downward drag that is mostly vertical, and never for a fling that reaches the top on its own.
 */
class Pull(private val back: OnBackPressedDispatcher) : NestedScrollConnection {
    private var down = Offset.Zero
    private var now = Offset.Zero
    private var pulling = false
    private var pulled = 0f
    private var height = 1f

    /** How far the pull has come, 0 to just short of 1, as the back gesture's progress. */
    val progress: Float
        get() = if (pulling) (pulled / (height * REACH)).coerceIn(0f, 0.99f) else 0f

    // Where the finger went down and where it is: the angle of the drag so far. Watched on the
    // way in, consuming nothing, so the list scrolls exactly as it did.
    internal suspend fun watch(scope: androidx.compose.ui.input.pointer.PointerInputScope) {
        height = scope.size.height.toFloat().coerceAtLeast(1f)
        scope.awaitEachGesture {
            down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial).position
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

    override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset {
        if (pulling || source != NestedScrollSource.UserInput || available.y <= 0f || !vertical) return Offset.Zero
        pulling = true
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
        // The fling was the pull's: the list does not go on to scroll with it.
        return available
    }

    private companion object {
        /** How far down, as a share of the page's height, a pull runs to reach the card. */
        const val REACH = 0.6f

        /** A pull let go past this much of the way goes back. */
        const val COMMIT = 0.2f

        /** Or one let go moving down faster than this many page heights a second. */
        const val FLICK = 1.2f
    }
}
