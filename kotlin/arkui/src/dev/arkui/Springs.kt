// The numbers everything moves by, said once: one stiffness, a damping for
// a move and another for a move a flick threw, the most a settle may take,
// the curve a gesture's preview follows, and where a flick is going. Nothing
// here knows what is moving — a page, the player, a sheet — so anything that
// moves can use it, and two things that move together move alike.
//
// A spring rather than a duration, because a spring starts from wherever
// the thing is and however fast it is going: a flick carries its speed into
// the settle, a tap starts it from rest, and either way it is done in about
// the same time. A tween is a fixed time whatever the distance, so a
// release near the end crawls through the rest of it.
//
// Critically damped unless the finger threw it. A thing that moves because
// it was tapped, or because the app moved it, arrives and stops: an
// overshoot there is the interface showing off, and the eye reads it as a
// wobble. A thing let go still moving has momentum the finger gave it, and
// a little overshoot ([MOMENTUM]) is that momentum spending itself, as a
// thrown thing does — so a release picks its spring by its speed
// ([settle]), and only a flick bounces. Reduced motion never does.
//
// Where a flick lands is projected, not guessed from where it was let go:
// [project] is the distance a scroll decelerating at iOS's normal rate would
// still cover from that speed, and a release snaps to whichever rest is
// nearest that projected point ([nearest]) — so a short fast flick goes,
// and a long slow drag let go near the middle goes wherever it is nearer.
//
// A gesture's preview is not the gesture's progress. Android's own back
// preview lets the page come only so far — free at first, then harder and
// harder, never past a limit — and letting go finishes the move or springs it
// back; [preview] is that curve, `limit · (1 − e^(−r / give))`.
package dev.arkui

import androidx.compose.animation.core.AnimationVector
import androidx.compose.animation.core.DecayAnimationSpec
import androidx.compose.animation.core.FiniteAnimationSpec
import androidx.compose.animation.core.SpringSpec
import androidx.compose.animation.core.TwoWayConverter
import androidx.compose.animation.core.VectorizedFiniteAnimationSpec
import androidx.compose.animation.core.exponentialDecay
import androidx.compose.animation.core.spring
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.abs
import kotlin.math.exp

public object Springs {
    /**
     * How stiff every spring is: a page or the player let go from rest settles to within half a
     * percent of the way in about 280 ms (critically damped), and a flick's settle crosses its end
     * in about 200 ms on [MOMENTUM]. Between Compose's `StiffnessMediumLow` (400) and
     * `StiffnessMedium` (1500).
     */
    public const val STIFFNESS: Float = 700f

    /**
     * Where a thing is, when nothing threw it there — a tap, the app moving it, a drag let go at
     * rest: critically damped, so it arrives and stops, Apple's default for anything moved.
     */
    public const val MOVE: Float = 1f

    /**
     * Where a thing is, when a flick threw it there: Apple's 0.8, an overshoot of about 1.5% of the
     * way, the flick's momentum spending itself. Only ever chosen by [settle], from the release's
     * speed.
     */
    public const val MOMENTUM: Float = 0.8f

    /**
     * How large a thing is and what of it shows — a scale, a clip, a dim: none. A page scaled past
     * its size, or clipped past its bounds, would show it.
     */
    public const val SIZE: Float = 1f

    /**
     * Where a spring over a 0..1 progress counts as there: a twentieth of a pixel of a page.
     * Compose's default is a hundredth of the way, which on a page is a last jump of several
     * pixels.
     */
    public const val REST: Float = 0.0005f

    /** The most a settle takes, however slowly it was let go: it is where it was going by then. */
    public const val CAP_MS: Int = 300

    /**
     * How fast a release has to be moving to count as a flick, and so settle on [MOMENTUM]: about
     * the speed of a deliberate throw, well above the drift a finger has when it is lifted from a
     * drag it meant to stop.
     */
    public val FLICK: Dp = 500.dp

    /**
     * How quickly a projected flick slows: iOS's `UIScrollView.DecelerationRate.normal`, the
     * fraction of its speed a scroll keeps each millisecond.
     */
    public const val DECELERATION: Float = 0.998f

    /**
     * A snappier deceleration, iOS's `.fast`: for a release that decides an act rather than a
     * place — a skip, going back — where only a real flick should count for much more than the
     * distance it was dragged. About a tenth of a second's worth.
     */
    public const val SNAPPY: Float = 0.99f

    /** Where a thing is, over a 0..1 progress: critically damped. */
    public fun move(): SpringSpec<Float> = spring(MOVE, STIFFNESS, REST)

    /** How large a thing is, over a 0..1 progress. */
    public fun size(): SpringSpec<Float> = spring(SIZE, STIFFNESS, REST)

