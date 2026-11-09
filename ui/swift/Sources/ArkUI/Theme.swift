// The look every app shares and an app colours in: the spacing, how things
// move, colours given for both sides, and the derived square's palette as an
// environment value an app sets once at its root.
import SwiftUI
import UIKit

extension Color {
    /// One colour for light and another for dark, as the trait collection says.
    public init(light: UInt32, dark: UInt32) {
        self.init(uiColor: UIColor { t in UIColor(rgb: t.userInterfaceStyle == .dark ? dark : light) })
    }
}

extension UIColor {
    public convenience init(rgb: UInt32, alpha: CGFloat = 1) {
        self.init(red: CGFloat((rgb >> 16) & 0xff) / 255, green: CGFloat((rgb >> 8) & 0xff) / 255, blue: CGFloat(rgb & 0xff) / 255, alpha: alpha)
    }
}

/// The spacings, so every screen has the same rhythm: Android's `Space`.
public enum Space {
    public static let xs: CGFloat = 4
    public static let s: CGFloat = 8
    public static let m: CGFloat = 12
    public static let l: CGFloat = 16
    public static let xl: CGFloat = 24
    public static let xxl: CGFloat = 32
    /// Where a page's content starts from its edges.
    public static let page: CGFloat = 16
    /// A picture on a shelf.
    public static let shelf: CGFloat = 148
    /// A picture beside a row.
    public static let thumb: CGFloat = 48
    /// A page's header picture.
    public static let header: CGFloat = 200
    /// Corners: a picture, a card.
    public static let corner: CGFloat = 8
}

/// The springs everything moves by, made of arkui's motion tokens
/// (`MotionTokens`, ui/theme.toml `[motion]`), so they are Android's
/// (`Springs.kt`) and the desktop's to the number: one stiffness, 700 at
/// unit mass, critically damped — a thing put somewhere lands there without
/// passing it, as Apple's own moves do — and a little under that, 0.8, only
/// for a thing that was thrown: let go by a finger still moving
/// (`released`), which hands the spring its speed, so what it overshoots by
/// is the throw's. SwiftUI's `spring(response:dampingFraction:)` takes the
/// same spring as its period, `2π/√(k/m)`: `MotionTokens.response`.
///
/// Under Reduce Motion (`\.reducesMotion`) each is `reduced(_:)` into
/// `calm`, a short cross-fade's spring with no overshoot.
public enum Springs {
    public static let stiffness: Double = MotionTokens.stiffness
    public static let response: Double = MotionTokens.response
    /// Where a thing is. No overshoot: nothing threw it.
    public static let move = Animation.spring(response: response, dampingFraction: MotionTokens.move)
    /// How large a thing is: a scale, a clip, a dim. No overshoot.
    public static let size = Animation.spring(response: response, dampingFraction: MotionTokens.size)
    /// Something going, quickly: critically damped at the fade's stiffness, about 150 ms.
    public static let fade = Animation.spring(response: MotionTokens.fadeResponse, dampingFraction: 1)
    /// What a thrown thing lands on: a whisper of overshoot, the throw's.
    public static let momentum: Double = MotionTokens.momentum
    /// Let go faster than this, in points a second, a thing was thrown:
    /// the shared flick (`MotionTokens.flick`), Android's `Springs.FLICK`.
    public static let thrown: Double = MotionTokens.flick
    /// What stands in for a move under Reduce Motion: `fade`, Android's
    /// `Springs.fade()` — short, critically damped, so it never passes
    /// where it goes, and a spring, so it turns where it is if sent again.
    public static let calm = fade
    /// The most a settle may take, in seconds (`MotionTokens.capMs`). SwiftUI
    /// springs are not cut short; it is here so a caller that waits on a
    /// settle waits no longer than Android's `Capped` would.
    public static let cap: Double = Double(MotionTokens.capMs) / 1000
    /// A back gesture's preview: at most this share of what it previews…
    public static let preview: Double = MotionTokens.preview
    /// …and this much of the gesture brings it 63% of the way.
    public static let give: Double = MotionTokens.give

    /// What a thing a finger let go of springs on from `from` to `to`, the
    /// finger going at `velocity` (points a second, along the same axis):
    /// it starts at that speed, so nothing seams where the drag ends; and
    /// thrown (`thrown`), it lands on the momentum's damping, otherwise
    /// critically damped as any move.
    public static func released(velocity: Double, from: Double, to: Double) -> Animation {
        let damping = abs(velocity) > thrown ? momentum : 1
        return .interpolatingSpring(Spring(response: response, dampingRatio: damping),
                                    initialVelocity: Momentum.relative(velocity, from: from, to: to))
    }
}

extension Animation {
    /// This one, or under Reduce Motion (`\.reducesMotion`, read by the
    /// view that animates) the short fade that stands in for it (`Springs.calm`).
    public func reduced(_ still: Bool) -> Animation { still ? Springs.calm : self }
}

private struct SquaresKey: EnvironmentKey {
    static let defaultValue = Squares.neutral
}

extension EnvironmentValues {
    /// The derived square's palette: ark-ui's neutral one until an app sets its own.
    public var squares: Squares {
        get { self[SquaresKey.self] }
        set { self[SquaresKey.self] = newValue }
    }
}
