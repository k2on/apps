// The small generic pieces a page is laid out of, with the same names and
// the same inputs as Android's (`dev.arkui`, `Parts.kt`) and the desktop's
// (`arkui::parts`); ui/docs/parts.md lists them: a page's header about
// one thing, a section's heading, rows in a rounded card with hairlines
// between them, a notice across the page, one of a few to choose, a count, a
// glyph on a washed square, a bar that fills toward a goal, bars against
// the largest and a ring of parts, the field a name is typed in, a scrubber,
// a volume, and the glyphs by their shared names. `Pill` and `Pager` are in
// Figures.swift, `SearchField` in Search.swift. Nothing here names a thing
// any app is about; every colour is the theme's.
#if canImport(SwiftUI)
import SwiftUI
#if canImport(Charts)
import Charts
#endif

/// A card's corners, as an inset grouped list rounds its sections.
public let CardCorner = CGFloat(LayoutTokens.cardCorner)

@available(iOS 26.0, *)
/// The header of a page that is about one thing: its `cover`, what kind of
/// thing it is, its name, who it is by (a link in the accent when `onBy`
/// goes somewhere), the facts true of the whole of it, and the page's own
/// `actions` under them all.
public struct PageHeader<Cover: View, Actions: View>: View {
    let kind: String
    let title: String
    let facts: String
    let by: String?
    let onBy: (() -> Void)?
    let cover: Cover
    let actions: Actions

    public init(kind: String, title: String, facts: String, by: String? = nil, onBy: (() -> Void)? = nil,
                @ViewBuilder cover: () -> Cover, @ViewBuilder actions: () -> Actions) {
        self.kind = kind
        self.title = title
        self.facts = facts
        self.by = by
        self.onBy = onBy
        self.cover = cover()
        self.actions = actions()
    }

    public var body: some View {
        VStack(spacing: Space.xs) {
            cover.padding(.top, Space.s).padding(.bottom, Space.l)
            Text(kind).font(.caption.weight(.semibold)).foregroundStyle(.quiet)
            Text(title).font(.title2.bold()).multilineTextAlignment(.center)
            if let by {
                Button(by) { onBy?() }.buttonStyle(.borderless).font(.headline).foregroundStyle(Color.accentText).disabled(onBy == nil)
            }
            Text(facts).font(.footnote).foregroundStyle(.quiet)
            actions.padding(.top, Space.l)
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, Space.page)
        .padding(.bottom, Space.m)
    }
}

/// A section's heading over its card, as a grouped list sets one: its name,
/// quiet, heard as a heading, and what it adds up to — or an action — at
/// its end.
public struct SectionHeader<Trailing: View>: View {
    let title: String
    let trailing: Trailing

    public init(_ title: String, @ViewBuilder trailing: () -> Trailing) {
        self.title = title
        self.trailing = trailing()
    }

    public var body: some View {
        HStack {
            Text(title).font(.headline).foregroundStyle(.quiet).lineLimit(1)
                .accessibilityAddTraits(.isHeader)
            Spacer(minLength: Space.s)
            trailing
        }
        .padding(.horizontal, Space.l)
        .padding(.top, Space.xl)
        .padding(.bottom, Space.s)
    }
}

extension SectionHeader where Trailing == EmptyView {
    /// The heading alone.
    public init(_ title: String) {
        self.init(title) { EmptyView() }
    }
}

/// Rows in a rounded card, as an inset grouped list draws a section; a
/// `RowDivider` between each, which the caller puts there.
public struct GroupCard<Content: View>: View {
    let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        VStack(spacing: 0) { content }
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.containerLow, in: RoundedRectangle(cornerRadius: CardCorner, style: .continuous))
    }
}

/// The hairline between two rows of a `GroupCard`, from where the text starts.
public struct RowDivider: View {
    let inset: CGFloat

    public init(inset: CGFloat = Space.l) {
        self.inset = inset
    }

    public var body: some View {
        Rectangle().fill(Color.hairline).frame(height: 0.5).padding(.leading, inset)
    }
}

@available(iOS 26.0, *)
/// A notice across the page — the server cannot be reached, say: a quiet
/// glyph (an SF Symbol) beside a sentence, on glass. It says; it does not ask.
public struct Banner: View {
    let symbol: String
    let text: String

    public init(symbol: String, text: String) {
        self.symbol = symbol
        self.text = text
    }

