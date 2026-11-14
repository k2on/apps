// A photo by its URL and a slip of paper (ark_app's `photo`, `zoom_photo`
// and `paper`, ui/docs/parts.md): what a receipt is shown as, its photo
// whole and the paper it was printed on.
#if canImport(SwiftUI)
import ArkUICore
import SwiftUI

/// A photo by its URL — `file:` (a file this device holds), `data:` (its
/// bytes) or `https:` — fitted to its width, `label` said for it; a
/// spinner while the URL is empty (still being fetched). Zoomed (`zoom`),
/// a pinch or a double tap makes it larger, a drag pans it while it is,
/// and past its least and most size it rubber-bands (`Momentum.band`) and
/// settles back on `Springs.move` when let go.
public struct Photo: View {
    let url: String
    let label: String
    let zoom: Bool

    public init(_ url: String, label: String, zoom: Bool) {
        self.url = url
        self.label = label
        self.zoom = zoom
    }

    public var body: some View {
        Group {
            if let image = Photos.image(url) {
                if zoom { Zoomed(image: image) } else { image.resizable().scaledToFit() }
            } else if let web = URL(string: url), web.scheme == "https" {
                AsyncImage(url: web) { phase in
                    if let image = phase.image { image.resizable().scaledToFit() } else { waiting }
                }
            } else {
                waiting
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: Space.corner))
        .listRowInsets(EdgeInsets(top: Space.m, leading: 0, bottom: Space.m, trailing: 0))
        .listRowBackground(Color.clear)
        .accessibilityElement()
        .accessibilityLabel(label)
        .accessibilityAddTraits(.isImage)
    }

    private var waiting: some View {
        ProgressView().frame(maxWidth: .infinity, minHeight: Space.row * 2)
    }
}

/// The photos a `file:` or `data:` URL names, decoded once and kept a while.
@MainActor
enum Photos {
    private static var kept: [String: Image] = [:]

    static func image(_ url: String) -> Image? {
        if let i = kept[url] { return i }
        guard let u = URL(string: url), u.scheme == "file" || u.scheme == "data",
              let data = try? Data(contentsOf: u), let image = picture(data) else { return nil }
        if kept.count >= 16 { kept.removeAll() }
        kept[url] = image
        return image
    }
}

/// A photo that pinches, double-taps and pans.
private struct Zoomed: View {
    let image: Image
    @Environment(\.reducesMotion) private var still
    /// Where it rests: its scale, and where it is panned to.
    @State private var scale: CGFloat = 1
    @State private var offset: CGSize = .zero
    /// What the fingers are doing now.
    @GestureState private var pinch: CGFloat = 1
    @GestureState private var drag: CGSize = .zero

    private static let most = CGFloat(MotionTokens.zoomMax)

    /// `s` as drawn: as it is between its least and most, rubber-banded past either.
    private static func banded(_ s: CGFloat) -> CGFloat {
        if s < 1 { return 1 - CGFloat(Momentum.band(Double(1 - s), limit: 0.5)) }
        if s > most { return most + CGFloat(Momentum.band(Double(s - most), limit: Double(most) / 2)) }
        return s
    }

    /// The furthest it can be panned at `s` in a frame of `size` and stay over it.
    private static func clamped(_ o: CGSize, _ s: CGFloat, _ size: CGSize) -> CGSize {
        let x = max(0, (size.width * s - size.width) / 2), y = max(0, (size.height * s - size.height) / 2)
        return CGSize(width: min(max(o.width, -x), x), height: min(max(o.height, -y), y))
    }

    var body: some View {
        GeometryReader { g in
            let shown = Zoomed.banded(scale * pinch)
            image.resizable().scaledToFit()
                .frame(width: g.size.width, height: g.size.height)
                .scaleEffect(shown)
                .offset(x: offset.width + drag.width, y: offset.height + drag.height)
                .contentShape(Rectangle())
                .gesture(
                    MagnifyGesture()
                        .updating($pinch) { v, s, _ in s = v.magnification }
                        .onEnded { v in
                            let s = min(max(scale * v.magnification, 1), Zoomed.most)
                            withAnimation(Springs.move.reduced(still)) {
                                scale = s
                                offset = Zoomed.clamped(offset, s, g.size)
                            }
                        }
                )
                .simultaneousGesture(
                    DragGesture(minimumDistance: scale > 1 ? 0 : 100_000)
                        .updating($drag) { v, d, _ in d = v.translation }
                        .onEnded { v in
                            let moved = CGSize(width: offset.width + v.translation.width, height: offset.height + v.translation.height)
                            let rest = Zoomed.clamped(moved, scale, g.size)
                            offset = moved
                            withAnimation(Springs.released(velocity: v.velocity.width, from: moved.width, to: rest.width).reduced(still)) {
                                offset = rest
                            }
                        }
                )
                .onTapGesture(count: 2) {
                    withAnimation(Springs.move.reduced(still)) {
                        scale = scale > 1 ? 1 : CGFloat(MotionTokens.zoomTap)
                        offset = .zero
                    }
                }
        }
        .aspectRatio(contentMode: .fit)
        .frame(minHeight: Space.row * 6)
        .clipped()
    }
}

/// A slip of paper (a receipt as it was printed): monospace, dark on white
/// in light and dark alike; its heading bold and its date centred, a rule,
/// each line's name with its figure against the right and its note under
/// it, a rule, and the total.
public struct Paper: View {
    let heading: String
    let date: String
    let names: [String]
    let figures: [String]
    let notes: [String]
    let total: String

    public init(_ heading: String, date: String, names: [String], figures: [String], notes: [String], total: String) {
        self.heading = heading
        self.date = date
        self.names = names
        self.figures = figures
        self.notes = notes
        self.total = total
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: Space.xs) {
            Text(heading).fontWeight(.bold).frame(maxWidth: .infinity)
            Text(date).frame(maxWidth: .infinity)
            rule
            ForEach(names.indices, id: \.self) { i in
                HStack(alignment: .firstTextBaseline) {
                    Text(names[i]).lineLimit(2)
                    Spacer(minLength: Space.m)
                    Text(i < figures.count ? figures[i] : "")
                }
                if i < notes.count, !notes[i].isEmpty {
                    Text("  \(notes[i])").foregroundStyle(Color.black.opacity(0.55))
                }
            }
            rule
            HStack {
                Text("TOTAL").fontWeight(.bold)
                Spacer()
                Text(total).fontWeight(.bold)
            }
        }
        .font(.system(.footnote, design: .monospaced))
        .foregroundStyle(Color.black)
        .padding(Space.l)
        .background(Color.white, in: RoundedRectangle(cornerRadius: Space.corner))
        .shadow(color: .black.opacity(0.18), radius: 6, y: 2)
        .environment(\.colorScheme, .light)
        .listRowInsets(EdgeInsets(top: Space.m, leading: 0, bottom: Space.m, trailing: 0))
        .listRowBackground(Color.clear)
        .accessibilityElement(children: .combine)
    }

    private var rule: some View {
        Text(String(repeating: "-", count: 40)).lineLimit(1).truncationMode(.tail).foregroundStyle(Color.black.opacity(0.5))
    }
}
#endif
