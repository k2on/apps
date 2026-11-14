// What the generator's primitives (ui/docs/primitives.md) are written with:
// a text style by name, the theme's surfaces and corners, a tint as a
// figure's colour, and the leaves a stack holds — a glyph's tile, a status
// badge, a meter — called with what a recorded view hands them. The stacks,
// spacers, scroll views and text themselves are SwiftUI's own.
import SwiftUI

/// The face words are set in: SwiftUI's text styles, so Dynamic Type grows
/// them (`ark_app::view::TextStyle`; Compose's `ArkType`).
public enum ArkType: Sendable {
    case largeTitle, title, headline, body, subheadline, footnote, caption, figure

    public var font: Font {
        switch self {
        case .largeTitle: return .largeTitle.bold()
        case .title: return .title2.bold()
        case .headline: return .headline
        case .body: return .body
        case .subheadline: return .subheadline
        case .footnote: return .footnote
        case .caption: return .caption
        // A card's figure: large, bold and rounded, its digits in columns.
        case .figure: return .system(.largeTitle, design: .rounded, weight: .bold).monospacedDigit()
        }
    }
}

extension View {
    /// Set in `type`.
    public func arkType(_ type: ArkType) -> some View { font(type.font) }
}

extension Tint {
    /// A figure's colour in this tint: the words' colour while neutral.
    public var figure: Color { self == .neutral ? .ink : color }
}

/// What is behind a primitive (`Surface`): the theme's containers.
public enum ArkSurface {
    public static var card: Color { .containerLow }
    public static var raised: Color { .containerHigh }
}

/// A primitive's corners (`Corner`): `card-corner` and `corner`.
public enum ArkCorner {
    public static let card = CGFloat(LayoutTokens.cardCorner)
    public static let small = CGFloat(LayoutTokens.corner)
}

extension Tile {
    /// A glyph by its Lucide name on a tile of the tint a view names.
    public init(_ glyph: String, tint: String) {
        self.init(symbol: ArkGlyphs.sf(glyph), color: Tint(named: tint).color)
    }
}

extension StatusBadge {
    /// A figure in a badge of the tint a view names.
    public init(_ figure: String, tint: String) {
        self.init(figure, tint: Tint(named: tint))
    }
}

/// How far, 0 to 1, as the system's bar in a tint a view names (neutral is
/// the positive's), the width it is given.
public struct MeterBar: View {
    let fraction: Double
    let tint: Tint

    public init(_ fraction: Double, tint: String) {
        self.fraction = fraction
        self.tint = Tint(named: tint)
    }

    public var body: some View {
        ProgressView(value: min(max(fraction, 0), 1))
            .progressViewStyle(.linear)
            .tint(tint == .neutral ? Tint.positive.color : tint.color)
    }
}
