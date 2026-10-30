// A row that does one thing when it is swiped toward the end — approve,
// archive, mark done — the way Gmail's rows do: the action's colour and
// glyph come out from under the row as it moves, grow once it has gone far
// enough to count, and the phone ticks there. Letting go past that point
// acts and the row springs home; whether it then leaves the list is the
// list's business (an approved transaction leaves an inbox because the
// inbox no longer holds it, not because the row was thrown away).
//
// The same action is on the row for TalkBack and Switch Access, which do
// not swipe: a custom action named `label`.
package dev.arkui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics

/**
 * `row`, swiped toward the end to `onAct`: `label` and `icon` say what that does, on `container`
 * (the primary container unless said). `enabled` false is a row that does not move.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
public fun SwipeToAct(
    label: String,
    icon: ImageVector,
    onAct: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    container: Color = MaterialTheme.colorScheme.primaryContainer,
    onContainer: Color = MaterialTheme.colorScheme.onPrimaryContainer,
    row: @Composable () -> Unit,
) {
    val act by rememberUpdatedState(onAct)
    val haptics = LocalHapticFeedback.current
    val state =
        rememberSwipeToDismissBoxState(
            confirmValueChange = { v ->
                if (v == SwipeToDismissBoxValue.StartToEnd) act()
                // Never left dismissed: the row comes home, and the list says whether it stays.
                false
            })
    // The tick, once, as the swipe passes the point where letting go acts.
    LaunchedEffect(state.targetValue) {
        if (state.targetValue == SwipeToDismissBoxValue.StartToEnd) haptics.performHapticFeedback(HapticFeedbackType.LongPress)
    }
    SwipeToDismissBox(
        state = state,
        enableDismissFromStartToEnd = enabled,
        enableDismissFromEndToStart = false,
        modifier =
            modifier.semantics {
                if (enabled) customActions = listOf(CustomAccessibilityAction(label) { act(); true })
            },
        backgroundContent = {
            val armed = state.targetValue == SwipeToDismissBoxValue.StartToEnd
            val grow by animateFloatAsState(if (armed) 1.15f else 0.85f, label = "swipe-glyph")
            Row(
                Modifier.fillMaxSize().background(container).padding(horizontal = Space.xl),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.Start) {
                    Icon(icon, contentDescription = null, tint = onContainer, modifier = Modifier.scale(grow))
                    Spacer(Modifier.width(Space.m))
                    Text(label, color = onContainer, style = MaterialTheme.typography.labelLarge)
                }
        },
    ) {
        row()
    }
}
