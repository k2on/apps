// Glass, as iOS 26's Liquid Glass changes shape, and only that: no blur,
// no refraction, no highlight. A piece of glass here is a solid capsule or
// rounded rectangle in the theme's brightest surface with a soft shadow
// under it, and what it borrows from iOS is how that surface moves.
//
// Three things move it. A piece that moves in its layout glides there on
// a spring, its content with it, rather than jumping. Pieces in one
// `GlassContainer` share one surface: two that come closer than the
// container's spacing are joined by a neck that thickens as they close, as
// SwiftUI's `GlassEffectContainer` blends them, a piece that appears buds
// out of its nearest neighbour, and one that goes shrinks back into it.
// And a `GlassMorph` is a button whose glass grows into the menu it opens
// — its bounds and its corners going from the button's to the menu's on
// one spring, the button's face fading out as the menu's comes in, the
// menu anchored at the button's edge — and shrinks back into the button
// when the menu closes, as iOS's menus and popovers grow out of the button
// that opened them.
//
// It all moves on arkui's critically damped spring for where a thing is:
// nothing here was thrown, so nothing overshoots. With motion reduced
// (`reducedMotion`), nothing glides or buds — a piece is simply where its
// layout has it — and a morph does not grow: the menu fades in where it
// opens, over the button, and fades out back into it, on `Springs.fade`.
//
// Everything is drawn with what every Android version has: a path, a
// clip and a paint's shadow layer (drawn on hardware from Android 9; below
// it the glass has no shadow), so a screenshot on the JVM draws what a
// phone does.
package dev.arkui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationVector4D
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.spring
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathOperation
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.Measurable
import androidx.compose.ui.layout.MeasureResult
import androidx.compose.ui.layout.MeasureScope
import androidx.compose.ui.layout.layoutId
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.node.CompositionLocalConsumerModifierNode
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.GlobalPositionAwareModifierNode
import androidx.compose.ui.node.LayoutModifierNode
import androidx.compose.ui.node.ModifierNodeElement
import androidx.compose.ui.node.currentValueOf
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.lerp
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/** The glass's look, said once. */
public object Glass {
    /**
     * What glass is filled with: the theme's glass (arkui/theme.toml), which `arkuiScheme` puts in
     * the scheme's brightest surface — white on a light page, a lifted charcoal on a dark one.
     */
    @Composable
    @ReadOnlyComposable
    public fun fill(): Color = MaterialTheme.colorScheme.surfaceBright

    /** How soft the shadow under glass is: its blur's radius. None, for glass with no shadow. */
    public val shadow: Dp = 12.dp

    /** A menu's corners, grown from its button's. */
    public val menuCorner: Dp = 28.dp

    /** How wide a menu is, at most: the screen less a margin on each side, if that is narrower. */
    public val menuWidth: Dp = 320.dp

    /** A menu's margin from the edges of the space it opens in. */
    public val margin: Dp = Space.l

    /** A sheet's margin from the space's sides and foot: the iPhone's floating sheets'. */
    public val sheetMargin: Dp = 8.dp

    /** A sheet's corners: the iPhone's, near the screen's own. */
    public val sheetCorner: Dp = 36.dp
}

/** A piece of glass's outline: a capsule, its ends half its height, or a rounded rectangle. */
@Immutable
public class GlassShape private constructor(private val corner: Dp?) {
    /** Its corners' radius at `size`: never more than half its shorter side. */
    internal fun radius(size: Size, density: Density): Float {
        val most = min(size.width, size.height) / 2f
        return corner?.let { min(with(density) { it.toPx() }, most) } ?: most
    }

    override fun equals(other: Any?): Boolean = other is GlassShape && other.corner == corner

    override fun hashCode(): Int = corner.hashCode()

    public companion object {
        /** Ends as round as it is tall: a chip, a button, a tab bar; on a square, a circle. */
        public val Capsule: GlassShape = GlassShape(null)

        /** A rectangle with corners of `corner`. */
        public fun rounded(corner: Dp): GlassShape = GlassShape(corner)
    }
}

