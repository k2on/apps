// A core's views as a screen holds them (`ark_peer::store` behind an app's
// `Session`): a screen holds the view it shows while it is composed, and
// reads it again whenever the core says it moved — the same helper every
// Ark app on Android draws its views with, as `.holding(view)` is on the
// iPhone and the store itself is in iced.
//
//     val lines = watched.watch(MoneyView.Budget(month)) { session.budget(month).lines }
//
// Nothing here splices, counts or keeps rows: the core holds each view by
// count, keeps its rows spliced and says which views moved; this turns that
// into Compose's recomposition.
package dev.arkui

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.remember

/**
 * An app's views by key `V` (its `MoneyView`, its `HarkenView`): [hold] is its session's hold, and
 * [moved] is told — from the session's listener, on any thread — which views moved, which the
 * compositions reading them are recomposed for, on the main thread ([post]).
 */
public class Watched<V>(
    private val hold: (V) -> AutoCloseable,
    private val post: (() -> Unit) -> Unit = { f -> main.post(f) },
) {
    private val ticks = mutableStateMapOf<V, Int>()

    /** `views` moved: whatever reads one of them is read again. */
    public fun moved(views: List<V>) {
        post { for (v in views) ticks[v] = (ticks[v] ?: 0) + 1 }
    }

    /** How many times `view` has moved: what a composition reading it depends on. */
    public fun tick(view: V): Int = ticks[view] ?: 0

    /** `view` held until the answer is closed. */
    public fun hold(view: V): AutoCloseable = hold.invoke(view)

    private companion object {
        val main by lazy { Handler(Looper.getMainLooper()) }
    }
}

/**
 * `view` held while this is composed — the holding closed when it leaves — and `read` of it, read
 * again whenever it moved.
 */
@Composable
public fun <V, T> Watched<V>.watch(view: V, read: () -> T): T {
    val holding = remember(this, view) { hold(view) }
    DisposableEffect(holding) { onDispose { holding.close() } }
    val tick = tick(view)
    return remember(this, view, tick) { read() }
}
