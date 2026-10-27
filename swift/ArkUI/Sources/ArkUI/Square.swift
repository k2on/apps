import SwiftUI
import UIKit

/// The derived square, filling its box: a gradient in the name's colour, from
/// full strength to a third, and a note knocked back on it. Right on its own;
/// a picture that does arrive simply appears over it.
public struct Square: View {
    public let seed: String
    @Environment(\.colorScheme) private var scheme
    @Environment(\.squares) private var squares

    public init(seed: String) {
        self.seed = seed
    }

    public var body: some View {
        let set = squares.of(dark: scheme == .dark)
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
