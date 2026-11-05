// Where a thrown thing comes to rest, and how fast it is going relative to
// the way it has left, as Apple's Designing Fluid Interfaces has them: a
// gesture let go is projected forward by its velocity, the way a scroll
// view decelerates, and what it lands on is chosen from where it would stop,
// not from where the finger left it; then the spring it lands on starts at
// the finger's speed. Foundation alone, so the arithmetic is tested on Linux.
import Foundation

public enum Momentum {
    /// How fast a projection slows, per millisecond: a scroll view's normal
    /// deceleration, and a snappier one for a throw that should not go far.
    /// arkui's motion tokens (`MotionTokens.deceleration`, `.snappy`).
    public static let normal: Double = MotionTokens.deceleration
    public static let snappy: Double = MotionTokens.snappy

    /// How much further something let go at `velocity` (points a second)
    /// travels before it stops, decelerating at `rate`: UIScrollView's
    /// exponential decay, not the textbook's `v²/2a`.
    public static func project(_ velocity: Double, rate: Double = normal) -> Double {
        (velocity / 1000) * rate / (1 - rate)
    }

    /// Of `points`, the one nearest to where a thing at `at`, let go at
    /// `velocity`, would come to rest.
    public static func target(_ points: [Double], at: Double, velocity: Double, rate: Double = normal) -> Double? {
        let rest = at + project(velocity, rate: rate)
        return points.min { abs($0 - rest) < abs($1 - rest) }
    }

    /// `velocity` as a share of the way left from `from` to `to`, a second:
    /// what a spring that counts its own progress from nought to one takes as
    /// its start. Nought when there is no way left to go.
    public static func relative(_ velocity: Double, from: Double, to: Double) -> Double {
        let gap = to - from
        return abs(gap) < 0.5 ? 0 : velocity / gap
    }

    /// How far a thing dragged `x` points past its edge is drawn past it:
    /// Apple's rubber band, `(x · d · c) / (d + c · |x|)`, where `d` is the
    /// room it is dragged in (`limit`) and `c` `MotionTokens.rubberBand`
    /// (0.55). It follows the finger at first and less and less, never
    /// reaching `d`; the sign is the drag's.
    public static func band(_ x: Double, limit: Double, c: Double = MotionTokens.rubberBand) -> Double {
        guard limit > 0 else { return 0 }
        return (x * limit * c) / (limit + c * abs(x))
    }
}
