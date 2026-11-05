// Whether motion is reduced, said once. Android has one switch for it —
// Settings, Accessibility, "Remove animations", which sets the animator
// duration scale to 0 — and no separate "reduce" as the iPhone has; and
// Compose, left to itself, reads that switch by making every animation jump
// to its end. That is not what Apple's guidance means by reduced motion:
// reduced is a gentler motion, not none. A page that cross-fades in still
// says it arrived, a menu that fades in still says where it came from; what
// goes is the travel — the slide, the scale, the overshoot — that a moving
// eye has to follow.
//
// So everything that moves in arkui asks [reducedMotion] and, when it is
// on, takes its gentle path: a page, the expanding card, a glass morph
// cross-fade on a quick critically damped spring instead of growing, a
// cover's scale and a glass piece's glide are instant, and a release of
// something dragged settles without the momentum spring's bounce (the drag
// itself still follows the finger 1:1 — that is the finger's motion, not
// the app's). And the activity runs Compose on the system's own duration
// scale except when that scale is 0, where it runs at 1 ([keepMotion]):
// what the app does under "Remove animations" is then the app's choice, not
// a blanket jump. (Material's own pieces — its bottom sheets, its ripples —
// have no gentle path to take, and so move as they would with the setting
// off.)
//
// For now that choice is to animate anyway ([ReducedMotion.ALWAYS_ANIMATE]):
// the phone this is written on has animations removed, and its owner wants
// to see them. Flip the constant and the system's switch is obeyed again.
//
// A test or a screenshot forces either path with [LocalReducedMotion].
//
// Reduce Transparency and Increase Contrast have no clean equivalent on
// Android (high-contrast text is only reachable by reflection), so there is
// nothing here for them; the glass is already a solid surface (Glass.kt).
package dev.arkui

import android.animation.ValueAnimator
import androidx.activity.ComponentActivity
import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.snap
import androidx.compose.runtime.Composable
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.MotionDurationScale
import androidx.compose.ui.platform.compositionContext
import androidx.compose.ui.platform.createLifecycleAwareWindowRecomposer
import android.view.ViewGroup

public object ReducedMotion {
    /**
     * Animate whatever the system says. While true, "Remove animations" is ignored: every
     * animation runs as it would with the setting off, and [reducedMotion] is false unless a test
     * says otherwise. The owner's phone has the setting on and they want to see the motion for
     * now; set this to false to obey the setting again, and the gentle paths take over.
     */
    public const val ALWAYS_ANIMATE: Boolean = true

    /** Whether the system has animations removed: the animator duration scale is 0. */
    public fun system(): Boolean = !ValueAnimator.areAnimatorsEnabled()

    /** Whether motion is reduced, the system's setting as [ALWAYS_ANIMATE] lets it count. */
    public fun isOn(): Boolean = !ALWAYS_ANIMATE && system()

    /**
     * The duration scale Compose runs on in an activity that has [keepMotion]: the system's own
     * (a faster or slower scale from the developer options still counts), except 0, which is 1 —
     * the app decides what "removed" looks like, rather than every animation jumping to its end.
     */
    public fun scale(): Float {
        val s = ValueAnimator.getDurationScale()
        return if (s > 0f) s else 1f
    }
}

/**
 * Whether motion is reduced here: what a test or screenshot provides, or null for the system's
 * setting as [ReducedMotion.isOn] counts it.
 */
public val LocalReducedMotion: ProvidableCompositionLocal<Boolean?> = staticCompositionLocalOf {
    null
}

/**
 * Whether what moves here should take its gentle path: a cross-fade or an instant move in place of
 * a slide, a scale or an overshoot.
 */
@Composable
@ReadOnlyComposable
public fun reducedMotion(): Boolean = LocalReducedMotion.current ?: ReducedMotion.isOn()

/**
 * Runs this activity's Compose content on [ReducedMotion.scale] rather than the system's raw
 * duration scale: called before `setContent`, it gives the content view a recomposer of its own
 * whose animations read that scale. Everything composed under it — a sheet's or a menu's window
 * too, which take their parent's recomposer — moves by it.
 */
public fun ComponentActivity.keepMotion() {
    val root = window.decorView.findViewById<ViewGroup>(android.R.id.content) ?: return
    val scale =
        object : MotionDurationScale {
            override val scaleFactor: Float
                get() = ReducedMotion.scale()
        }
    root.compositionContext = root.createLifecycleAwareWindowRecomposer(scale, lifecycle)
}

/**
 * `spec`, unless motion is reduced: then the change is instant. For what only travels — a cover's
 * scale as it plays and pauses, a piece's glide — where the move itself says nothing the end state
 * does not.
 */
@Composable
@ReadOnlyComposable
public fun <T> unlessReduced(spec: FiniteAnimationSpec<T>): FiniteAnimationSpec<T> =
    if (reducedMotion()) snap() else spec