/**
 * This element on glass. Alone, it draws its own glass behind it; in a [GlassContainer], the
 * container draws it, joined to its neighbours, and the element's content follows its glass as
 * that springs to where the layout has put it. `id` is what the container knows it by, as
 * SwiftUI's `glassEffectID` is: a piece with an id buds from its neighbour as it appears and shrinks
 * into it as it goes.
 */
@Composable
public fun Modifier.glass(shape: GlassShape = GlassShape.Capsule, id: Any? = null): Modifier =
    glass(shape, id, hidden = null)

@Composable
private fun Modifier.glass(shape: GlassShape, id: Any?, hidden: (() -> Boolean)?): Modifier {
    val fill = Glass.fill()
    return this.then(GlassElement(shape, id, fill, Glass.shadow, hidden))
}

/**
 * A space glass shares: every [glass] and [GlassMorph] in `content` is drawn by this, as one
 * surface. Two pieces closer than `spacing` are joined, the nearer the thicker; a piece appearing
 * buds from its nearest neighbour, and one going shrinks into it. A [GlassMorph]'s menu opens over
 * this container's whole area, so a container is best as large as the screen it is on.
 */
@Composable
public fun GlassContainer(
    modifier: Modifier = Modifier,
    spacing: Dp = Space.s,
    content: @Composable BoxScope.() -> Unit,
) {
    val density = LocalDensity.current
    val scope = rememberCoroutineScope()
    val space = remember { GlassSpace(scope) }
    space.density = density
    space.reduced = reducedMotion()
    space.reach = with(density) { spacing.toPx() }
    space.fill = Glass.fill()
    space.blur = with(density) { Glass.shadow.toPx() }
    space.scrim = MaterialTheme.colorScheme.scrim
    // What is there on the first frame is simply there; only what comes after it buds.
    LaunchedEffect(space) {
        withFrameNanos {}
        space.settled = true
    }
    Box(modifier.onPlaced { space.coords = it }.drawBehind { space.draw(this) }) {
        CompositionLocalProvider(LocalGlass provides space) { content() }
        for (m in space.morphs) key(m) { MorphLayer(space, m) }
    }
}

/**
 * A button whose glass becomes the menu it opens, as iOS's menus grow out of their button: one
 * surface, its bounds and its corners going from the button's to the menu's on a spring as
 * `expanded` goes true, and back as it goes false. The button's face (`collapsed`) fades out over
 * the first third of the way and the menu (`content`) comes in over the rest, clipped to the glass
 * as it grows. The menu is anchored at the button: its bottom at the button's bottom when the
 * button is in the lower half of the [GlassContainer] (it grows up), its top at the button's top
 * otherwise; and its leading edge at the button's when the button is in the first third across,
 * its trailing edge in the last third, centred on it between. A tap outside the menu, or back,
 * calls `onDismiss`.
 *
 * `sheet`: the menu opens as the iPhone's sheets do in iOS 26, rather than as a menu: as wide as
 * the space less [Glass.sheetMargin] each side, resting on its foot that far up, its corners
 * `menuCorner` — still grown out of the button, and shrunk back into it.
 *
 * It must be in a [GlassContainer], which is where the menu opens; outside one it is the button
 * alone. `held` holds it at that much of the way, 0 to 1, whatever `expanded` says: a screenshot's
 * frame of the morph.
 */
