// What the generator's primitives (ui/docs/primitives.md) are written with:
// a text style by name, the theme's surfaces, corners and measures, a tint
// as a figure's colour, a filled picture fetched and kept (`ArkPicture`),
// and the leaves a stack holds — a glyph's tile, a status badge, a meter —
// called with what a recorded view hands them. The stacks, spacers, scroll
// views, grids, text, the derived square (`Square`) and the frames
// themselves are SwiftUI's own.
import SwiftUI

#if canImport(ImageIO)
import ImageIO
#endif

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

/// A primitive's fixed measures (`Measure`): `[layout]`'s `art-*`.
public enum ArkMeasure {
    public static let artRow = CGFloat(LayoutTokens.artRow)
    public static let artShelf = CGFloat(LayoutTokens.artShelf)
    public static let artHeader = CGFloat(LayoutTokens.artHeader)
    public static let actionsWidth = CGFloat(LayoutTokens.actionsWidth)
}

/// A picture by its URL that is exactly the frame it is given (`image(..)
/// .fill()`): the picture covers the frame and is cut to it, so it never
/// sizes anything by its own aspect — a frame is decided first (a square,
/// a side) and the picture laid into it. Nothing is drawn until it has
/// come, so what is under it shows (a cover's derived square); a picture
/// that never comes is nothing. Decoded for `decode` points across (the
/// most a filling picture is drawn at when none), once per size, and kept;
/// a screenshot waits for it to come or fail.
public struct ArkPicture: View {
    let url: String
    let label: String
    let decode: CGFloat?
    @Environment(\.displayScale) private var scale

    public init(_ url: String, label: String, decode: CGFloat?) {
        self.url = url
        self.label = label
        self.decode = decode
    }

    public var body: some View {
        Color.clear
            .overlay {
                if !url.isEmpty { Loaded(url: url, pixels: pixels) }
            }
            .clipped()
            .accessibilityElement()
            .accessibilityLabel(label)
            .accessibilityAddTraits(.isImage)
            .accessibilityHidden(label.isEmpty)
    }

    /// The pixels it is decoded for: its side at the display's scale.
    private var pixels: Int {
        Int(((decode ?? PictureCache.fill) * scale).rounded())
    }

    /// The picture, once it has come, covering the frame.
    private struct Loaded: View {
        let url: String
        let pixels: Int
        @State private var image: Image? = nil
        @State private var done = false

        var body: some View {
            let kept = PictureCache.shared.cached(url, pixels)
            ZStack { (image ?? kept)?.resizable().scaledToFill() }
                // Not ready for a screenshot until it has come, or failed (ui/docs/shots.md).
                .readyWhile(kept == nil && !done, "picture \(url)")
                .task(id: "\(pixels) \(url)") {
                    image = await PictureCache.shared.load(url, pixels)
                    done = true
                }
        }
    }
}

/// Pictures decoded once for each size they are drawn at, and kept: a
/// picture drawn at one size is always the same one, never whichever size
/// of it came first. The decode is ImageIO's thumbnail at the size drawn, so
/// a 3000-pixel scan is never decoded whole. One that cannot be had is
/// remembered for the launch (what is under it is already right). The
/// encoded originals are URLSession's cache's.
@MainActor
final class PictureCache {
    static let shared = PictureCache()
    /// The side a filling picture with no measure (a grid's, the player's)
    /// is decoded for.
    static let fill: CGFloat = 384

    private var kept: [String: Image] = [:]
    private var missing: Set<String> = []
    private var loading: [String: Task<Image?, Never>] = [:]

    func cached(_ url: String, _ pixels: Int) -> Image? { kept["\(pixels) \(url)"] }

    func load(_ url: String, _ pixels: Int) async -> Image? {
        let key = "\(pixels) \(url)"
        if let k = kept[key] { return k }
        if missing.contains(url) { return nil }
        if let t = loading[key] { return await t.value }
        let t = Task<Image?, Never> {
            guard let u = URL(string: url), let (data, _) = try? await URLSession.shared.data(from: u) else { return nil }
            return await Task.detached { Self.decode(data, pixels) }.value
        }
        loading[key] = t
        let image = await t.value
        loading[key] = nil
        if let i = image { kept[key] = i } else { missing.insert(url) }
        return image
    }

    nonisolated private static func decode(_ data: Data, _ pixels: Int) -> Image? {
        #if canImport(ImageIO)
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: max(pixels, 64),
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        return Image(decorative: cg, scale: 1)
        #else
        return nil
        #endif
    }
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
