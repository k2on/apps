// A card that is one thing with two faces: a bar standing above something
// at the foot of the screen (a tab bar), and the whole screen it expands
// into. How far it has come up is one number, `p`, 0 at the bar and 1
// covering the screen. The card's bounds go from the bar's to the screen's
// as `p` goes, its corners from the bar's to none; the collapsed face goes
// out over the first fifth of the way, the expanded face comes in over the
// last two fifths, and one piece the card draws itself — a cover — travels
// between the two the whole way, from its place on the bar to the place the
// expanded face leaves for it. What is under it dims. harken's player is
// this card (its bar and its full player), and its Expo player was this
// same shape, for the same reason: a bar and a sheet drawn as two things
// show the seam between them, and what you drag up is not what arrives.
//
// `p` is the offset of an anchored drag, so the finger moves it, a release
// settles it to the nearer end — or the end it was flung toward — on a
// spring that starts at the release's speed and is done within 300 ms
// however slowly it was let go (`Springs.settle`), a tap on the bar runs it
// up along the same path, and the system's back gesture previews it down a
// little way — no further, however far the finger goes (`Springs.preview`)
// — and finishes or springs back. A sideways drag on the bar is the bar's
// own, split from the vertical one by each detector's axis, and leaves `p`
// alone. `p` is read in layout and drawing only: the card moving composes
// nothing, and what is behind it is not composed again at all.
@file:OptIn(ExperimentalFoundationApi::class)

package dev.arkui

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.spring
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.AnchoredDraggableState
import androidx.compose.foundation.gestures.DraggableAnchors
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.anchoredDraggable
import androidx.compose.foundation.gestures.animateTo
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.Placeable
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.layoutId
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.lerp
import kotlin.coroutines.cancellation.CancellationException
import kotlin.math.roundToInt
import kotlinx.coroutines.launch

/** The two ends an [ExpandingCard] rests at. */
public enum class CardFace {
    Collapsed,
    Expanded,
}

/**
 * The card's one number and the drag that moves it. [p] is 0 at the bar and 1 expanded, and is read
 * only where the card is laid out and drawn. A screenshot's card is [held] at a `p` of its own.
 */
@Stable
public class ExpandingCardState(
    initial: CardFace,
    density: Density,
    private val held: Float? = null,
) {
    /** The anchored drag whose offset [p] is. */
    public val drag: AnchoredDraggableState<CardFace> =
        AnchoredDraggableState(
            initialValue = initial,
            // The nearer end, unless the finger was moving: then the end it was moving toward.
            positionalThreshold = { distance -> distance * 0.5f },
            velocityThreshold = { with(density) { 400.dp.toPx() } },
            // Always the spring, from the release's own speed, and never longer than its cap: a
            // slow release does not crawl to the end at the finger's pace, and a flick lands
            // sooner.
            snapAnimationSpec = Springs.settle(),
            decayAnimationSpec = Springs.noDecay(),
        )

    /**
     * How far the bar's top has to travel to reach the screen's: the drag's whole range, in pixels.
     */
    public var travel: Float = 0f
        private set

    /** How far the bar has been pulled sideways. */
    internal var swipe by mutableFloatStateOf(0f)

    /** How far up the card is: 0 at the bar, 1 covering the screen. */
    public val p: Float
        get() {
            held?.let {
                return it
            }
            val o = drag.offset
            if (o.isNaN() || travel <= 0f)
                return if (drag.currentValue == CardFace.Expanded) 1f else 0f
            return (1f - o / travel).coerceIn(0f, 1f)
        }

    internal val isHeld: Boolean
        get() = held != null

    internal fun reach(t: Float) {
        if (held != null || t == travel || t <= 0f) return
        travel = t
        drag.updateAnchors(
            DraggableAnchors {
                CardFace.Collapsed at t
                CardFace.Expanded at 0f
            })
    }

    /** Up to the whole screen, along the drag's own path. */
    public suspend fun open() {
        if (held == null) drag.animateTo(CardFace.Expanded)
    }

    /** Down to the bar. */
    public suspend fun close() {
        if (held == null) drag.animateTo(CardFace.Collapsed)
    }

    public companion object {
        /** A card that stays at `p`: a screenshot's, a frame of the move. */
        public fun held(p: Float, density: Density): ExpandingCardState =
            ExpandingCardState(if (p >= 1f) CardFace.Expanded else CardFace.Collapsed, density, p)
    }
}

/**
 * An [ExpandingCardState] at the bar, kept across a configuration change by which end it was at.
 */