@Composable
public fun GlassMorph(
    expanded: Boolean,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    shape: GlassShape = GlassShape.Capsule,
    id: Any? = null,
    menuWidth: Dp = Glass.menuWidth,
    menuCorner: Dp = Glass.menuCorner,
    sheet: Boolean = false,
    held: Float? = null,
    collapsed: @Composable () -> Unit,
    content: @Composable () -> Unit,
) {
    val space = LocalGlass.current
    if (space == null) {
        Box(modifier.glass(shape, id)) { collapsed() }
        return
    }
    val p = remember { Animatable(held ?: if (expanded) 1f else 0f) }
    val reduced = reducedMotion()
    LaunchedEffect(expanded, held) {
        if (held != null) p.snapTo(held)
        else
            p.animateTo(
                if (expanded) 1f else 0f,
                if (reduced) Springs.fade() else spring(Springs.MOVE, Springs.STIFFNESS, REST))
    }
    val m = remember(space) { Morph(p).also { space.morphs += it } }
    m.reduced = reduced
    DisposableEffect(space, m) { onDispose { space.morphs -= m } }
    m.expanded = expanded && held == null
    m.shape = shape
    m.menuWidth = menuWidth
    m.menuCorner = menuCorner
    m.sheet = sheet
    m.onDismiss = onDismiss
    m.collapsed = collapsed
    m.content = content
    BackHandler(enabled = m.expanded, onBack = onDismiss)
    // In its place, the button keeps its room; while it is out as the morph, the morph draws it.
    Box(
        modifier
            .glass(shape, id, hidden = { p.value > 0f })
            .onPlaced { m.slot = it }
            .graphicsLayer { alpha = if (p.value > 0f) 0f else 1f }) {
            collapsed()
        }
}

/** The container a piece of glass is drawn by, if it is in one. */
private val LocalGlass: ProvidableCompositionLocal<GlassSpace?> = staticCompositionLocalOf {
    null
}

/** How close a spring of a rectangle, in pixels, counts as there. */
private val Settled = Rect(0.5f, 0.5f, 0.5f, 0.5f)

/** How close the morph's 0..1, as there. */
private const val REST = 0.001f

/** How dark what is under an open menu gets. */
private const val MENU_DIM = 0.12f

/** How much of the space it opens in a menu may be tall, at most. */
private const val MENU_TALL = 0.72f

/** The shadow under glass. */
private val Shade = Color.Black.copy(alpha = 0.16f)

private fun glide() = spring(Springs.MOVE, Springs.STIFFNESS, Settled)

/** One piece of glass in a container: where its layout has it, and where it is drawn. */
private class Piece(
    var shape: GlassShape,
    start: Rect,
    var hidden: (() -> Boolean)?,
) {
    var target: Rect = start
    val drawn: Animatable<Rect, AnimationVector4D> = Animatable(start, Rect.VectorConverter)
}

/** A piece that has gone, shrinking into its neighbour. */
private class Ghost(val shape: GlassShape, from: Rect) {
    val drawn: Animatable<Rect, AnimationVector4D> = Animatable(from, Rect.VectorConverter)
}

/** A [GlassMorph] as its container draws it: how far out it is, and what it holds. */
private class Morph(val p: Animatable<Float, *>) {
    var expanded by mutableStateOf(false)

    /** Reduced motion: the menu fades in and out where it opens, rather than growing there. */
    var reduced: Boolean = false
    var shape: GlassShape = GlassShape.Capsule
    var menuWidth: Dp = Glass.menuWidth
    var menuCorner: Dp = Glass.menuCorner
    var sheet: Boolean = false
    var onDismiss: () -> Unit = {}
    var collapsed: @Composable () -> Unit by mutableStateOf({})
    var content: @Composable () -> Unit by mutableStateOf({})

    /**
     * Where the button is laid out: state, so that the surface is laid out again once the button
     * has been placed, on the first frame as on any other.
     */
    var slot: LayoutCoordinates? by mutableStateOf(null)

    /** Where the surface is drawn now, and its corners, in the container's pixels. */
    var at by mutableStateOf(Rect.Zero)
    var corner by mutableStateOf(0f)
}

@Stable
private class GlassSpace(val scope: CoroutineScope) {
    var density: Density = Density(1f)
    var reduced = false
    var reach = 0f
    var fill = Color.White
    var blur = 0f
    var scrim = Color.Black
    var settled = false
    var coords: LayoutCoordinates? = null
    val pieces = mutableStateListOf<Piece>()
    val ghosts = mutableStateListOf<Ghost>()
    val morphs = mutableStateListOf<Morph>()
    val paint = Paint()