    /** Whether a release at `velocity`, in pixels a second, was a flick. */
    public fun isFlick(velocity: Float, density: Density): Boolean =
        abs(velocity) >= with(density) { FLICK.toPx() }

    /**
     * A settle of something dragged in pixels, done by [CAP_MS]: critically damped, unless it was
     * let go at a flick's speed ([isFlick]) — then on [MOMENTUM], the throw's overshoot — and never
     * that when motion is `reduced`. Started from the release's own speed by its caller.
     */
    public fun settle(
        velocity: Float = 0f,
        density: Density? = null,
        reduced: Boolean = false,
    ): FiniteAnimationSpec<Float> {
        val thrown = !reduced && density != null && isFlick(velocity, density)
        return Capped(spring(if (thrown) MOMENTUM else MOVE, STIFFNESS, 0.5f), CAP_MS)
    }

    /**
     * How much further a thing let go at `velocity` (in units a second) would go before it
     * stopped, decelerating as a scroll does at [rate]: Apple's projection from *Designing Fluid
     * Interfaces*, `(v / 1000) · d / (1 − d)`. About half a second's worth at the normal rate.
     */
    public fun project(velocity: Float, rate: Float = DECELERATION): Float =
        velocity / 1000f * rate / (1f - rate)

    /**
     * Of `rests`, the one nearest where a thing at `at` let go at `velocity` is going ([project]):
     * a flick is thrown to the rest it is headed for, a release at rest stays with the nearer one.
     */
    public fun nearest(at: Float, velocity: Float, rests: Iterable<Float>): Float {
        val to = at + project(velocity)
        return rests.minByOrNull { abs(it - to) } ?: at
    }

    /**
     * A fling that goes nowhere: a draggable's settle is then always its spring, carrying the
     * release's speed, never a decay that coasts the rest of the way at whatever speed was left.
     */
    public fun noDecay(): DecayAnimationSpec<Float> = exponentialDecay(frictionMultiplier = 1e6f)

    /**
     * How stiff a fade is: critically damped, it is all but done in about 150 ms — a tab let go
     * past the back gesture's threshold, going.
     */
    public const val FADE_STIFFNESS: Float = 2500f

    /**
     * Something going, quickly: critically damped at [FADE_STIFFNESS]. Also reduced motion's
     * cross-fade, everywhere a thing would otherwise travel.
     */
    public fun fade(): SpringSpec<Float> = spring(SIZE, FADE_STIFFNESS, REST)

    /** How far a back gesture's preview comes, at most, as a share of what it previews. */
    public const val PREVIEW: Float = 0.1f

    /** How soon the preview stiffens: it has come 63% of [PREVIEW] by this much of the gesture. */
    public const val GIVE: Float = 0.12f

    /**
     * A gesture's raw progress `r`, 0 to 1, as how far its preview has come: in step with the
     * finger at first, then harder and harder, and never past [limit].
     */
    public fun preview(r: Float, limit: Float = PREVIEW, give: Float = GIVE): Float =
        limit * (1f - exp(-r.coerceAtLeast(0f) / give))
}

/**
 * [spec], but never longer than [ms]: at [ms] it is where it was going. For a spring tuned to be
 * all but there by then, the cut is under a pixel; what it bounds is the case the tuning did not
 * see, a release so slow and so far that the spring's tail would otherwise be what is watched.
 */
public class Capped<T>(private val spec: FiniteAnimationSpec<T>, private val ms: Int) :
    FiniteAnimationSpec<T> {
    override fun <V : AnimationVector> vectorize(
        converter: TwoWayConverter<T, V>
    ): VectorizedFiniteAnimationSpec<V> =
        CappedVectorized(spec.vectorize(converter), ms * 1_000_000L)

    override fun equals(other: Any?): Boolean =
        other is Capped<*> && other.spec == spec && other.ms == ms

    override fun hashCode(): Int = spec.hashCode() * 31 + ms
}

private class CappedVectorized<V : AnimationVector>(
    private val inner: VectorizedFiniteAnimationSpec<V>,
    private val cap: Long
) : VectorizedFiniteAnimationSpec<V> {
    override fun getValueFromNanos(
        playTimeNanos: Long,
        initialValue: V,
        targetValue: V,
        initialVelocity: V
    ): V =
        if (playTimeNanos >= cap) targetValue
        else inner.getValueFromNanos(playTimeNanos, initialValue, targetValue, initialVelocity)

    override fun getVelocityFromNanos(
        playTimeNanos: Long,
        initialValue: V,
        targetValue: V,
        initialVelocity: V
    ): V =
        inner.getVelocityFromNanos(
            minOf(playTimeNanos, cap), initialValue, targetValue, initialVelocity)

    override fun getDurationNanos(initialValue: V, targetValue: V, initialVelocity: V): Long =
        minOf(cap, inner.getDurationNanos(initialValue, targetValue, initialVelocity))
}
