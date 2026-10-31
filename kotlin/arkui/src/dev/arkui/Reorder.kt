// A list whose rows are held and dragged into a new order: a row held a
// moment lifts — a little larger, a shadow under it — and follows the
// finger, and the rows it passes glide out of its way, so the gap it would
// land in is always open under it. Let go, it settles into that gap, and
// the app is told from where to where.
//
// The rows that make way are the lazy list's own placement animation
// (`LazyItemScope.animateItem`) on arkui's springs: while a row is dragged
// the list is drawn in the order it has been dragged into, so every row
// laid out somewhere new glides there. The row dragged is not animated by
// the list; it is drawn where the finger has it — a place on the screen,
// not in the list — offset from wherever the list lays it out, so it never
// jumps: not when it changes place, not when the rows around it come and
// go, not when the list scrolls under it. Held near the list's top or
// bottom edge, the list scrolls, faster the nearer.
//
// A row passes another once its edge is past the other's middle: its
// bottom edge going down, its top edge going up. Whatever their heights,
// the row it just passed is then on the far side of its middle, so the two
// do not trade back and forth.
//
// A long press, not a press, starts a drag: a tap on a row still does what
// a tap did, and a drag that is not held first still scrolls the list. A
// screen reader moves a row a place at a time instead ([Reorder.actions]).
//
// **Sections.** A list can be rows under headers — a budget's categories in
// their groups (`rememberReorder` with `isHeader`). A row dragged past a
// header changes section, though never goes above the first, and the app is
// told where it landed ([Landing]: under which header, in front of which
// row). A header held moves its section, rows and all: while one is, the app
// draws the headers alone ([Reorder.folded]) and the list is kept on the
// header held, so a long section is passed as one row; let go, the sections
// open again around it.
//
// The app owns the order. Until the list it gives says the row moved —
// the session's answer comes back from the room — the order it was
// dragged into is kept, so the row does not jump back on the way.
package dev.arkui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.VisibilityThreshold
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LocalPinnableContainer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import kotlin.math.abs
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** How much larger a lifted row is drawn. */
private const val LIFT_SCALE = 0.02f

/** How far its shadow falls once lifted. */
private val LIFT_SHADOW = 8.dp

/** How long an order dragged into is drawn ahead of the app's saying so. */
private const val PENDING_MS = 2000L

/** How near the list's edge a held row scrolls it. */
private val EDGE = 56.dp

/** How far the list scrolls in a frame with the row held at its very edge. */
private val EDGE_SPEED = 12.dp

/** How the rows that make way move: arkui's spring for where a thing is. */
private val PLACEMENT: FiniteAnimationSpec<IntOffset> = spring(Springs.MOVE, Springs.STIFFNESS, IntOffset.VisibilityThreshold)

/** Where a row was let go in a list of sections: under which header, and in front of what. */
data class Landing<out K>(
    /** The header it is under; none for a header. */
    val section: K?,
    /**
     * What it is in front of: for a row, the next row of its section; for a header, the next header.
     * None when it is the last.
     */
    val before: K?,
)

/**
 * `order` with `key` moved to where `over` is — after it when it was before it, in front of it when
 * it was after — or null when it cannot go there. With headers (`isHeader`) a row never goes above
 * the first header, and a header moves with its section's rows and passes only another header: past
 * that header's section going down, in front of it going up.
 */
fun <K> reordered(order: List<K>, key: K, over: K, isHeader: (K) -> Boolean = { false }): List<K>? {
    val now = order.indexOf(key)
    val to = order.indexOf(over)
    if (now < 0 || to < 0 || now == to) return null
    if (!isHeader(key)) {
        val moved = order.toMutableList().apply { add(to, removeAt(now)) }
        return moved.takeUnless { isHeader(order[0]) && !isHeader(it[0]) }
    }
    if (!isHeader(over)) return null
    // A section is its header and the rows up to the next header.
    fun end(of: List<K>, at: Int) = (at + 1 until of.size).firstOrNull { isHeader(of[it]) } ?: of.size
    val section = order.subList(now, end(order, now))
    val rest = order.subList(0, now) + order.subList(end(order, now), order.size)
    val at = rest.indexOf(over).let { if (to > now) end(rest, it) else it }
    return rest.subList(0, at) + section + rest.subList(at, rest.size)
}

