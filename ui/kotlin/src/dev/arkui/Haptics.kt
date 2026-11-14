// The phone's answer in the hand, on the same events the iPhone's
// `.sensoryFeedback` answers: a success when something is kept — a
// transaction saved, approved, money moved or assigned, a show followed — and
// a selection tick when a choice steps — a month, a tick on a playlist, a
// digit typed into an amount, a transaction cleared. Apple's three rules for
// it: it is caused by the event it answers (it fires on the change itself,
// not on a tap that might not change anything), it lands on the same frame
// as what the eye sees change (it is performed as the change is composed),
// and it is kept for what matters, so that it still means something.
//
// Android's own constants, the nearest to iOS's: `CONFIRM` for a success
// (Android 11 and later; a keyboard tap before it), `SEGMENT_TICK` for a
// selection (Android 14 and later; the clock's tick before it). Performed
// on the view, so the system's haptics setting is respected.
package dev.arkui

import android.os.Build
import android.view.HapticFeedbackConstants
import android.view.View
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalView

/** What a haptic says: something kept, or a choice stepped. */
public enum class Feedback {
    Success,
    Selection,
}

/** `feedback` on this view, now: the platform's nearest constant to iOS's. */
public fun View.perform(feedback: Feedback): Boolean =
    performHapticFeedback(
        when (feedback) {
            Feedback.Success ->
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) HapticFeedbackConstants.CONFIRM
                else HapticFeedbackConstants.KEYBOARD_TAP
            Feedback.Selection ->
                if (Build.VERSION.SDK_INT >= 34) HapticFeedbackConstants.SEGMENT_TICK
                else HapticFeedbackConstants.CLOCK_TICK
        })

/**
 * SwiftUI's `.sensoryFeedback(feedback, trigger:)`: `feedback` each time `trigger` changes, and not
 * for the value it starts at. Performed as the change is composed, so it lands on the frame that
 * shows it.
 */
@Composable
public fun SensoryFeedback(feedback: Feedback, trigger: Any?) {
    val view = LocalView.current
    val seen = remember { Seen(trigger) }
    SideEffect {
        if (seen.value != trigger) {
            seen.value = trigger
            view.perform(feedback)
        }
    }
}

private class Seen(var value: Any?)
