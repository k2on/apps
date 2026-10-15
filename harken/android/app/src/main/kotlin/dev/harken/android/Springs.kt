// The numbers everything moves by, said once: one stiffness, a damping for
// where a thing is and another for how large it is, the most a settle may
// take, and the curve a gesture's preview follows. Nothing here knows what
// is moving — a page, the player, a sheet — so anything that moves can use
// it, and two things that move together move alike.
//
// A spring rather than a duration, because a spring starts from wherever
// the thing is and however fast it is going: a flick carries its speed into
// the settle, a tap starts it from rest, and either way it is done in about
// the same time. A tween is a fixed time whatever the distance, so a
// release near the end crawls through the rest of it.
//
// A gesture's preview is not the gesture's progress. Android's own back
// preview lets the page come only so far — free at first, then harder and
// harder, never past a limit — and letting go finishes the move or springs it
// back; [preview] is that curve, `limit · (1 − e^(−r / give))`.
package dev.harken.android

import androidx.compose.animation.core.AnimationVector
import androidx.compose.animation.core.DecayAnimationSpec
import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.SpringSpec
import androidx.compose.animation.core.TwoWayConverter
import androidx.compose.animation.core.VectorizedFiniteAnimationSpec
import androidx.compose.animation.core.exponentialDecay
import androidx.compose.animation.core.spring
import kotlin.math.exp

object Springs {
    /**
     * How stiff every spring is: a page or the player let go from rest settles to within half a
     * percent of the way in about 280 ms (critically damped), and crosses its end in about 210 ms
     * with the [MOVE] damping. Between Compose's `StiffnessMediumLow` (400) and `StiffnessMedium`
     * (1500).
     */
    const val STIFFNESS = 700f

    /**
     * Where a thing is: a whisper of overshoot, about 0.3% of the way, so a page or the player
     * lands rather than stops.
     */
    const val MOVE = 0.88f

    /**
     * How large a thing is and what of it shows — a scale, a clip, a dim: none. A page scaled past
     * its size, or clipped past its bounds, would show it.
     */
    const val SIZE = 1f

    /**
     * Where a spring over a 0..1 progress counts as there: a twentieth of a pixel of a page. Compose's
     * default is a hundredth of the way, which on a page is a last jump of several pixels.
     */
    const val REST = 0.0005f

    /** The most a settle takes, however slowly it was let go: it is where it was going by then. */
    const val CAP_MS = 300

    /** Where a thing is, over a 0..1 progress. */
    fun move(): SpringSpec<Float> = spring(MOVE, STIFFNESS, REST)

    /** How large a thing is, over a 0..1 progress. */
    fun size(): SpringSpec<Float> = spring(SIZE, STIFFNESS, REST)

    /** A settle of something dragged in pixels: [MOVE], from the release's speed, done by [CAP_MS]. */
    fun settle(): FiniteAnimationSpec<Float> = Capped(spring(MOVE, STIFFNESS, 0.5f), CAP_MS)

    /**
     * A fling that goes nowhere: a draggable's settle is then always its spring, carrying the
     * release's speed, never a decay that coasts the rest of the way at whatever speed was left.
     */
    fun noDecay(): DecayAnimationSpec<Float> = exponentialDecay(frictionMultiplier = 1e6f)

    /** How far a back gesture's preview comes, at most, as a share of what it previews. */
    const val PREVIEW = 0.1f

    /** How soon the preview stiffens: it has come 63% of [PREVIEW] by this much of the gesture. */
    const val GIVE = 0.12f

    /**
     * A gesture's raw progress `r`, 0 to 1, as how far its preview has come: in step with the finger
     * at first, then harder and harder, and never past [limit].
     */
    fun preview(r: Float, limit: Float = PREVIEW, give: Float = GIVE): Float = limit * (1f - exp(-r.coerceAtLeast(0f) / give))
}

/**
 * [spec], but never longer than [ms]: at [ms] it is where it was going. For a spring tuned to be
 * all but there by then, the cut is under a pixel; what it bounds is the case the tuning did not
 * see, a release so slow and so far that the spring's tail would otherwise be what is watched.
 */
class Capped<T>(private val spec: FiniteAnimationSpec<T>, private val ms: Int) : FiniteAnimationSpec<T> {
    override fun <V : AnimationVector> vectorize(converter: TwoWayConverter<T, V>): VectorizedFiniteAnimationSpec<V> =
        CappedVectorized(spec.vectorize(converter), ms * 1_000_000L)

    override fun equals(other: Any?) = other is Capped<*> && other.spec == spec && other.ms == ms

    override fun hashCode() = spec.hashCode() * 31 + ms
}

private class CappedVectorized<V : AnimationVector>(private val inner: VectorizedFiniteAnimationSpec<V>, private val cap: Long) : VectorizedFiniteAnimationSpec<V> {
    override fun getValueFromNanos(playTimeNanos: Long, initialValue: V, targetValue: V, initialVelocity: V): V =
        if (playTimeNanos >= cap) targetValue else inner.getValueFromNanos(playTimeNanos, initialValue, targetValue, initialVelocity)

    override fun getVelocityFromNanos(playTimeNanos: Long, initialValue: V, targetValue: V, initialVelocity: V): V =
        inner.getVelocityFromNanos(minOf(playTimeNanos, cap), initialValue, targetValue, initialVelocity)

    override fun getDurationNanos(initialValue: V, targetValue: V, initialVelocity: V): Long = minOf(cap, inner.getDurationNanos(initialValue, targetValue, initialVelocity))
}