    /** A piece first laid out at `r`: there, on the first frame; budding from a neighbour after. */
    fun join(shape: GlassShape, r: Rect, hidden: (() -> Boolean)?): Piece {
        val start = if (settled && !reduced) bud(r, null) else r
        val p = Piece(shape, start, hidden)
        pieces += p
        if (start != r) scope.launch { p.drawn.animateTo(r, glide()) }
        return p
    }

    fun move(p: Piece, r: Rect) {
        if (r == p.target) return
        p.target = r
        scope.launch { if (reduced) p.drawn.snapTo(r) else p.drawn.animateTo(r, glide()) }
    }

    /** A piece gone: it shrinks into its nearest neighbour, or to nothing, and is forgotten. */
    fun leave(p: Piece) {
        pieces -= p
        if (!settled || reduced || p.hidden?.invoke() == true) return
        val from = p.drawn.value
        val g = Ghost(p.shape, from)
        ghosts += g
        scope.launch {
            g.drawn.animateTo(bud(from, p), glide())
            ghosts -= g
        }
    }

    /**
     * Where a piece at `r` comes from, or goes to: a round bud just inside the end of its nearest
     * neighbour that faces it, if one is within twice the spacing; a speck at its own centre if
     * none is.
     */
    fun bud(r: Rect, not: Piece?): Rect {
        var best: Rect? = null
        var gap = 2 * reach
        for (o in pieces) {
            if (o === not || o.hidden?.invoke() == true) continue
            val n = o.drawn.value
            val g = gapBetween(n, r)
            if (g <= gap) {
                gap = g
                best = n
            }
        }
        val n = best ?: return Rect(r.center, r.center)
        val side = min(n.width, n.height)
        return when {
            r.left >= n.right - 1f -> Rect(n.right - side, n.top, n.right, n.bottom)
            r.right <= n.left + 1f -> Rect(n.left, n.top, n.left + side, n.bottom)
            r.top >= n.bottom - 1f -> Rect(n.left, n.bottom - side, n.right, n.bottom)
            else -> Rect(n.left, n.top, n.right, n.top + side)
        }
    }

    /** Every piece, every ghost and every neck between them, as one path, filled with a shadow. */
    fun draw(scope: DrawScope) {
        val shapes = ArrayList<Pair<Rect, Float>>(pieces.size + ghosts.size)
        for (p in pieces) {
            if (p.hidden?.invoke() == true) continue
            val r = p.drawn.value
            shapes += r to p.shape.radius(r.size, density)
        }
        for (g in ghosts) {
            val r = g.drawn.value
            shapes += r to g.shape.radius(r.size, density)
        }
        if (shapes.isEmpty()) return
        var path = Path()
        for ((r, c) in shapes) {
            if (r.width <= 0f || r.height <= 0f) continue
            path = Path.combine(PathOperation.Union, path, rounded(r, c))
        }
        for (i in shapes.indices) for (j in i + 1 until shapes.size) {
            neck(shapes[i], shapes[j], reach)?.let {
                path = Path.combine(PathOperation.Union, path, it)
            }
        }
        scope.drawGlass(path, fill, blur, paint)
    }
}

// How far apart two rectangles are: the gap between their nearest edges, along whichever axis
// they are apart on, or 0 where they overlap.
private fun gapBetween(a: Rect, b: Rect): Float {
    val dx = max(b.left - a.right, a.left - b.right)
    val dy = max(b.top - a.bottom, a.top - b.bottom)
    return max(max(dx, dy), 0f)
}

private fun rounded(r: Rect, c: Float): Path =
    Path().apply { addRoundRect(RoundRect(r, CornerRadius(c, c))) }

/**
 * The neck that joins two pieces side by side, or one above the other, that are nearer than
 * `reach`: from just inside one's end to just inside the other's, its waist nothing at `reach`
 * and nearly their whole height as they touch, its sides curving in as liquid's do.
 */
