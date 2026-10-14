// How often a list's rows and cards are composed, for the test that holds
// the scroll fix (`ScrollTests`): a hit per composition of a `TrackRow` or a
// `RecordCard`, while a test has installed a sink — and nothing at all
// otherwise, which is every run of the app: one volatile read per row.
package dev.harken.android

import androidx.compose.runtime.Composable
import androidx.compose.runtime.NonRestartableComposable
import androidx.compose.runtime.SideEffect

object Recompositions {
    /** Where a hit goes while a test listens; null, always, in the app. */
    @Volatile var sink: ((String) -> Unit)? = null

    /**
     * Count this composition as `what`, if anybody is counting. Neither restartable nor skippable,
     * so it runs every time its caller does: a skippable one would be skipped with its argument
     * unchanged and count nothing.
     */
    @Composable
    @NonRestartableComposable
    fun hit(what: String) {
        val s = sink ?: return
        SideEffect { s(what) }
    }
}