/** `order` as its sections: each header with the rows under it, up to the next. */
fun <K> sections(order: List<K>, isHeader: (K) -> Boolean): List<Pair<K, List<K>>> {
    val out = ArrayList<Pair<K, MutableList<K>>>()
    for (k in order) if (isHeader(k)) out.add(k to ArrayList<K>()) else out.lastOrNull()?.second?.add(k)
    return out
}

/** Where `key` is in `order`, as a [Landing]; null when it is not there. */
fun <K> landing(order: List<K>, key: K, isHeader: (K) -> Boolean): Landing<K>? {
    val at = order.indexOf(key)
    if (at < 0) return null
    if (isHeader(key)) return Landing(null, order.drop(at + 1).firstOrNull(isHeader))
    return Landing(order.take(at).lastOrNull(isHeader), order.getOrNull(at + 1)?.takeUnless(isHeader))
}

/**
 * A drag of one row of a lazy list into a new place. Remembered by [rememberReorder]; the list draws
 * its rows in [order], each in a [ReorderableItem].
 */
@Stable
class Reorder internal constructor(
    private val list: LazyListState,
    private val scope: CoroutineScope,
    private val haptics: HapticFeedback,
    private val density: Density,
    private val isHeader: ((Any) -> Boolean)?,
    private val lead: Int,
    private val onDrop: () -> ((key: Any, from: Int, after: List<Any>) -> Unit),
) {
    /** The row held, by its key, while it is and until it has settled. */
    var dragged by mutableStateOf<Any?>(null)
        private set

    /** A header is held: the app draws the headers alone, every section folded under its own. */
    val folded: Boolean
        get() = dragged?.let { isHeader?.invoke(it) } == true

    /** The order the rows are drawn in while one is dragged or settling: their keys. */
    private var drawn by mutableStateOf<List<Any>?>(null)

    /** Where the row held is drawn: its top, on the list's axis, as the list's items' offsets are. */
    private var top by mutableFloatStateOf(0f)

    /** Let go: the row settles by [offset], from where it was drawn to its place. */
    private var settling by mutableStateOf(false)

    /** The rows' keys as the app last gave them. */
    private var given: List<Any> = emptyList()

    /** The order when the row was picked up. */
    private var picked: List<Any> = emptyList()
    private var from = -1
    private var ending = false    internal val offset = Animatable(0f)
    internal val lift = Animatable(0f)

    private fun header(key: Any) = isHeader?.invoke(key) == true

    /**
     * `items` in the order to draw them: their own, or the order a row has been dragged into while it
     * is, and after it is let go until `items` comes back in that order.
     */
    fun <T> order(items: List<T>, key: (T) -> Any): List<T> {
        val keys = items.map(key)
        given = keys
        val order = drawn ?: return items
        val byKey = items.associateBy(key)
        // A row the app has taken out meanwhile is gone from the drawing too; one it has added is
        // drawn after the rest.
        return order.mapNotNull { byKey[it] } + items.filter { key(it) !in order }
    }

    internal fun settled(keys: List<Any>) {
        if (dragged == null && drawn == keys) drawn = null
    }

    /** How far the row `key` is drawn from where the list lays it out. */
    internal fun shift(key: Any): Float =
        if (settling) offset.value
        else list.layoutInfo.visibleItemsInfo.firstOrNull { it.key == key }?.let { top - it.offset } ?: 0f

    internal fun start(key: Any) {
        if (dragged != null) return
        val keys = drawn ?: given
        val at = keys.indexOf(key)
        val held = list.layoutInfo.visibleItemsInfo.firstOrNull { it.key == key }
        if (at < 0 || held == null) return
        from = at
        picked = keys
        drawn = keys
        top = held.offset.toFloat()
        ending = false
        dragged = key
        // Folding the sections moves every header. The list is kept on the one held where the folded
        // list has it — after the rows before the reordered ones, one row a header — so it is still
        // in the list, under the finger.
        if (header(key)) list.requestScrollToItem(lead + keys.take(at).count { header(it) }, -held.offset)
        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
        scope.launch { lift.animateTo(1f, Springs.size()) }
        scope.launch { edges(key) }
    }

    /**
     * What a screen reader offers for `key` in place of a drag: a place up, and a place down. A
     * header passes the next header's section, as a drag of it does.
     */
    fun actions(key: Any): List<CustomAccessibilityAction> =
        listOf(CustomAccessibilityAction("Move up") { nudge(key, -1) }, CustomAccessibilityAction("Move down") { nudge(key, 1) })

    private fun nudge(key: Any, by: Int): Boolean {
        if (dragged != null) return false
        val order = drawn ?: given
        val among = if (header(key)) order.filter { header(it) } else order
        val at = among.indexOf(key)
        if (at < 0) return false
        val over = among.getOrNull(at + by) ?: return false
        val next = reordered(order, key, over) { header(it) } ?: return false
        drawn = next
        scope.launch {
            delay(PENDING_MS)
            if (dragged == null) drawn = null
        }
        onDrop()(key, order.indexOf(key), next)
        return true
    }

    internal fun drag(dy: Float) {
        if (dragged == null || ending) return
        top += dy
        step()
    }

    // The row held is past the middle of another: it takes that one's place, or the farthest one's
    // it may take when it passed several at once.
    private fun step() {
        val key = dragged ?: return
        val order = drawn ?: return
        val items = list.layoutInfo.visibleItemsInfo
        // Not before the list is laid out in the order last drawn: until then the rows are where
        // they were, and the row just passed would be passed back.
        val shown = items.map { it.key }.filter { it in order }
        if (shown != order.filter { it in shown }) return
        val held = items.firstOrNull { it.key == key } ?: return
        val bottom = top + held.size
        val next =
            items
                .filter { o ->
                    o.key != key && o.key in order &&
                        (if (o.offset > held.offset) bottom > o.offset + o.size / 2f else top < o.offset + o.size / 2f)
                }
                .sortedByDescending { abs(it.offset - held.offset) }
                .firstNotNullOfOrNull { o -> reordered(order, key, o.key) { header(it) } } ?: return
        drawn = next
        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
    }

    // While `key` is held near an edge the list scrolls under it, and it passes what comes by.
    private suspend fun edges(key: Any) {
        val edge = with(density) { EDGE.toPx() }
        val speed = with(density) { EDGE_SPEED.toPx() }
        while (dragged == key && !ending) {
            withFrameNanos {}
            val info = list.layoutInfo
            val size = info.visibleItemsInfo.firstOrNull { it.key == key }?.size ?: continue
            val into =
                when {
                    top < info.viewportStartOffset + edge -> (top - info.viewportStartOffset - edge) / edge
                    top + size > info.viewportEndOffset - edge -> (top + size - info.viewportEndOffset + edge) / edge
                    else -> 0f
                }
            if (into == 0f || ending) continue
            list.scrollBy(speed * into.coerceIn(-1f, 1f))
            step()
        }
    }

    internal fun end() {
        val key = dragged ?: return
        if (ending) return
        ending = true
        val order = drawn ?: picked
        val slot = list.layoutInfo.visibleItemsInfo.firstOrNull { it.key == key }?.offset
        scope.launch {
            offset.snapTo(if (slot != null) top - slot else 0f)
            settling = true
            launch { lift.animateTo(0f, Springs.size()) }
            offset.animateTo(0f, Springs.settle())
            settling = false
            dragged = null
            // Drawn in the new order until the app's list says so, or for as long as it might take.
            delay(PENDING_MS)
            if (dragged == null) drawn = null
        }
        if (order != picked) onDrop()(key, from, order)
    }
}

