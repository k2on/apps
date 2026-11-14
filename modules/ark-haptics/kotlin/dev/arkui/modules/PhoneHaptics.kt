// ark-haptics on Android (modules/ark-haptics/README.md): the `Haptics` interface UniFFI writes,
// over ArkUI's `View.perform` (ui/docs/motion.md, Haptics). An app that lists `haptics` in its
// modules.toml compiles this file in and registers one with its screens.
package dev.arkui.modules

import android.view.View
import dev.arkui.Feedback
import dev.arkui.perform
import uniffi.ark_haptics.Haptic
import uniffi.ark_haptics.Haptics

/** `ui/docs/motion.md`'s haptics, on the view the app is drawn in. */
class PhoneHaptics(private val view: View) : Haptics {
    override fun perform(haptic: Haptic) {
        // A warning is a success's weight here: ArkUI's two are what Android has an answer for.
        view.post { view.perform(if (haptic == Haptic.SELECTION) Feedback.Selection else Feedback.Success) }
    }
}