private fun neck(a: Pair<Rect, Float>, b: Pair<Rect, Float>, reach: Float): Path? {
    if (reach <= 0f) return null
    val (ra, ca) = a
    val (rb, cb) = b
    val across = min(ra.bottom, rb.bottom) - max(ra.top, rb.top)
    val along = min(ra.right, rb.right) - max(ra.left, rb.left)
    val sideways = across > 0f && (rb.left - ra.right > -ca || ra.left - rb.right > -cb)
    val stacked = !sideways && along > 0f
    if (!sideways && !stacked) return null
    // Said along x; one above the other is the same with x and y swapped.
    fun pt(x: Float, y: Float) = if (sideways) Offset(x, y) else Offset(y, x)
    val (l, lc, r, rc) =
        if (sideways) {
            if (ra.center.x <= rb.center.x) Four(ra.flat(), ca, rb.flat(), cb)
            else Four(rb.flat(), cb, ra.flat(), ca)
        } else {
            if (ra.center.y <= rb.center.y) Four(ra.turned(), ca, rb.turned(), cb)
            else Four(rb.turned(), cb, ra.turned(), ca)
        }
    val gap = r.left - l.right
    if (gap >= reach) return null
    val k = (1f - gap / reach).coerceIn(0f, 1f)
    val top = max(l.top, r.top)
    val bottom = min(l.bottom, r.bottom)
    val h = bottom - top
    if (h <= 0f) return null
    val mid = (top + bottom) / 2f
    val x0 = l.right - lc
    val x1 = r.left + rc
    if (x1 <= x0) return null
    val e = h / 2f * 0.9f
    val waist = h / 2f * 0.85f * k * k
    val bend = 2f * waist - e
    val xm = (x0 + x1) / 2f
    return Path().apply {
        val a0 = pt(x0, mid - e)
        moveTo(a0.x, a0.y)
        val c0 = pt(xm, mid - bend)
        val a1 = pt(x1, mid - e)
        quadraticTo(c0.x, c0.y, a1.x, a1.y)
        val a2 = pt(x1, mid + e)
        lineTo(a2.x, a2.y)
        val c1 = pt(xm, mid + bend)
        val a3 = pt(x0, mid + e)
        quadraticTo(c1.x, c1.y, a3.x, a3.y)
        close()
    }
}

private data class Four(val l: Rect, val lc: Float, val r: Rect, val rc: Float)

private fun Rect.flat() = this

private fun Rect.turned() = Rect(top, left, bottom, right)

/** `path` filled with glass, and the soft shadow under it. */
private fun DrawScope.drawGlass(path: Path, fill: Color, blur: Float, paint: Paint) {
    paint.color = fill
    val p = paint.asFrameworkPaint()
    if (blur > 0f) p.setShadowLayer(blur, 0f, blur / 3f, Shade.toArgb()) else p.clearShadowLayer()
    drawIntoCanvas { it.drawPath(path, paint) }
}

// The element of [glass]: alone it draws its glass; in a container it tells the container where
// it is laid out, and its content follows the glass the container draws.
private class GlassElement(
    val shape: GlassShape,
    val id: Any?,
    val fill: Color,
    val shadow: Dp,
    val hidden: (() -> Boolean)?,
) : ModifierNodeElement<GlassNode>() {
    override fun create() = GlassNode(shape, id, fill, shadow, hidden)

    override fun update(node: GlassNode) {
        if (node.id != id) node.forget()
        node.shape = shape
        node.id = id
        node.fill = fill
        node.shadow = shadow
        node.hidden = hidden
        node.piece?.let {
            it.shape = shape
            it.hidden = hidden
        }
    }

    override fun equals(other: Any?): Boolean =
        other is GlassElement &&
            other.shape == shape &&
            other.id == id &&
            other.fill == fill &&
            other.shadow == shadow &&
            other.hidden === hidden

    override fun hashCode(): Int =
        ((shape.hashCode() * 31 + id.hashCode()) * 31 + fill.hashCode()) * 31 + shadow.hashCode()
}