@Composable
public fun rememberExpandingCard(): ExpandingCardState {
    val density = LocalDensity.current
    return rememberSaveable(
        density,
        saver =
            Saver(
                save = { it.drag.currentValue == CardFace.Expanded },
                restore = {
                    ExpandingCardState(if (it) CardFace.Expanded else CardFace.Collapsed, density)
                },
            ),
    ) {
        ExpandingCardState(CardFace.Collapsed, density)
    }
}

/**
 * The card's measurements, in its own units. The defaults are harken's player: a 56 dp bar with a 2
 * dp progress hairline under it, inset [side] from the screen's edges and [gap] above what it
 * stands on, a 40 dp cover 8 dp into it, and the expanded cover [bigInset] from the edges.
 */
@Immutable
public data class ExpandingCardSpec(
    /**
     * The bar's height, as the pieces its face is laid out of: each is rounded to whole pixels
     * before they are added, as a column of them is, so the card sits on exactly the pixels the
     * face's own layout gives it.
     */
    public val bar: List<Dp> = listOf(56.dp, 2.dp),
    /** The bar's inset from the screen's sides. */
    public val side: Dp = Space.s,
    /** The gap between the bar and what it stands on. */
    public val gap: Dp = Space.xs,
    /** The bar's corners; the expanded card has none. */
    public val corner: Dp = Space.corner,
    /** The bar's shadow at rest; none expanded. */
    public val shadow: Dp = 2.dp,
    /** How far the card lifts off the page while it moves, at its most. */
    public val lift: Dp = 12.dp,
    /** The travelling piece's size on the bar, and how far into the bar it stands. */
    public val thumb: Dp = 40.dp,
    public val thumbInset: Dp = 8.dp,
    /** The travelling piece's corner on the bar and expanded. */
    public val thumbCorner: Dp = 4.dp,
    public val bigCorner: Dp = 12.dp,
    /**
     * The expanded piece's margin from the screen's sides — it is the screen's width less two of
     * these, square — and where its top is guessed to be before the expanded face has said.
     */
    public val bigInset: Dp = Space.xl,
    public val bigTop: Dp = 104.dp,
    /** A sideways drag let go faster than this many dp a second is a skip, however short. */
    public val flick: Dp = 600.dp,
    /** …or one pulled past this share of the bar's width. */
    public val wide: Float = 0.25f,
    /** How dark what is under the card gets, expanded. */
    public val dim: Float = 0.4f,
    /** The collapsed face is gone by this much of the way up. */
    public val collapsedOut: Float = 0.2f,
    /** The expanded face starts to come in this far up. */
    public val expandedIn: Float = 0.6f,
)

// Where things are, in pixels, for a card `w` by `h` over something `stand` tall: whole pixels
// where
// the bar is laid out, rounded as Compose rounds each piece of a layout, so the card sits on
// exactly
// the pixels the bar's own layout would give it.
private class Geometry(d: Density, spec: ExpandingCardSpec) {
    val s = with(d) { spec.side.roundToPx().toFloat() }
    val xs = with(d) { spec.gap.roundToPx().toFloat() }
    val xl = with(d) { spec.bigInset.roundToPx().toFloat() }
    val barH = with(d) { spec.bar.sumOf { it.roundToPx() }.toFloat() }
    val thumb = with(d) { spec.thumb.roundToPx().toFloat() }
    val thumbInset = with(d) { spec.thumbInset.roundToPx().toFloat() }
    val corner = with(d) { spec.corner.toPx() }
    val shadow = with(d) { spec.shadow.toPx() }
    val lift = with(d) { spec.lift.toPx() }
    val artSmall = with(d) { spec.thumbCorner.toPx() }
    val artBig = with(d) { spec.bigCorner.toPx() }
    val guessTop = with(d) { spec.bigTop.toPx() }
    val far = with(d) { spec.flick.toPx() }

    /** The card's size, as last measured: the screen's. */
    var w = 0f
    var h = 0f

    fun bar(w: Float, h: Float, stand: Float) =
        Rect(s, h - stand - xs - barH, w - s, h - stand - xs)

    fun card(w: Float, h: Float, stand: Float, p: Float): Rect {
        val b = bar(w, h, stand)
        return Rect(
            lerp(b.left, 0f, p), lerp(b.top, 0f, p), lerp(b.right, w, p), lerp(b.bottom, h, p))
    }

    fun thumb(w: Float, h: Float, stand: Float): Rect {
        val b = bar(w, h, stand)
        return Rect(Offset(b.left + thumbInset, b.top + thumbInset), Size(thumb, thumb))
    }

    fun bigGuess(w: Float) = Rect(Offset(xl, guessTop), Size(w - 2 * xl, w - 2 * xl))
}