/** A [Reorder] of the rows of `list`; `onMove(from, to)` is told when one is let go somewhere new. */
@Composable
fun rememberReorder(list: LazyListState, onMove: (from: Int, to: Int) -> Unit): Reorder {
    val move by rememberUpdatedState(onMove)
    return rememberDrag(list, null, 0) { key, from, after -> move(from, after.indexOf(key)) }
}

/**
 * A [Reorder] of the rows of `list` in sections, each header (`isHeader`) heading the rows after it up
 * to the next: `onDrop(key, landing)` is told where a row or a header was let go when it moved. `lead`
 * is how many of the list's items come before the first header: where the folded headers start.
 */
@Composable
fun rememberReorder(
    list: LazyListState,
    lead: Int,
    isHeader: (Any) -> Boolean,
    onDrop: (key: Any, landing: Landing<Any>) -> Unit,
): Reorder {
    val drop by rememberUpdatedState(onDrop)
    val header by rememberUpdatedState(isHeader)
    return rememberDrag(list, { k -> header(k) }, lead) { key, _, after -> landing(after, key) { header(it) }?.let { drop(key, it) } }
}

@Composable
private fun rememberDrag(
    list: LazyListState,
    isHeader: ((Any) -> Boolean)?,
    lead: Int,
    onDrop: (key: Any, from: Int, after: List<Any>) -> Unit,
): Reorder {
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    val drop by rememberUpdatedState(onDrop)
    return remember(list) { Reorder(list, scope, haptics, density, isHeader, lead) { drop } }
}

