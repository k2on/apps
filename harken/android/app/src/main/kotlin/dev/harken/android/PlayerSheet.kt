// The player as one thing with two faces: the bar above the tabs and the
// full player are one card, and how far it has come up is one number, `p`,
// 0 at the bar and 1 covering the screen. The card's bounds go from the
// bar's to the screen's as `p` goes, its corners from the bar's to none;
// the bar's face goes out over the first fifth of the way, the full face
// comes in over the last two fifths, and the art travels between the two
// the whole way, from the thumbnail to the big square. The tab bar slides
// away under it and the page behind dims. Harken's Expo player was this
// same shape, for the same reason: a bar and a sheet drawn as two things
// show the seam between them, and what you drag up is not what arrives.
//
// `p` is the offset of an anchored drag, so the finger moves it, a fling
// settles it to the nearer end, a tap on the bar runs it up along the same
// path, and the system's back gesture runs it down under the finger. It is
// read in layout and drawing only: the card moving composes nothing, and
// the page behind it is not composed again at all.
@file:OptIn(ExperimentalFoundationApi::class)

package dev.harken.android

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.exponentialDecay
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
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/** The two ends the player rests at. */
enum class Face {
    Bar,
    Full,
}

/** How dark the page gets under the full player. */
const val PLAYER_DIM = 0.4f

/** The bar's height: its row and the hairline under it. */
val BAR = 58.dp

/**
 * The player's one number and the drag that moves it. [p] is 0 at the bar and 1 full, and is read
 * only where the card is laid out and drawn. A screenshot's sheet is [held] at a `p` of its own.
 */
@Stable
class PlayerSheet(initial: Face, density: Density, private val held: Float? = null) {
    val drag =
        AnchoredDraggableState(
            initialValue = initial,
            // The nearer end, unless the finger was moving: then the end it was moving toward.
            positionalThreshold = { distance -> distance * 0.5f },
            velocityThreshold = { with(density) { 400.dp.toPx() } },
            snapAnimationSpec = spring(dampingRatio = 1f, stiffness = 400f),
            decayAnimationSpec = exponentialDecay(),
        )

    /** How far the bar's top has to travel to reach the screen's: the drag's whole range. */
    internal var travel = 0f
        private set

    /** How far the bar has been pulled sideways, to skip. */
    internal var swipe by mutableFloatStateOf(0f)

    val p: Float
        get() {
            held?.let {
                return it
            }
            val o = drag.offset
            if (o.isNaN() || travel <= 0f) return if (drag.currentValue == Face.Full) 1f else 0f
            return (1f - o / travel).coerceIn(0f, 1f)
        }

    internal val isHeld: Boolean
        get() = held != null

    internal fun reach(t: Float) {
        if (held != null || t == travel || t <= 0f) return
        travel = t
        drag.updateAnchors(
            DraggableAnchors {
                Face.Bar at t
                Face.Full at 0f
            }
        )
    }

    suspend fun open() {
        if (held == null) drag.animateTo(Face.Full)
    }

    suspend fun close() {
        if (held == null) drag.animateTo(Face.Bar)
    }

    companion object {
        /** A sheet that stays at `p`: a screenshot's, a frame of the move. */
        fun held(p: Float, density: Density) = PlayerSheet(if (p >= 1f) Face.Full else Face.Bar, density, p)
    }
}

@Composable
fun rememberPlayerSheet(): PlayerSheet {
    val density = LocalDensity.current
    return rememberSaveable(density, saver = Saver(save = { it.drag.currentValue == Face.Full }, restore = { PlayerSheet(if (it) Face.Full else Face.Bar, density) })) {
        PlayerSheet(Face.Bar, density)
    }
}

// Where things are, in pixels, for a sheet `w` by `h` over tabs `tabs` tall: whole pixels where the
// bar is laid out, rounded as Compose rounds each piece of a layout, so
// the card sits on exactly the pixels the bar's own layout would give it.
private class Geometry(d: Density) {
    val s = with(d) { Space.s.roundToPx().toFloat() }
    val xs = with(d) { Space.xs.roundToPx().toFloat() }
    val xl = with(d) { Space.xl.roundToPx().toFloat() }
    val barH = with(d) { (56.dp.roundToPx() + 2.dp.roundToPx()).toFloat() }
    val thumb = with(d) { 40.dp.roundToPx().toFloat() }
    val thumbInset = with(d) { 8.dp.roundToPx().toFloat() }
    val corner = with(d) { Space.corner.toPx() }
    val shadow = with(d) { 2.dp.toPx() }
    val lift = with(d) { 12.dp.toPx() }
    val artSmall = with(d) { 4.dp.toPx() }
    val artBig = with(d) { 12.dp.toPx() }
    val guessTop = with(d) { 104.dp.toPx() }
    val far = with(d) { 600.dp.toPx() }