/**
 * The card over the page and what it stands on: the dim, the card with its two faces, and the piece
 * that travels between them. `stand` is the height of what the bar stands on (a tab bar); the card
 * fills the space it is given, which is the screen.
 * - `collapsed` is the bar's face, given the modifier it must take: it is laid out once at the
 *   bar's size and only moved, and not placed at all past [ExpandingCardSpec.collapsedOut], so
 *   nothing on it can be pressed through the expanded card. It should leave the travelling piece's
 *   place empty.
 * - `expanded` is the full face, laid out at the screen's size, composed only while the card is
 *   above the bar — collapsed, it takes no touch and no tick. Its first argument goes on the space
 *   it leaves for the travelling piece, which is told where that is.
 * - `traveler` is the piece both faces share, drawn by the card itself at the expanded size and
 *   scaled from the bar's [ExpandingCardSpec.thumb] to that space.
 *
 * A tap on the bar calls `onOpen` (which is expected to [ExpandingCardState.open] it); a sideways
 * drag on the bar, far or fast enough, calls `onSwipeLeft` or `onSwipeRight`, and springs back. The
 * system's back gesture, while the card is up or going up, previews it down and closes it.
 */
@Composable
public fun ExpandingCard(
    state: ExpandingCardState,
    stand: Dp,
    onOpen: () -> Unit,
    collapsed: @Composable (Modifier) -> Unit,
    expanded: @Composable (slot: Modifier, modifier: Modifier) -> Unit,
    traveler: @Composable (Modifier) -> Unit,
    onSwipeLeft: () -> Unit = {},
    onSwipeRight: () -> Unit = {},
    openLabel: String? = null,
    spec: ExpandingCardSpec = remember { ExpandingCardSpec() },
) {
    val sheet = state
    val density = LocalDensity.current
    val scope = rememberCoroutineScope()
    val g = remember(density, spec) { Geometry(density, spec) }
    val tabsPx = with(density) { stand.roundToPx().toFloat() }
    // Read in composition only where they flip, never as the card moves: whether the expanded face
    // is there at all (collapsed, it is not composed, so it takes no touch and no tick), whether
    // the bar is at rest (a tap opens it, a sideways drag is the bar's), and which way it is
    // headed.
    val full by remember(sheet) { derivedStateOf { sheet.p > 0f } }
    val resting by remember(sheet) { derivedStateOf { sheet.p == 0f } }
    val up by
        remember(sheet) {
            derivedStateOf { sheet.drag.targetValue == CardFace.Expanded && !sheet.isHeld }
        }

    // The edge's back gesture previews the card down a little way, toward a limit it never passes;
    // let go past the system's threshold and it settles to the bar from there, short of it and it
    // goes back up.
    PredictiveBackHandler(enabled = up) { events ->
        try {
            sheet.drag.anchoredDrag { _ ->
                events.collect { e -> dragTo(sheet.travel * Springs.preview(e.progress)) }
            }
        } catch (e: CancellationException) {
            scope.launch { sheet.open() }
            throw e
        }
        // Not in this handler's own coroutine: the settle turns the card toward the bar, which
        // stands the handler down, which cancels it — and a cancel here is the gesture undone.
        scope.launch { sheet.close() }
    }

    val scrim = MaterialTheme.colorScheme.scrim
    val card = MaterialTheme.colorScheme.surfaceContainerHigh
    val ground = MaterialTheme.colorScheme.background
    // Where the expanded face leaves room for the piece: read as the card is placed, just after the
    // face.
    val slot = remember { Spot() }
    val skip = rememberDraggableState { d -> sheet.swipe += d }

    CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onSurface) {
        Layout(
            content = {
                Spacer(
                    Modifier.layoutId(DIM).drawBehind {
                        drawRect(scrim, alpha = spec.dim * sheet.p)
                    })
                Layout(
                    content = {
                        collapsed(
                            Modifier.layoutId(MINI)
                                .draggable(
                                    skip,
                                    Orientation.Horizontal,
                                    enabled = resting && !sheet.isHeld,
                                    onDragStopped = { v ->
                                        // Far enough, or fast enough: left, or right.
                                        val wide = spec.wide * (g.w - 2 * g.s)
                                        if (sheet.swipe < -wide || v < -g.far) onSwipeLeft()
                                        else if (sheet.swipe > wide || v > g.far) onSwipeRight()
                                        animate(
                                            sheet.swipe,
                                            0f,
                                            animationSpec = spring(Springs.MOVE, Springs.STIFFNESS),
                                        ) { x, _ ->
                                            sheet.swipe = x
                                        }
                                    },
                                ))
                        if (full) {
                            expanded(slot.modifier, Modifier.layoutId(FULL))
                        }
                        traveler(Modifier.layoutId(ART))
                    },
                    modifier =
                        Modifier.layout { m, c ->
                                // The card: the bar's bounds at 0, the screen's at 1. Measured at
                                // its size for the moment, so that what it touches is what it
                                // covers.
                                val w = c.maxWidth.toFloat()
                                val h = c.maxHeight.toFloat()
                                g.w = w
                                g.h = h
                                sheet.reach(g.bar(w, h, tabsPx).top)
                                val r = g.card(w, h, tabsPx, sheet.p)
                                val pl =
                                    m.measure(
                                        Constraints.fixed(
                                            r.width.roundToInt().coerceAtLeast(0),
                                            r.height.roundToInt().coerceAtLeast(0),
                                        ))
                                layout(c.maxWidth, c.maxHeight) {
                                    pl.place(r.left.roundToInt(), r.top.roundToInt())
                                }
                            }
                            .graphicsLayer {
                                val p = sheet.p
                                shape = RoundedCornerShape(lerp(g.corner, 0f, p))
                                clip = true
                                shadowElevation = lerp(g.shadow, 0f, p) + g.lift * 4f * p * (1f - p)
                            }
                            .drawBehind { drawRect(lerp(card, ground, sheet.p)) }
                            .anchoredDraggable(
                                sheet.drag,
                                Orientation.Vertical,
                                enabled = !sheet.isHeld,
                            )
                            // No ripple: the card answering a tap is the card coming up.
                            .clickable(
                                interactionSource = null,
                                indication = null,
                                enabled = resting,
                                onClickLabel = openLabel,
                                onClick = onOpen,
                            ),
                ) { measurables, c ->
                    // The faces are laid out once, at the bar's size and the screen's, and only
                    // moved: the card changing size measures nothing inside it.
                    val w = g.w
                    val h = g.h
                    val p = sheet.p
                    val r = g.card(w, h, tabsPx, p)
                    val bar = g.bar(w, h, tabsPx)
                    var mini: Placeable? = null
                    var face: Placeable? = null
                    var art: Placeable? = null
                    val side = (w - 2 * g.xl).roundToInt().coerceAtLeast(1)
                    for (m in measurables) {
                        when (m.layoutId) {
                            MINI ->
                                mini =
                                    m.measure(
                                        Constraints.fixed(
                                            bar.width.roundToInt(),
                                            bar.height.roundToInt(),
                                        ))
                            FULL ->
                                face = m.measure(Constraints.fixed(w.roundToInt(), h.roundToInt()))
                            ART -> art = m.measure(Constraints.fixed(side, side))
                        }
                    }
                    layout(c.maxWidth, c.maxHeight) {
                        val dx = -r.left
                        val dy = -r.top
                        // The bar's face, gone by a fifth of the way: not placed past it, so its
                        // buttons cannot be pressed through the expanded card.
                        if (p < spec.collapsedOut) {
                            mini?.placeWithLayer(
                                (bar.left + dx + sheet.swipe).roundToInt(),
                                (bar.top + dy).roundToInt(),
                            ) {
                                alpha = 1f - sheet.p / spec.collapsedOut
                            }
                        }
                        face?.placeWithLayer(dx.roundToInt(), dy.roundToInt()) {
                            alpha =
                                ((sheet.p - spec.expandedIn) / (1f - spec.expandedIn)).coerceIn(
                                    0f, 1f)
                        }
                        art?.let { a ->
                            val from = g.thumb(w, h, tabsPx).translate(sheet.swipe * (1f - p), 0f)
                            // The face has just been placed, so where its slot is, is known now.
                            val to =
                                slot.coords
                                    ?.takeIf { face != null && it.isAttached }
                                    ?.let { sc ->
                                        coordinates
                                            ?.localBoundingBoxOf(sc, clipBounds = false)
                                            ?.translate(r.left, r.top)
                                    } ?: g.bigGuess(w)
                            val at =
                                Rect(
                                    lerp(from.left, to.left, p),
                                    lerp(from.top, to.top, p),
                                    lerp(from.right, to.right, p),
                                    lerp(from.bottom, to.bottom, p),
                                )
                            a.placeWithLayer(0, 0) {
                                val k = at.width / a.width
                                transformOrigin = TransformOrigin(0f, 0f)
                                scaleX = k
                                scaleY = k
                                translationX = at.left + dx
                                translationY = at.top + dy
                                shape = RoundedCornerShape(lerp(g.artSmall, g.artBig, p) / k)
                                clip = true
                            }
                        }
                    }
                }
            },
        ) { measurables, c ->
            val placeables =
                measurables.map { it.measure(Constraints.fixed(c.maxWidth, c.maxHeight)) }
            layout(c.maxWidth, c.maxHeight) { placeables.forEach { it.place(0, 0) } }
        }
    }
}

private const val DIM = "dim"
private const val MINI = "mini"
private const val FULL = "full"
private const val ART = "art"