/**
 * A row of a reorderable list that is not itself reordered but makes way for those that are — a
 * section's foot, a gap — gliding as they glide.
 */
fun Modifier.makingWay(scope: LazyItemScope): Modifier =
    with(scope) { this@makingWay.animateItem(fadeInSpec = null, fadeOutSpec = null, placementSpec = PLACEMENT) }

/**
 * One row of a reorderable list, keyed `key` as the lazy list has it: held and dragged it moves,
 * and it makes way for another. `keys` are every row's, in the order the app gave them, so the drawing
 * can let go of an order it has caught up with. Lifted, it is drawn on the scheme's raised container
 * in `shape`; `modifier` is outside the row, so a padding there is not lifted with it.
 */
@Composable
fun LazyItemScope.ReorderableItem(
    reorder: Reorder,
    key: Any,
    keys: List<Any>,
    modifier: Modifier = Modifier,
    shape: Shape = RectangleShape,
    content: @Composable () -> Unit,
) {
    LaunchedEffect(keys) { reorder.settled(keys) }
    val held = reorder.dragged == key
    // Held, the row stays composed though the list scrolls it out of sight a moment, so the drag
    // holding it goes on.
    val pins = LocalPinnableContainer.current
    DisposableEffect(held, pins) {
        val pinned = if (held) pins?.pin() else null
        onDispose { pinned?.release() }
    }
    val ground = MaterialTheme.colorScheme.surfaceContainerHigh
    Box(
        modifier
            .then(if (held) Modifier.zIndex(1f) else Modifier.makingWay(this))
            .graphicsLayer {
                if (held) {
                    val l = reorder.lift.value
                    translationY = reorder.shift(key)
                    scaleX = 1f + LIFT_SCALE * l
                    scaleY = 1f + LIFT_SCALE * l
                    shadowElevation = LIFT_SHADOW.toPx() * l
                    this.shape = shape
                    clip = true
                }
            }
            .background(if (held) ground else Color.Transparent, shape)
            .pointerInput(key) {
                detectDragGesturesAfterLongPress(
                    onDragStart = { reorder.start(key) },
                    onDrag = { change, amount ->
                        change.consume()
                        reorder.drag(amount.y)
                    },
                    onDragEnd = { reorder.end() },
                    onDragCancel = { reorder.end() },
                )
            }
    ) {
        content()
    }
}
