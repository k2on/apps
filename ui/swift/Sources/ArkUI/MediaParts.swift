// The parts a screen of covers and sounds is laid out with (ui/docs/primitives.md,
// "What Harken Lays Out"), as `ark_app::view` names them: something pressed as
// one (`Pressable`), a cover over its derived square (`Artwork`), tiles in as
// many columns as fit (`ArtworkGrid`), and where a sound is, counted from its
// clock as it is drawn (`ClockScrubber`, `ClockHairline`). Their twins are
// Compose's MediaParts.kt, iced's `arkui::view` and arkui-web's parts.
import SwiftUI

#if canImport(ImageIO)
import ImageIO
#endif

/// How a pressed thing answers (ui/docs/motion.md, Press Feedback): a cover,
/// a tile or a glyph gives (`.press`), a row the page's width lights
/// (`.pressRow`).
public enum PressKind {
    case give
    case highlight
}

/// What is laid out in `content`, pressed as one: its press on touch-down,
/// said as `label`, its menu on a long press.
public struct Pressable<Content: View>: View {
    let label: String
    let press: PressKind
    let menu: [MenuAction]
    let onTap: () -> Void
    let content: Content

    public init(_ label: String, press: PressKind, menu: [MenuAction] = [], onTap: @escaping () -> Void, @ViewBuilder content: () -> Content) {
        self.label = label
        self.press = press
        self.menu = menu
        self.onTap = onTap
        self.content = content()
    }

    public var body: some View {
        let button = Button(action: onTap) { content.contentShape(Rectangle()) }
            .accessibilityLabel(label)
        Group {
            switch press {
            case .give: button.buttonStyle(.press)
            case .highlight: button.buttonStyle(.pressRow)
            }
        }
        .contextMenu {
            if !menu.isEmpty { MenuActions(menu: menu) }
        }
    }
}

/// How large a cover is drawn: `[layout]`'s `art-*`, or as wide as given.
public enum ArtSize {
    case row
    case shelf
    case header
    case fill

    /// Its side; none when it fills.
    public var side: CGFloat? {
        switch self {
        case .row: CGFloat(LayoutTokens.artRow)
        case .shelf: CGFloat(LayoutTokens.artShelf)
        case .header: CGFloat(LayoutTokens.artHeader)
        case .fill: nil
        }
    }
}

extension View {
    /// As wide as a cover of `size`, what it holds kept to it.
    public func artWidth(_ size: ArtSize) -> some View {
        Group {
            if let side = size.side {
                frame(width: side, alignment: .leading)
            } else {
                frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

/// A cover: the picture at `url` over the square `seed` derives (shown while
/// it comes, or when there is none), at `size`, round for a person. A
/// screenshot waits for it to come or fail.
public struct Artwork: View {
    let url: String
    let seed: String
    let label: String
    let size: ArtSize
    let round: Bool
    @Environment(\.displayScale) private var scale

    public init(_ url: String, seed: String, label: String, size: ArtSize, round: Bool) {
        self.url = url
        self.seed = seed
        self.label = label
        self.size = size
        self.round = round
    }

    public var body: some View {
        let shape = round ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: Space.corner, style: .continuous))
        let art = ZStack {
            Square(seed: seed)
            if !url.isEmpty {
                ArtworkPicture(url: url, pixels: pixels)
            }
        }
        .clipShape(shape)
        .accessibilityElement()
        .accessibilityLabel(label)
        .accessibilityAddTraits(.isImage)
        if let side = size.side {
            art.frame(width: side, height: side)
        } else {
            art.aspectRatio(1, contentMode: .fit).frame(maxWidth: .infinity)
        }
    }

    /// The pixels it is decoded for: its side at the display's scale, a
    /// filling one at the most a cover is drawn at.
    private var pixels: Int {
        Int(((size.side ?? ArtworkCache.fill) * scale).rounded())
    }
}

/// The picture of an `Artwork`, decoded for its pixels and kept.
private struct ArtworkPicture: View {
    let url: String
    let pixels: Int
    @State private var image: Image? = nil
    @State private var done = false

    var body: some View {
        let kept = ArtworkCache.shared.cached(url, pixels)
        ZStack { (image ?? kept)?.resizable().scaledToFill() }
            // Not ready for a screenshot until it has come, or failed (ui/docs/shots.md).
            .readyWhile(kept == nil && !done, "artwork \(url)")
            .task(id: "\(pixels) \(url)") {
                image = await ArtworkCache.shared.load(url, pixels)
                done = true
            }
    }
}

/// Covers decoded once for each size they are drawn at, and kept: a cover
/// drawn at one size is always the same picture, never whichever size of it
/// came first. The decode is ImageIO's thumbnail at the size drawn, so a
/// 3000-pixel scan is never decoded whole. A cover that cannot be had is
/// remembered for the launch: it is decoration, and the square is already
/// right. The encoded originals are URLSession's cache's.
@MainActor
final class ArtworkCache {
    static let shared = ArtworkCache()
    /// The side a filling cover (a grid's, the player's) is decoded for.
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

/// Tiles in as many columns as fit at least `art-shelf` wide, lazily, at the
/// page's margins.
public struct ArtworkGrid<Content: View>: View {
    let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: CGFloat(LayoutTokens.artShelf)), spacing: Space.m, alignment: .top)], spacing: Space.l) {
            content
        }
        .padding(.horizontal, Space.page)
    }
}

