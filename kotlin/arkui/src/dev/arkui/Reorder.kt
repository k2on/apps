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
// the list; it is drawn where the finger has it, by an offset that is
// carried across each change of its place so it never jumps.
//
// A long press, not a press, starts a drag: a tap on a row still does what
// a tap did, and a drag that is not held first still scrolls the list.
//
// The app owns the order. Until the list it gives says the row moved —
// the session's answer comes back from the room — the order it was
// dragged into is kept, so the row does not jump back on the way.
package dev.arkui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.VisibilityThreshold
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** How much larger a lifted row is drawn. */
private const val LIFT_SCALE = 0.02f

/** How far its shadow falls once lifted. */
private val LIFT_SHADOW = 8.dp

/** How long an order dragged into is drawn ahead of the app's saying so. */
private const val PENDING_MS = 2000L

/**
 * A drag of one row of a lazy list into a new place. Remembered by [rememberReorder]; the list draws
 * its rows in [order], each in a [ReorderableItem].
 */
@Stable
class Reorder internal constructor(
    private val list: LazyListState,
    private val scope: CoroutineScope,
    private val haptics: HapticFeedback,
    private val onMove: () -> ((from: Int, to: Int) -> Unit),
) {
    /** The row held, by its key, while it is. */
    var dragged by mutableStateOf<Any?>(null)
        private set

    /** The order the rows are drawn in while one is dragged or settling: their keys. */
    private var drawn by mutableStateOf<List<Any>?>(null)

    /** The rows' keys as the app last gave them. */
    private var given: List<Any> = emptyList()
    private var from = -1
    internal val offset = Animatable(0f)
    internal val lift = Animatable(0f)

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

    internal fun start(key: Any) {
        val keys = drawn ?: given
        val at = keys.indexOf(key)
        if (at < 0) return
        from = at
        drawn = keys
        dragged = key
        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
        scope.launch { offset.snapTo(0f) }
        scope.launch { lift.animateTo(1f, Springs.size()) }
    }

    internal fun drag(dy: Float) {
        val key = dragged ?: return
        val order = drawn ?: return
        scope.launch {
            offset.snapTo(offset.value + dy)
            val items = list.layoutInfo.visibleItemsInfo
            val held = items.firstOrNull { it.key == key } ?: return@launch
            val middle = held.offset + offset.value + held.size / 2f
            val over =
                items.firstOrNull {
                    it.key != key && it.key in order && middle >= it.offset && middle < it.offset + it.size
                } ?: return@launch
            val to = order.indexOf(over.key)
            val now = order.indexOf(key)
            if (to < 0 || now < 0) return@launch
            // The row takes the place of the one under its middle, which moves over to make room;
            // laid out there from the next frame, it is offset by the difference, so it stays under
            // the finger.
            val lands = if (to > now) held.offset + over.size else over.offset
            drawn = order.toMutableList().apply { add(to, removeAt(now)) }
            offset.snapTo(offset.value - (lands - held.offset))
        }
    }

    internal fun end() {
        val key = dragged ?: return
        val order = drawn ?: return
        val to = order.indexOf(key)
        val fromAt = from
        scope.launch {
            launch { lift.animateTo(0f, Springs.size()) }
            offset.animateTo(0f, Springs.settle())
            dragged = null
            // Drawn in the new order until the app's list says so, or for as long as it might take.
            delay(PENDING_MS)
            if (dragged == null) drawn = null
        }
        if (to >= 0 && to != fromAt) onMove()(fromAt, to)
    }
}

/** A [Reorder] of the rows of `list`; `onMove(from, to)` is told when one is let go somewhere new. */
@Composable
fun rememberReorder(list: LazyListState, onMove: (from: Int, to: Int) -> Unit): Reorder {
    val scope = rememberCoroutineScope()
    val haptics = LocalHapticFeedback.current
    val move by rememberUpdatedState(onMove)
    return remember(list) { Reorder(list, scope, haptics) { move } }
}

/**
 * One row of a reorderable list, keyed `key` as the lazy list has it: held and dragged it moves,
 * and it makes way for another. `keys` are every row's, in the order the app gave them, so the drawing
 * can let go of an order it has caught up with.
 */
@Composable
fun LazyItemScope.ReorderableItem(reorder: Reorder, key: Any, keys: List<Any>, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    LaunchedEffect(keys) { reorder.settled(keys) }
    val held = reorder.dragged == key
    val ground = MaterialTheme.colorScheme.surfaceContainerHigh
    Box(
        modifier
            .then(
                if (held) Modifier.zIndex(1f)
                else Modifier.animateItem(fadeInSpec = null, fadeOutSpec = null, placementSpec = spring(Springs.MOVE, Springs.STIFFNESS, IntOffset.VisibilityThreshold))
            )
            .graphicsLayer {
                if (held) {
                    val l = reorder.lift.value
                    translationY = reorder.offset.value
                    scaleX = 1f + LIFT_SCALE * l
                    scaleY = 1f + LIFT_SCALE * l
                    shadowElevation = LIFT_SHADOW.toPx() * l
                }
            }
            .background(if (held) ground else Color.Transparent)
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
