// What a surface is drawn on, as the person's settings ask: glass, or
// under Reduce Transparency the theme's solid container, so nothing behind
// it shows through; a scroll view's edge soft where content meets the bars,
// or hard (a solid edge and its line) under it; and under Increase Contrast
// a defined edge round the glass and stronger hairlines. Both settings are
// the system's, followed as they are. And a size of type that grows with
// the person's text size (`scaledFont`), for a figure or a mark larger than
// any text style.
import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

#if canImport(UIKit)
extension ShapeStyle where Self == Color {
    /// A hairline that holds its own: the theme's (`hairline`), or under
    /// Increase Contrast the stronger edge of a control (`outline`).
    public static var edge: Color {
        Color(uiColor: UIColor { t in
            UIColor(rgb: (t.accessibilityContrast == .high ? Tokens.outline : Tokens.hairline).of(dark: t.userInterfaceStyle == .dark))
        })
    }

    /// The track a value fills — a slider's, a progress bar's: the quiet
    /// colour, faint, or twice as strong under Increase Contrast.
    public static var track: Color {
        Color(uiColor: UIColor { t in
            UIColor(rgb: Tokens.quiet.of(dark: t.userInterfaceStyle == .dark), alpha: t.accessibilityContrast == .high ? 0.5 : 0.25)
        })
    }
}
#else
extension ShapeStyle where Self == Color {
    /// A hairline (the Mac's: its own Increase Contrast draws the edges).
    public static var edge: Color { Color(Tokens.hairline) }

    /// The track a value fills: the quiet colour, faint.
    public static var track: Color { Color(Tokens.quiet).opacity(0.25) }
}
#endif

#if canImport(SwiftUI)
@available(iOS 26.0, *)
extension View {
    /// On glass in `shape`, tinted with `tint` and answering the finger when
    /// `interactive`; under Reduce Transparency on the theme's container
    /// instead, in the same shape, the tint laid over it as it is given (a
    /// faint wash given faint); and under
    /// Increase Contrast with the edge of a control round it.
    public func surface<S: Shape>(in shape: S, tint: Color? = nil, interactive: Bool = false) -> some View {
        modifier(Surface(shape: shape, tint: tint, interactive: interactive))
    }

    /// A scroll view's edges where the bars float over it: soft, or under
    /// Reduce Transparency hard, so no content shows through the bars.
    public func scrollEdges() -> some View {
        modifier(ScrollEdges())
    }
}

@available(iOS 26.0, *)
private struct Surface<S: Shape>: ViewModifier {
    let shape: S
    let tint: Color?
    let interactive: Bool
    @Environment(\.accessibilityReduceTransparency) private var solid
    @Environment(\.colorSchemeContrast) private var contrast

    func body(content: Content) -> some View {
        Group {
            if solid {
                content.background {
                    shape.fill(Color.containerHigh)
                        .overlay { if let tint { shape.fill(tint) } }
                }
            } else {
                content.glassEffect(glass, in: shape)
            }
        }
        .overlay { if contrast == .increased { shape.stroke(Color.outline, lineWidth: 1) } }
    }

    private var glass: Glass {
        var g = Glass.regular
        if let tint { g = g.tint(tint) }
        if interactive { g = g.interactive() }
        return g
    }
}

@available(iOS 26.0, *)
private struct ScrollEdges: ViewModifier {
    @Environment(\.accessibilityReduceTransparency) private var solid

    func body(content: Content) -> some View {
        content.scrollEdgeEffectStyle(solid ? .hard : .soft, for: .all)
    }
}
#endif

extension View {
    /// The system font at `size` points at the default text size, growing
    /// and shrinking with the person's text size as `relativeTo` does: for a
    /// figure or a mark larger than any text style.
    public func scaledFont(_ size: CGFloat, weight: Font.Weight = .regular, design: Font.Design = .default,
                           relativeTo style: Font.TextStyle = .largeTitle) -> some View {
        modifier(ScaledFont(size: size, weight: weight, design: design, style: style))
    }
}

private struct ScaledFont: ViewModifier {
    @ScaledMetric private var size: CGFloat
    let weight: Font.Weight
    let design: Font.Design

    init(size: CGFloat, weight: Font.Weight, design: Font.Design, style: Font.TextStyle) {
        _size = ScaledMetric(wrappedValue: size, relativeTo: style)
        self.weight = weight
        self.design = design
    }

    func body(content: Content) -> some View {
        content.font(.system(size: size, weight: weight, design: design))
    }
}