/// Milliseconds as `m:ss`, or `h:mm:ss` from an hour: ark_format's `clock`.
func clockWords(_ ms: Int64) -> String {
    let secs = max(ms, 0) / 1000
    let h = secs / 3600
    return h == 0
        ? String(format: "%d:%02d", secs / 60, secs % 60)
        : String(format: "%d:%02d:%02d", h, secs / 60 % 60, secs % 60)
}

/// Where a clock says a sound is at `now`: `at` read at `since` (the
/// device's epoch milliseconds), counting on while it plays.
func clockAt(_ at: Int64, since: Int64, playing: Bool, length: Int64, now: Date) -> Int64 {
    let n = Int64(now.timeIntervalSince1970 * 1000)
    return min(max(playing ? at + max(n - since, 0) : at, 0), max(length, 1))
}

/// Where a sound is, which a finger drags: ArkUI's `Scrubber` over its
/// clock, redrawn as it plays (`TimelineView`) and still while it does not;
/// let go, `onSeek` is sent once with where. Meeting either end under the
/// finger is a selection's haptic.
public struct ClockScrubber: View {
    let at: Int64
    let since: Int64
    let playing: Bool
    let length: Int64
    let onSeek: (Int64) -> Void
    @State private var end = 0

    public init(_ at: Int64, since: Int64, playing: Bool, length: Int64, onSeek: @escaping (Int64) -> Void) {
        self.at = at
        self.since = since
        self.playing = playing
        self.length = length
        self.onSeek = onSeek
    }

    public var body: some View {
        let whole = Double(max(length, 1))
        TimelineView(.periodic(from: .now, by: playing && !Shots.on ? 0.25 : 3600)) { tick in
            let now = clockAt(at, since: since, playing: playing, length: length, now: tick.date)
            Scrubber(
                value: Double(now) / whole,
                left: { clockWords(Int64($0 * whole)) },
                right: { _ in clockWords(length) },
                onValue: { v in end = v <= 0 ? -1 : v >= 1 ? 1 : 0 },
                onDone: { v in
                    end = 0
                    onSeek(Int64(v * whole))
                })
        }
        .sensoryFeedback(.selection, trigger: end) { _, new in new != 0 }
    }
}

/// How far along a sound is, a hairline in the accent, counting on as it
/// plays.
public struct ClockHairline: View {
    let at: Int64
    let since: Int64
    let playing: Bool
    let length: Int64

    public init(_ at: Int64, since: Int64, playing: Bool, length: Int64) {
        self.at = at
        self.since = since
        self.playing = playing
        self.length = length
    }

    public var body: some View {
        TimelineView(.periodic(from: .now, by: playing && !Shots.on ? 0.25 : 3600)) { tick in
            let now = clockAt(at, since: since, playing: playing, length: length, now: tick.date)
            // Drawn, not laid out: the fill scaled from the leading edge.
            ZStack(alignment: .leading) {
                Rectangle().fill(Color.quiet.opacity(0.25))
                Rectangle().fill(Color.accent)
                    .scaleEffect(x: CGFloat(now) / CGFloat(max(length, 1)), y: 1, anchor: .leading)
            }
            .frame(height: 2)
            .accessibilityElement()
            .accessibilityLabel("Position")
            .accessibilityValue(clockWords(now))
        }
    }
}

/// How loud, as `volume` lays it out: `VolumeSlider` over a level 0 to
/// 100, its loudspeaker sending `onMute`; the level follows the finger and
/// `onChange` is sent once, with where it was let go, as the scrubber's
/// seek is. A level from elsewhere is sprung to (`VolumeSlider`).
public struct VolumePart: View {
    let level: Int64
    let muted: Bool
    let about: String
    let onChange: (Int64) -> Void
    let onMute: () -> Void
    @State private var held: Int?

    public init(_ level: Int64, muted: Bool, about: String, onChange: @escaping (Int64) -> Void, onMute: @escaping () -> Void) {
        self.level = level
        self.muted = muted
        self.about = about
        self.onChange = onChange
        self.onMute = onMute
    }

    public var body: some View {
        VolumeSlider(
            level: Int(min(max(level, 0), 100)), muted: muted, about: about,
            onLevel: { held = $0 },
            onDone: {
                if let h = held { onChange(Int64(h)) }
                held = nil
            },
            onMute: onMute)
    }
}
