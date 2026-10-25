// harken's look on the phone, and how everything moves: the desktop's
// palette (`harken/iced/src/palette.rs`) — the system's neutral greys, which
// on iOS are the system's own colours, and the gold, one for a dark page
// and one for a white one — the derived square a cover is until it
// arrives, and the springs, which are Android's `Springs.kt` said in SwiftUI's terms
// (PARITY.md, "Motion"). Liquid Glass brings its own materials: what is here
// is only what the glass is tinted with.
import SwiftUI
import UIKit

extension Color {
    /// One colour for light and another for dark, as the trait collection says.
    init(light: UInt32, dark: UInt32) {
        self.init(uiColor: UIColor { t in UIColor(rgb: t.userInterfaceStyle == .dark ? dark : light) })
    }

    /// The gold: `controlAccentColor`'s slot. A dark page takes a bright leaf
    /// gold; a white one cannot, because text drawn on it has to be legible.
    static let gold = Color(light: 0x9A741A, dark: 0xE9BB45)
}

extension UIColor {
    convenience init(rgb: UInt32, alpha: CGFloat = 1) {
        self.init(red: CGFloat((rgb >> 16) & 0xff) / 255, green: CGFloat((rgb >> 8) & 0xff) / 255, blue: CGFloat(rgb & 0xff) / 255, alpha: alpha)
    }
}

/// The spacings, so every screen has the same rhythm: Android's `Space`.
enum Space {
    static let xs: CGFloat = 4
    static let s: CGFloat = 8
    static let m: CGFloat = 12
    static let l: CGFloat = 16
    static let xl: CGFloat = 24
    static let xxl: CGFloat = 32
    /// Where a page's content starts from its edges.
    static let page: CGFloat = 16
    /// A cover on a shelf.
    static let shelf: CGFloat = 148
    /// A cover beside a row.
    static let thumb: CGFloat = 48
    /// A page's header cover.
    static let header: CGFloat = 200
    /// A record's cover beside its title, as a catalogue's page opens.
    static let record: CGFloat = 132
    /// Corners: a cover, a card.
    static let corner: CGFloat = 8
}

/// The numbers everything moves by, Android's (`ui/Springs.kt`): one
/// stiffness, 700 at unit mass, a damping of 0.88 for where a thing is — a
/// whisper of overshoot, so it lands rather than stops — and 1 for how large
/// it is and what of it shows. SwiftUI's `spring(response:dampingFraction:)`
/// takes the same spring as its period, `2π/√(k/m)`: 0.2375 s.
enum Springs {
    static let stiffness: Double = 700
    static let response: Double = 2 * Double.pi / stiffness.squareRoot()
    /// Where a thing is.
    static let move = Animation.spring(response: response, dampingFraction: 0.88)
    /// How large a thing is: a scale, a clip, a dim. No overshoot.
    static let size = Animation.spring(response: response, dampingFraction: 1)
    /// Something going, quickly: critically damped at 2500, about 150 ms.
    static let fade = Animation.spring(response: 2 * Double.pi / 2500.0.squareRoot(), dampingFraction: 1)

    /// A sideways drag on the mini player let go past this share of its width, or…
    static let wide: CGFloat = 0.25
    /// …faster than this, in points a second, is a skip.
    static let flick: CGFloat = 600
}

/// The derived square's gradients — the desktop's `DARK_ART` and `LIGHT_ART`,
/// their first stops — picked by `Art.hash`, so the phone, the desktop and
/// the Android app draw one album as one square.
enum Squares {
    static let dark: [UInt32] = [0x7A5C15, 0x8A6B22, 0x6B5A2A, 0x8F7327, 0x5E4A18, 0xA08137]
    static let light: [UInt32] = [0xE8D19A, 0xEFDCAE, 0xE2D2AC, 0xF0DFA8, 0xE6D0A0, 0xF3E6BE]
}

/// The derived square, filling its box: a gradient in the name's tint, from
/// full strength to a third, and a note knocked back on it. Right on its own;
/// a cover that does arrive simply appears over it.
struct Square: View {
    let seed: String
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let set = scheme == .dark ? Squares.dark : Squares.light
        let tint = Color(uiColor: UIColor(rgb: set[Art.pick(set.count, seed)]))
        Canvas { ctx, size in
            let r = CGRect(origin: .zero, size: size)
            ctx.fill(Path(r), with: .linearGradient(Gradient(colors: [tint, tint.opacity(0.33)]), startPoint: .zero, endPoint: CGPoint(x: size.width, y: size.height)))
            // The desktop's note, in its 100-unit box: a head, a stem and a flag.
            let u = size.width / 100
            let ink = GraphicsContext.Shading.color(tint.opacity(0.35))
            ctx.fill(Path(ellipseIn: CGRect(x: 36 * u, y: 54 * u, width: 16 * u, height: 16 * u)), with: ink)
            ctx.fill(Path(CGRect(x: 50 * u, y: 26 * u, width: 4 * u, height: 36 * u)), with: ink)
            var flag = Path()
            flag.move(to: CGPoint(x: 50 * u, y: 26 * u))
            flag.addLine(to: CGPoint(x: 68 * u, y: 32 * u))
            flag.addLine(to: CGPoint(x: 68 * u, y: 40 * u))
            flag.addLine(to: CGPoint(x: 50 * u, y: 34 * u))
            flag.closeSubpath()
            ctx.fill(flag, with: ink)
        }
        .accessibilityHidden(true)
    }
}

/// The SF Symbol for a device's kind, as the picker and the device chip draw it.
func symbol(forDevice kind: String, name: String) -> String {
    switch kind {
    case "phone": return "iphone"
    case "speaker": return name.lowercased().contains("tv") ? "tv" : "hifispeaker"
    case "browser": return "globe"
    case "car": return "car"
    default: return "laptopcomputer"
    }
}