    /** The sheet's size, as last measured: the screen's. */
    var w = 0f
    var h = 0f

    fun bar(w: Float, h: Float, tabs: Float) = Rect(s, h - tabs - xs - barH, w - s, h - tabs - xs)

    fun card(w: Float, h: Float, tabs: Float, p: Float): Rect {
        val b = bar(w, h, tabs)
        return Rect(lerp(b.left, 0f, p), lerp(b.top, 0f, p), lerp(b.right, w, p), lerp(b.bottom, h, p))
    }

    fun thumb(w: Float, h: Float, tabs: Float): Rect {
        val b = bar(w, h, tabs)
        return Rect(Offset(b.left + thumbInset, b.top + thumbInset), Size(thumb, thumb))
    }

    fun bigGuess(w: Float) = Rect(Offset(xl, guessTop), Size(w - 2 * xl, w - 2 * xl))
}

/**
 * The player over the page and the tabs: the dim, and the card with its two faces and the art. `tabs`
 * is the tab bar's height, which the bar stands on.
 */
@Composable
fun PlayerCard(sheet: PlayerSheet, state: PlayerState, controls: Controls, position: StateFlow<Long>, tabs: Dp) {
    val now = state.now ?: return
    val density = LocalDensity.current
    val scope = rememberCoroutineScope()
    val g = remember(density) { Geometry(density) }
    val tabsPx = with(density) { tabs.roundToPx().toFloat() }
    // Read in composition only where they flip, never as the card moves: whether the full face is
    // there at all (collapsed, it is not composed, so it takes no touch and no tick), whether the
    // bar is at rest (a tap opens it, a sideways drag skips), and which way it is headed.
    val full by remember(sheet) { derivedStateOf { sheet.p > 0f } }
    val resting by remember(sheet) { derivedStateOf { sheet.p == 0f } }
    val up by remember(sheet) { derivedStateOf { sheet.drag.targetValue == Face.Full && !sheet.isHeld } }

    // The edge's back gesture takes the full player down under the finger; let go short of the
    // system's threshold and it goes back up.
    PredictiveBackHandler(enabled = up) { events ->
        try {
            sheet.drag.anchoredDrag { _ -> events.collect { e -> dragTo(sheet.travel * e.progress) } }
            sheet.close()
        } catch (e: CancellationException) {
            scope.launch { sheet.open() }
            throw e
        }
    }

    val scrim = MaterialTheme.colorScheme.scrim
    val card = MaterialTheme.colorScheme.surfaceContainerHigh
    val ground = MaterialTheme.colorScheme.background
    // Where the full face leaves room for the art: read as the card is placed, just after the face.
    val slot = remember { Spot() }
    val seed = now.album.ifEmpty { now.title }
    val skip = rememberDraggableState { d -> sheet.swipe += d }

    CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onSurface) {
        Layout(
            content = {
                Spacer(Modifier.layoutId(DIM).drawBehind { drawRect(scrim, alpha = PLAYER_DIM * sheet.p) })
                Layout(
                    content = {
                        MiniFace(
                            state,
                            controls,
                            position,
                            art = false,
                            modifier =
                                Modifier.layoutId(MINI)
                                    .draggable(
                                        skip,
                                        Orientation.Horizontal,
                                        enabled = resting && !sheet.isHeld,
                                        onDragStopped = { v ->
                                            // Far enough, or fast enough: left is the next track, right the one before.
                                            val wide = WIDE * (g.w - 2 * g.s)
                                            if (sheet.swipe < -wide || v < -g.far) controls.next() else if (sheet.swipe > wide || v > g.far) controls.previous()
                                            animate(sheet.swipe, 0f, animationSpec = spring(dampingRatio = 0.8f, stiffness = 500f)) { x, _ -> sheet.swipe = x }
                                        },
                                    ),
                        )
                        if (full) {
                            FullFace(
                                state,
                                controls,
                                position,
                                slot = slot.modifier,
                                modifier = Modifier.layoutId(FULL),
                            )
                        }
                        Cover(seed, now.art, 360.dp, Modifier.layoutId(ART), corner = 0.dp)
                    },
                    modifier =
                        Modifier.layout { m, c ->
                                // The card: the bar's bounds at 0, the screen's at 1. Measured at its
                                // size for the moment, so that what it touches is what it covers.
                                val w = c.maxWidth.toFloat()
                                val h = c.maxHeight.toFloat()
                                g.w = w
                                g.h = h
                                sheet.reach(g.bar(w, h, tabsPx).top)
                                val r = g.card(w, h, tabsPx, sheet.p)
                                val pl = m.measure(Constraints.fixed(r.width.roundToInt().coerceAtLeast(0), r.height.roundToInt().coerceAtLeast(0)))
                                layout(c.maxWidth, c.maxHeight) { pl.place(r.left.roundToInt(), r.top.roundToInt()) }
                            }
                            .graphicsLayer {
                                val p = sheet.p
                                shape = RoundedCornerShape(lerp(g.corner, 0f, p))
                                clip = true
                                shadowElevation = lerp(g.shadow, 0f, p) + g.lift * 4f * p * (1f - p)
                            }
                            .drawBehind { drawRect(lerp(card, ground, sheet.p)) }
                            .anchoredDraggable(sheet.drag, Orientation.Vertical, enabled = !sheet.isHeld)
                            // No ripple: the card answering a tap is the card coming up.
                            .clickable(interactionSource = null, indication = null, enabled = resting, onClickLabel = "Open the player", onClick = controls::open),
                ) { measurables, c ->
                    // The faces are laid out once, at the bar's size and the screen's, and only moved:
                    // the card changing size measures nothing inside it.
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
                            MINI -> mini = m.measure(Constraints.fixed(bar.width.roundToInt(), bar.height.roundToInt()))
                            FULL -> face = m.measure(Constraints.fixed(w.roundToInt(), h.roundToInt()))
                            ART -> art = m.measure(Constraints.fixed(side, side))
                        }
                    }
                    layout(c.maxWidth, c.maxHeight) {
                        val dx = -r.left
                        val dy = -r.top
                        // The bar's face, gone by a fifth of the way: not placed past it, so its
                        // buttons cannot be pressed through the full player.
                        if (p < MINI_OUT) {
                            mini?.placeWithLayer((bar.left + dx + sheet.swipe).roundToInt(), (bar.top + dy).roundToInt()) {
                                alpha = 1f - sheet.p / MINI_OUT
                            }
                        }
                        face?.placeWithLayer(dx.roundToInt(), dy.roundToInt()) { alpha = ((sheet.p - FULL_IN) / (1f - FULL_IN)).coerceIn(0f, 1f) }
                        art?.let { a ->
                            val from = g.thumb(w, h, tabsPx).translate(sheet.swipe * (1f - p), 0f)
                            // The face has just been placed, so where its slot is, is known now.
                            val to =
                                slot.coords?.takeIf { face != null && it.isAttached }?.let { sc -> coordinates?.localBoundingBoxOf(sc, clipBounds = false)?.translate(r.left, r.top) }
                                    ?: g.bigGuess(w)
                            val at = Rect(lerp(from.left, to.left, p), lerp(from.top, to.top, p), lerp(from.right, to.right, p), lerp(from.bottom, to.bottom, p))
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
            val placeables = measurables.map { it.measure(Constraints.fixed(c.maxWidth, c.maxHeight)) }
            layout(c.maxWidth, c.maxHeight) { placeables.forEach { it.place(0, 0) } }
        }
    }
}

private const val DIM = "dim"
private const val MINI = "mini"
private const val FULL = "full"
private const val ART = "art"

/** The bar's face is gone by this much of the way up. */
private const val MINI_OUT = 0.2f

/** The full face starts to come in this far up. */
private const val FULL_IN = 0.6f

/** A sideways pull past this share of the bar's width skips. */
private const val WIDE = 0.25f