    public var body: some View {
        HStack(spacing: Space.m) {
            Image(systemName: symbol).foregroundStyle(.quiet)
            Text(text).font(.footnote).foregroundStyle(.quiet)
        }
        .padding(Space.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .surface(in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .padding(.horizontal, Space.page)
        .padding(.vertical, Space.s)
    }
}

/// One of a few to choose from, in a capsule: the chosen one filled in the
/// accent, the rest on the container's fill; it gives under the finger.
public struct Chip: View {
    let label: String
    let selected: Bool
    let action: () -> Void

    public init(_ label: String, selected: Bool, action: @escaping () -> Void) {
        self.label = label
        self.selected = selected
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            Text(label)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(selected ? Color.onAccent : Color.ink)
                .padding(.horizontal, Space.l)
                .frame(minHeight: 36)
                .background(Capsule().fill(selected ? Color.accent : Color.containerHigh))
                .contentShape(Capsule())
        }
        .buttonStyle(.press)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// A count in the accent's capsule, as a row's "3 to review" is; VoiceOver
/// hears `said`.
public struct Badge: View {
    let count: Int
    let said: String

    public init(_ count: Int, said: String) {
        self.count = count
        self.said = said
    }

    public var body: some View {
        Text("\(count)")
            .font(.caption.weight(.bold))
            .monospacedDigit()
            .foregroundStyle(Color.onAccent)
            .padding(.horizontal, 7)
            .padding(.vertical, 2)
            .background(Color.accent, in: Capsule())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(said)
    }
}

/// A glyph (an SF Symbol) in `color` on a rounded square washed in it,
/// `side` across. It grows with the person's text size, as the words beside
/// it do, up to half as large again.
public struct Tile: View {
    let symbol: String
    let color: Color
    let side: CGFloat
    @ScaledMetric(relativeTo: .body) private var grows: CGFloat = 1

    public init(symbol: String, color: Color, side: CGFloat = 32) {
        self.symbol = symbol
        self.color = color
        self.side = side
    }

    public var body: some View {
        let side = self.side * min(grows, 1.5)
        Image(systemName: symbol)
            .font(.system(size: side * 0.5, weight: .semibold))
            .foregroundStyle(color)
            .frame(width: side, height: side)
            .background(color.opacity(0.16), in: RoundedRectangle(cornerRadius: side * 0.26, style: .continuous))
            .accessibilityHidden(true)
    }
}

/// A bar filling toward a goal: `fraction` of the way (held to the bar), in
/// `color` over `track`, `height` thick. It springs to a new value rather
/// than jumping; VoiceOver hears it as a share.
public struct ProgressBar: View {
    let fraction: Double
    let color: Color
    let track: Color
    let height: CGFloat
    @Environment(\.reducesMotion) private var still

    public init(_ fraction: Double, color: Color = .accent, track: Color = .containerHighest, height: CGFloat = 6) {
        self.fraction = fraction
        self.color = color
        self.track = track
        self.height = height
    }

    public var body: some View {
        let f = min(max(fraction, 0), 1)
        Capsule().fill(track)
            .frame(height: height)
            .overlay(alignment: .leading) {
                GeometryReader { g in
                    Capsule().fill(color)
                        .frame(width: f > 0 ? max(g.size.width * f, height) : 0)
                }
            }
            .animation(Springs.size.reduced(still), value: f)
            .accessibilityElement()
            .accessibilityValue(Text(f, format: .percent.precision(.fractionLength(0))))
    }
}

/// One part of a chart: what it is, how much, and its colour.
public struct Part: Identifiable, Equatable {
    public let label: String
    public let value: Int64
    public let color: Color
    public var id: String { label }

    public init(label: String, value: Int64, color: Color) {
        self.label = label
        self.value = value
        self.color = color
    }
}

/// `parts` as bars, each against the largest, with its label and its value
/// (`value` writes it) on a line above: a legend and a comparison at once.
/// One thing to VoiceOver, the sentence `description` says.
public struct Bars: View {
    let parts: [Part]
    let description: String
    let value: (Int64) -> String
    let height: CGFloat

    public init(_ parts: [Part], description: String, height: CGFloat = 10, value: @escaping (Int64) -> String) {
        self.parts = parts
        self.description = description
        self.height = height
        self.value = value
    }

    public var body: some View {
        let most = Double(max(parts.map(\.value).max() ?? 1, 1))
        VStack(spacing: Space.m) {
            ForEach(parts) { p in
                VStack(spacing: Space.xs) {
                    HStack(spacing: Space.s) {
                        Circle().fill(p.color).frame(width: 10, height: 10)
                        Text(p.label).font(.subheadline).lineLimit(1)
                        Spacer(minLength: Space.s)
                        Text(value(p.value)).font(.subheadline.weight(.semibold)).monospacedDigit().foregroundStyle(.quiet)
                    }
                    ProgressBar(Double(p.value) / most, color: p.color, track: .clear, height: height)
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(description)
    }
}

#if canImport(Charts)
/// A ring of `parts` with a small gap between each, `thickness` of its
/// radius, and `center` in the middle — a total, say. Parts of nothing are
/// left out. One thing to VoiceOver, the sentence `description` says.
public struct Donut<Center: View>: View {
    let parts: [Part]
    let description: String
    let thickness: Double
    let center: Center

    public init(_ parts: [Part], description: String, thickness: Double = 0.38, @ViewBuilder center: () -> Center) {
        self.parts = parts
        self.description = description
        self.thickness = thickness
        self.center = center()
    }

    public var body: some View {
        Chart(parts.filter { $0.value > 0 }) { p in
            SectorMark(angle: .value(p.label, Double(p.value)), innerRadius: .ratio(1 - thickness), angularInset: 1.5)
                .cornerRadius(5)
                .foregroundStyle(p.color)
        }
        .chartBackground { _ in center }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(description)
    }
}
#endif

/// The field a new thing's name is typed in, as a section of a form:
/// `label` as its prompt, focused as it appears, Done saying `onSubmit`,
/// and under it the reason the name will not do (`problem`, once something
/// is typed) or else `hint`.
public struct NameField: View {
    @Binding var name: String
    let problem: String?
    let hint: String
    let label: String
    let onSubmit: () -> Void
    @FocusState private var focused: Bool

    public init(name: Binding<String>, problem: String? = nil, hint: String = "", label: String = "Name", onSubmit: @escaping () -> Void = {}) {
        self._name = name
        self.problem = problem
        self.hint = hint
        self.label = label
        self.onSubmit = onSubmit
    }

    public var body: some View {
        Section {
            TextField(label, text: $name).focused($focused).submitLabel(.done).onSubmit(onSubmit)
        } footer: {
            if let p = problem, !name.isEmpty {
                Text(p.prefix(1).uppercased() + p.dropFirst()).foregroundStyle(.error)
            } else if !hint.isEmpty {
                Text(hint)
            }
        }
        // Not in a screenshot (ui/docs/shots.md): a shot that types taps it.
        .onAppear { if !Shots.on { focused = true } }
    }
}

/// A position through something that runs, which a finger drags: the `Bar`
/// with no handle, and under it what has gone (`left`) and what is left
/// (`right`), written by the caller from the bar's value. Under the finger
/// it is where the finger is, saying `onValue` as it goes; let go, `onDone`
/// says where; otherwise it is at `value`.
public struct Scrubber: View {
    let value: Double
    let enabled: Bool
    let label: String
    let left: (Double) -> String
    let right: (Double) -> String
    let onValue: (Double) -> Void
    let onDone: (Double) -> Void
    @State private var dragging: Double? = nil

    public init(value: Double, enabled: Bool = true, label: String = "Position",
                left: @escaping (Double) -> String, right: @escaping (Double) -> String,
                onValue: @escaping (Double) -> Void, onDone: @escaping (Double) -> Void) {
        self.value = value
        self.enabled = enabled
        self.label = label
        self.left = left
        self.right = right
        self.onValue = onValue
        self.onDone = onDone
    }

    public var body: some View {
        let at = dragging ?? min(max(value, 0), 1)
        VStack(spacing: 2) {
            // No handle, as Android's.
            Bar(value: at, fill: Color.accent, enabled: enabled, onValue: { v in
                dragging = v
                onValue(v)
            }, onDone: {
                if let d = dragging {
                    onDone(d)
                    dragging = nil
                }
            })
            .accessibilityLabel(label)
            HStack {
                Text(left(at))
                Spacer()
                Text(right(at))
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(.quiet)
        }
    }
}

/// A volume: its mute, then its slider (`Bar`), 0 to 100. Under the finger
/// it is where the finger is; anywhere else it is where the level it is
/// handed says, and a level that arrives from elsewhere is sprung to
/// (`Springs.move`) rather than jumped to. Android's `VolumeSlider`.
public struct VolumeSlider: View {
    let level: Int
    let muted: Bool
    let about: String
    let onLevel: (Int) -> Void
    let onDone: () -> Void
    let onMute: () -> Void
    @State private var shown: Double = 0
    @State private var dragging = false
    @Environment(\.reducesMotion) private var still

    public init(level: Int, muted: Bool, about: String, onLevel: @escaping (Int) -> Void, onDone: @escaping () -> Void, onMute: @escaping () -> Void) {
        self.level = level
        self.muted = muted
        self.about = about
        self.onLevel = onLevel
        self.onDone = onDone
        self.onMute = onMute
    }

    public var body: some View {
        HStack(spacing: Space.s) {
            Button(action: onMute) { Image(systemName: muted ? "speaker.slash.fill" : "speaker.wave.2.fill").frame(width: 28) }
                .foregroundStyle(muted ? Color.quiet : Color.ink)
                .accessibilityLabel(muted ? "Unmute \(about)" : "Mute \(about)")
            Bar(value: shown, fill: muted ? Color.quiet : Color.accent, onValue: { v in
                dragging = true
                shown = v
                onLevel(Int((v * 100).rounded()))
            }, onDone: {
                dragging = false
                onDone()
            })
            .accessibilityLabel("Volume of \(about)")
        }
        .onAppear { shown = Double(level) / 100 }
        .onChange(of: level) { _, l in if !dragging { withAnimation(Springs.move.reduced(still)) { shown = Double(l) / 100 } } }
    }
}

/// The glyphs every toolkit has under the same name (ui/docs/parts.md,
/// "Glyphs"): Lucide's names, as Android's `ArkGlyphs.named` and the
/// desktop's `arkui::glyphs::named` know them, said here as the SF Symbol
/// the iPhone draws for each.
public enum ArkGlyphs {
    /// The SF Symbol for Lucide's `name`, if it is one of the shared ones.
    public static func symbol(_ name: String) -> String? {
        switch name {
        case "chevron-left": return "chevron.left"
        case "chevron-right": return "chevron.right"
        case "chevron-down": return "chevron.down"
        case "ellipsis-vertical": return "ellipsis"
        case "x": return "xmark"
        case "plus": return "plus"
        case "minus": return "minus"
        case "check": return "checkmark"
        case "circle-check": return "checkmark.circle.fill"
        case "circle": return "circle"
        case "search": return "magnifyingglass"
        case "wifi-off": return "wifi.slash"
        case "volume-2": return "speaker.wave.2.fill"
        case "volume-x": return "speaker.slash.fill"
        case "log-out": return "rectangle.portrait.and.arrow.right"
        case "layers": return "tray.full.fill"
        case "link": return "link"
        case "pencil": return "pencil"
        case "copy": return "doc.on.doc"
        case "archive": return "xmark.bin"
        case "undo-2": return "arrow.uturn.backward"
        case "badge-check": return "checkmark.seal"
        case "circle-plus": return "plus.circle.fill"
        case "panel-right": return "sidebar.trailing"
        case "wand-sparkles": return "wand.and.stars"
        case "eye": return "eye"
        case "eye-off": return "eye.slash"
        case "arrow-up": return "arrow.up"
        case "arrow-down": return "arrow.down"
        case "trash": return "trash"
        case "lock": return "lock.fill"
        case "receipt": return "receipt"
        case "crosshair": return "scope"
        case "folder": return "folder"
        case "refresh-cw": return "arrow.triangle.2.circlepath"
        case "list-filter": return "line.3.horizontal.decrease.circle"
        case "mail": return "envelope"
        case "chart-column": return "chart.bar.xaxis"
        case "list": return "list.bullet.rectangle"
        case "settings": return "gearshape"
        case "inbox": return "tray.and.arrow.down.fill"
        case "triangle-alert": return "exclamationmark.triangle.fill"
        case "clock": return "clock.fill"
        case "dollar-sign": return "dollarsign"
        case "banknote": return "banknote"
        case "cloud": return "icloud"
        case "target": return "target"
        case "arrow-left-right": return "arrow.left.arrow.right"
        case "users": return "person.2"
        case "landmark", "building-2": return "building.columns"
        case "house": return "house"
        case "wallet": return "wallet.bifold"
        case "credit-card": return "creditcard"
        case "trending-up": return "chart.line.uptrend.xyaxis"
        case "piggy-bank": return "banknote"
        default: return nil
        }
    }
}
#endif