private class GlassNode(
    var shape: GlassShape,
    var id: Any?,
    var fill: Color,
    var shadow: Dp,
    var hidden: (() -> Boolean)?,
) :
    Modifier.Node(),
    CompositionLocalConsumerModifierNode,
    GlobalPositionAwareModifierNode,
    LayoutModifierNode,
    DrawModifierNode {
    private var space: GlassSpace? = null
    var piece: Piece? by mutableStateOf(null)
    private val paint = Paint()

    override fun onAttach() {
        space = currentValueOf(LocalGlass)
    }

    override fun onDetach() {
        forget()
        space = null
    }

    fun forget() {
        val p = piece ?: return
        piece = null
        space?.leave(p)
    }

    override fun onGloballyPositioned(coordinates: LayoutCoordinates) {
        val s = space ?: return
        val home = s.coords?.takeIf { it.isAttached } ?: return
        val r = home.localBoundingBoxOf(coordinates, clipBounds = false)
        val p = piece
        if (p == null) piece = s.join(shape, r, hidden) else s.move(p, r)
    }

    override fun MeasureScope.measure(measurable: Measurable, constraints: Constraints): MeasureResult {
        val pl = measurable.measure(constraints)
        return layout(pl.width, pl.height) {
            pl.placeWithLayer(0, 0) {
                // Where the glass is drawn, against where the layout has it: the content moves with
                // its centre, and comes in as it grows.
                val p = piece ?: return@placeWithLayer
                val t = p.target
                val d = p.drawn.value
                if (t.width <= 0f || t.height <= 0f) return@placeWithLayer
                translationX = d.center.x - t.center.x
                translationY = d.center.y - t.center.y
                alpha = ((min(d.width / t.width, d.height / t.height) - 0.5f) * 2f).coerceIn(0f, 1f)
            }
        }
    }

    override fun ContentDrawScope.draw() {
        if (space == null && hidden?.invoke() != true) {
            val r = Rect(Offset.Zero, size)
            val c = shape.radius(size, this)
            drawGlass(rounded(r, c), fill, shadow.toPx(), paint)
        }
        drawContent()
    }
}

