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

/// The numbers everything moves by, Android's (`ui/Springs.kt`): one
/// stiffness, 700 at unit mass, a damping of 0.88 for where a thing is — a
/// whisper of overshoot, so it lands rather than stops — and 1 for how large
/// it is and what of it shows. SwiftUI's `spring(response:dampingFraction:)`
/// takes the same spring as its period, `2π/√(k/m)`: 0.2375 s.
public enum Springs {
    public static let stiffness: Double = 700
    public static let response: Double = 2 * Double.pi / stiffness.squareRoot()
    /// Where a thing is.
    public static let move = Animation.spring(response: response, dampingFraction: 0.88)
    /// How large a thing is: a scale, a clip, a dim. No overshoot.
    public static let size = Animation.spring(response: response, dampingFraction: 1)
    /// Something going, quickly: critically damped at 2500, about 150 ms.
    public static let fade = Animation.spring(response: 2 * Double.pi / 2500.0.squareRoot(), dampingFraction: 1)
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
