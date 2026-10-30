// How often an app's parts are composed, for the tests that hold a list
// still while something beside it ticks: a hit per composition of whatever
// calls `hit` — harken counts its top, its frame, each screen, the mini
// player and the position's two leaves, a track's row, and a `CoverCard`
// here — while a test has installed a sink, and nothing at all otherwise,
// which is every run of the app: one volatile read per composition.
package dev.arkui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.NonRestartableComposable
import androidx.compose.runtime.SideEffect

public object Recompositions {
    /** Where a hit goes while a test listens; null, always, in the app. */
    @Volatile public var sink: ((String) -> Unit)? = null

    /**
     * Count this composition as `what`, if anybody is counting. Neither restartable nor skippable,
     * so it runs every time its caller does: a skippable one would be skipped with its argument
     * unchanged and count nothing.
     */
    @Composable
    @NonRestartableComposable
    public fun hit(what: String) {
        val s = sink ?: return
        SideEffect { s(what) }
    }
}