// A morph over its container: the dim and the tap that closes it, and the one surface, the
// button's bounds at 0 and the menu's at 1, the button's face and the menu inside it.
@Composable
private fun BoxScope.MorphLayer(space: GlassSpace, m: Morph) {
    val out by remember(m) { derivedStateOf { m.p.value > 0f } }
    if (!out) return
    Spacer(
        Modifier.matchParentSize().drawBehind {
            drawRect(space.scrim, alpha = MENU_DIM * m.p.value.coerceIn(0f, 1f))
        })
    val path = remember { Path() }
    Layout(
        content = {
            // The button's face, drawn here while the glass is out: seen, not pressed or read.
            Box(
                Modifier.layoutId(FACE).clearAndSetSemantics {}.pointerInput(Unit) {
                    awaitPointerEventScope {
                        while (true) {
                            awaitPointerEvent(PointerEventPass.Initial).changes.forEach {
                                it.consume()
                            }
                        }
                    }
                }) {
                    m.collapsed()
                }
            Box(Modifier.layoutId(MENU)) {
                CompositionLocalProvider(LocalGlass provides null) { m.content() }
            }
        },
        modifier =
            Modifier.matchParentSize()
                // A tap outside the glass closes it; one on the menu but on nothing in it, nothing.
                .then(
                    if (m.expanded)
                        Modifier.pointerInput(m) {
                            detectTapGestures { at -> if (!m.at.contains(at)) m.onDismiss() }
                        }
                    else Modifier)
                .then(
                    if (m.reduced) Modifier.graphicsLayer { alpha = m.p.value.coerceIn(0f, 1f) }
                    else Modifier)
                .drawWithContent {
                    val r = m.at
                    if (r.width <= 0f || r.height <= 0f) return@drawWithContent
                    path.reset()
                    path.addRoundRect(RoundRect(r, CornerRadius(m.corner, m.corner)))
                    drawGlass(path, space.fill, space.blur, space.paint)
                    clipPath(path) { this@drawWithContent.drawContent() }
                },
    ) { measurables, c ->
        val w = c.maxWidth.toFloat()
        val h = c.maxHeight.toFloat()
        val margin = (if (m.sheet) Glass.sheetMargin else Glass.margin).toPx()
        val home = space.coords
        val slot =
            m.slot
                ?.takeIf { it.isAttached && home != null && home.isAttached }
                ?.let { home!!.localBoundingBoxOf(it, clipBounds = false) }
        val wide =
            (if (m.sheet) w - 2 * margin else min(m.menuWidth.toPx(), w - 2 * margin))
                .roundToInt()
                .coerceAtLeast(0)
        val tall = (h * MENU_TALL).roundToInt().coerceAtLeast(0)
        var face: androidx.compose.ui.layout.Placeable? = null
        var menu: androidx.compose.ui.layout.Placeable? = null
        for (x in measurables) {
            when (x.layoutId) {
                FACE ->
                    face =
                        x.measure(
                            if (slot == null) Constraints()
                            else
                                Constraints.fixed(
                                    slot.width.roundToInt().coerceAtLeast(0),
                                    slot.height.roundToInt().coerceAtLeast(0)))
                MENU -> menu = x.measure(Constraints(wide, wide, 0, tall))
            }
        }
        layout(c.maxWidth, c.maxHeight) {
            if (slot == null) return@layout
            val f = face!!
            val n = menu!!
            // The menu at the button's edge: growing up from a button low in the space, down from
            // one high in it; from its leading edge, its trailing edge, or its middle.
            val up = slot.center.y > h / 2f
            val left =
                when {
                    slot.center.x < w / 3f -> slot.left
                    slot.center.x > 2f * w / 3f -> slot.right - n.width
                    else -> slot.center.x - n.width / 2f
                }.coerceIn(margin, max(margin, w - margin - n.width))
            val top =
                if (m.sheet) max(margin, h - margin - n.height)
                else
                    (if (up) slot.bottom - n.height else slot.top).coerceIn(
                        margin, max(margin, h - margin - n.height))
            val menuAt = Rect(Offset(left, top), Size(n.width.toFloat(), n.height.toFloat()))
            // Reduced motion: the glass is the menu's whole way, and only its opacity moves.
            val p = if (m.reduced) 1f else m.p.value
            val q = p.coerceIn(0f, 1f)
            val r =
                Rect(
                    lerp(slot.left, menuAt.left, p),
                    lerp(slot.top, menuAt.top, p),
                    lerp(slot.right, menuAt.right, p),
                    lerp(slot.bottom, menuAt.bottom, p),
                )
            m.at = r
            m.corner =
                lerp(m.shape.radius(slot.size, this@Layout), m.menuCorner.toPx(), q)
                    .coerceAtMost(min(r.width, r.height) / 2f)
            // The face rides at the middle of the glass, gone by a third of the way.
            f.placeWithLayer(
                (r.center.x - f.width / 2f).roundToInt(), (r.center.y - f.height / 2f).roundToInt()) {
                    alpha = (1f - q / 0.3f).coerceIn(0f, 1f)
                }
            // The menu in its place, coming in over the rest, a little smaller at first about the
            // corner it grows from.
            n.placeWithLayer(menuAt.left.roundToInt(), menuAt.top.roundToInt()) {
                alpha = ((q - 0.25f) / 0.6f).coerceIn(0f, 1f)
                val s = lerp(0.85f, 1f, q)
                scaleX = s
                scaleY = s
                transformOrigin =
                    TransformOrigin(
                        ((slot.center.x - menuAt.left) / menuAt.width).coerceIn(0f, 1f),
                        if (up) 1f else 0f)
            }
        }
    }
}

private const val FACE = "face"
private const val MENU = "menu"
