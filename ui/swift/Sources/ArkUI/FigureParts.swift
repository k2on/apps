// The parts a page of figures is laid out of, with the same names and
// inputs as the desktop's (`arkui::figures`) and Android's (`dev.arkui`,
// `FigureParts.kt`); ui/docs/parts.md, "Figures", lists them: a figure in
// a capsule of its state, a slim bar toward a goal, a page's headline
// figure, a button in a capsule, a notice bar with its actions, a strip of
// labelled figures, a stat tile, a chart's card, and the charts (bars
// labelled over them, columns in groups with a legend, a line over its
// area, a sparkline), as Swift Charts draws them. Nothing here names a
// thing any app is about: a figure is text the app wrote, a state a
// `Tint`. Every colour is the theme's; every move the system's own.
#if canImport(SwiftUI)
import Charts
import SwiftUI

/// A figure in a capsule of its state: wrong is filled in the error's
/// colour with a warning, worth a look carries a clock, good and nothing
/// are washed in their tint. Colour is never the only sign.
public struct StatusBadge: View {
    let figure: String
    let tint: Tint

    public init(_ figure: String, tint: Tint) {
        self.figure = figure
        self.tint = tint
    }

    public var body: some View {
        HStack(spacing: 4) {
            switch tint {
            case .negative: Image(systemName: "exclamationmark.triangle.fill").imageScale(.small)
            case .caution: Image(systemName: "clock.fill").imageScale(.small)
            default: EmptyView()
            }
            Text(figure).monospacedDigit()
        }
        .font(.callout.weight(.semibold))
        .foregroundStyle(tint == .negative ? Color.onError : tint.color)
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(tint == .negative ? Color.error : tint.color.opacity(tint == .neutral ? 0.12 : 0.16), in: .capsule)
    }
}

/// A slim bar toward a goal under a row's name, `LayoutTokens.progressWidth`
/// wide beside columns, the row's width when compact.
public struct GoalBar: View {
    let fraction: Double
    let tint: Tint
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var size
    private var compact: Bool { size == .compact }
    #else
    private let compact = false
    #endif

    public init(_ fraction: Double, tint: Tint) {
        self.fraction = fraction
        self.tint = tint
    }

    public var body: some View {
        ProgressView(value: min(max(fraction, 0), 1))
            .progressViewStyle(.linear)
            .tint(tint == .neutral ? Tint.positive.color : tint.color)
            .frame(maxWidth: compact ? .infinity : LayoutTokens.progressWidth)
    }
}

extension Tint {
    /// The tint a view names (`ark_app::view`'s words): `positive`,
    /// `caution`, `negative`, `accent`; anything else neutral.
    public init(named name: String) {
        switch name {
        case "positive": self = .positive
        case "caution": self = .caution
        case "negative": self = .negative
        case "accent": self = .accent
        default: self = .neutral
        }
    }
}

/// An inspector's or a detail's page: its cards one under another, a
/// grouped list (the reference's inspectors).
public struct DetailPage<Content: View>: View {
    let content: Content

    public init(@ViewBuilder _ content: () -> Content) { self.content = content() }

    public var body: some View {
        List { content }.groupedList()
    }
}

/// What an inspector or a detail is about, in the list's first card: its
/// glyph, its title, its figure in a `StatusBadge` of its tint (`badge`)
/// or large and rounded, what needs attention in the tint, a quiet
/// caption, centred; its actions as rows under it.
public struct HeaderCard<Actions: View>: View {
    let title: String
    let figure: String
    let tint: Tint
    let glyph: String?
    let badge: Bool
    let line: String?
    let caption: String?
    let actions: Actions

    public init(_ title: String, figure: String, tint: String, glyph: String? = nil, badge: Bool = false,
                line: String? = nil, caption: String? = nil, @ViewBuilder _ actions: () -> Actions) {
        self.title = title
        self.figure = figure
        self.tint = Tint(named: tint)
        self.glyph = glyph
        self.badge = badge
        self.line = line
        self.caption = caption
        self.actions = actions()
    }

    public var body: some View {
        Section {
            VStack(spacing: Space.xs) {
                if let glyph { Tile(symbol: ArkGlyphs.sf(glyph), color: tint.color) }
                if !title.isEmpty { Text(title).font(.headline) }
                if badge {
                    StatusBadge(figure, tint: tint)
                } else {
                    Text(figure)
                        .font(.system(.largeTitle, design: .rounded, weight: .bold))
                        .foregroundStyle(tint == .neutral ? Color.ink : tint.color)
                }
                if let line, !line.isEmpty { Text(line).font(.subheadline).foregroundStyle(tint.color) }
                if let caption, !caption.isEmpty { Text(caption).font(.footnote).foregroundStyle(.quiet) }
            }
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity)
            .padding(.vertical, Space.s)
            .accessibilityElement(children: .combine)
            actions
        }
    }
}

/// How far toward a goal: the bar in its tint, the row's width, and the
/// words under it.
public struct GoalProgress: View {
    let fraction: Double
    let tint: Tint
    let note: String

    public init(_ fraction: Double, tint: String, note: String) {
        self.fraction = fraction
        self.tint = Tint(named: tint)
        self.note = note
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: Space.xs) {
            ProgressView(value: min(max(fraction, 0), 1))
                .progressViewStyle(.linear)
                .tint(tint == .neutral ? Tint.positive.color : tint.color)
            Text(note).font(.footnote).foregroundStyle(.quiet)
        }
        .padding(.vertical, Space.xs)
    }
}

/// A page's headline figure: what it is (small, quiet), the figure large,
/// bold and rounded in its tint, a caption; `trailing` at its end, at its
/// own size (a capsule button), or under it at the accessibility sizes.
public struct Headline<Trailing: View>: View {
    let label: String
    let figure: String
    let tint: Tint
    let caption: String
    let trailing: Trailing
    @Environment(\.dynamicTypeSize) private var type

    public init(_ label: String, figure: String, tint: Tint, caption: String, @ViewBuilder trailing: () -> Trailing) {
        self.label = label
        self.figure = figure
        self.tint = tint
        self.caption = caption
        self.trailing = trailing()
    }

    public var body: some View {
        // Not ViewThatFits: under it, Budget's Auto-Assign menu was seen
        // drawn as a tall, empty capsule on the iPhone.
        let stacked = type.isAccessibilitySize
        let layout = stacked
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: Space.m))
            : AnyLayout(HStackLayout(alignment: .center, spacing: Space.l))
        layout {
            words
            if !stacked { Spacer(minLength: 0) }
            trailing.fixedSize()
        }
        .padding(.vertical, 6)
    }

    private var words: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label).font(.subheadline.weight(.medium)).foregroundStyle(.quiet)
            Text(figure)
                .font(.system(.largeTitle, design: .rounded, weight: .bold))
                .monospacedDigit()
                // One line beside its control, as small as it must be
                // ($1,284.50 broke after its point on the iPhone).
                .lineLimit(1)
                .minimumScaleFactor(0.6)
                .foregroundStyle(tint == .neutral ? Color.ink : tint.color)
            Text(caption).font(.footnote).foregroundStyle(.quiet)
        }
        .accessibilityElement(children: .combine)
    }
}

/// A button in a capsule: an action that sits on a page (Auto-Assign by
/// the headline, Review on a notice bar); `prominent` fills it.
public struct CapsuleButton: View {
    let title: String
    let glyph: String?
    let prominent: Bool
    let action: () -> Void

    public init(_ title: String, glyph: String? = nil, prominent: Bool = false, action: @escaping () -> Void) {
        self.title = title
        self.glyph = glyph
        self.prominent = prominent
        self.action = action
    }

    public var body: some View {
        let b = Button(action: action) {
            if let glyph { Label(title, systemImage: ArkGlyphs.sf(glyph)) } else { Text(title) }
        }
        .controlSize(.small)
        if prominent { b.buttonStyle(.borderedProminent) } else { b.buttonStyle(.bordered) }
    }
}

/// A notice across the top of a list with what can be done about it: its
/// glyph in the accent, its sentence, its actions at its end, on the bar's
/// material.
public struct NoticeBar<Actions: View>: View {
    let glyph: String
    let text: String
    let actions: Actions

    public init(_ glyph: String, text: String, @ViewBuilder actions: () -> Actions) {
        self.glyph = glyph
        self.text = text
        self.actions = actions()
    }

    public var body: some View {
        HStack(spacing: Space.m) {
            Image(systemName: ArkGlyphs.sf(glyph)).foregroundStyle(.tint).imageScale(.large).accessibilityHidden(true)
            Text(text).font(.subheadline.weight(.medium))
            Spacer(minLength: Space.s)
            actions
        }
        .padding(.horizontal)
        .padding(.vertical, Space.s)
        .background(.bar)
    }
}

/// One figure of a `FigureStrip`.
public struct StripFigure: Identifiable {
    public let label: String
    public let figure: String
    public let tint: Tint
    public var id: String { label }

    public init(_ label: String, figure: String, tint: Tint = .neutral) {
        self.label = label
        self.figure = figure
        self.tint = tint
    }
}

/// Figures side by side, each its label over it: an account's cleared,
/// uncleared and working balances.
public struct FigureStrip: View {
    let items: [StripFigure]

    public init(_ items: [StripFigure]) { self.items = items }

    public var body: some View {
        HStack(alignment: .top, spacing: Space.xl) {
            ForEach(items) { i in
                VStack(alignment: .leading, spacing: 1) {
                    Text(i.label).font(.subheadline).foregroundStyle(.quiet)
                    Text(i.figure).font(.body.weight(.semibold)).monospacedDigit()
                        .foregroundStyle(i.tint == .neutral ? Color.ink : i.tint.color)
                }
                .accessibilityElement(children: .combine)
            }
        }
    }
}

/// A tile with one figure: what it is, the figure large, a caption line
/// (kept when there is none, so a row of tiles is one height).
public struct StatTile: View {
    let label: String
    let figure: String
    let caption: String?

    public init(_ label: String, figure: String, caption: String? = nil) {
        self.label = label
        self.figure = figure
        self.caption = caption
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label).font(.subheadline).foregroundStyle(.quiet)
            Text(figure).font(.system(.title, design: .rounded, weight: .bold)).monospacedDigit()
            Text(caption ?? " ").font(.caption).foregroundStyle(.quiet)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(Space.m)
        .background(Color.container, in: .rect(cornerRadius: 10))
        .accessibilityElement(children: .combine)
    }
}

/// A chart in its card under its title.
public struct ChartCard<Chart: View>: View {
    let title: String
    let chart: Chart

    public init(_ title: String, @ViewBuilder chart: () -> Chart) {
        self.title = title
        self.chart = chart()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: Space.s) {
            Text(title).font(.headline)
            chart
                .padding(Space.m)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.container, in: .rect(cornerRadius: 10))
        }
    }
}

/// One bar of `LabelledBars`.
public struct LabelledBar: Identifiable {
    public let label: String
    public let value: Int64
    public let said: String
    public var id: String { label }

    public init(_ label: String, value: Int64, said: String) {
        self.label = label
        self.value = value
        self.said = said
    }
}

/// Bars against the largest, each its label over it and its value at its
/// end: one hue for magnitude (the chart's first), largest first as given.
public struct LabelledBars: View {
    let bars: [LabelledBar]

    public init(_ bars: [LabelledBar]) { self.bars = bars }

    public var body: some View {
        Chart(bars) { b in
            BarMark(x: .value("Amount", b.value), y: .value("Category", b.label))
                .foregroundStyle(Color.chart[0])
                .clipShape(Capsule())
                .annotation(position: .trailing) { Text(b.said).font(.caption2).foregroundStyle(.quiet) }
        }
        .chartXAxis(.hidden)
        .chartYAxis {
            AxisMarks { _ in AxisValueLabel(anchor: .bottomLeading) }
        }
        .frame(height: CGFloat(bars.count) * 34)
    }
}

/// One group of `ColumnChart`: its label and a value per series.
public struct ColumnGroup: Identifiable {
    public let label: String
    public let values: [Int64]
    public var id: String { label }

    public init(_ label: String, values: [Int64]) {
        self.label = label
        self.values = values
    }
}

/// Columns in groups side by side (a month's income beside its spending),
/// each series the chart's colour of its place, a legend over them.
public struct ColumnChart: View {
    let series: [String]
    let groups: [ColumnGroup]

    public init(series: [String], groups: [ColumnGroup]) {
        self.series = series
        self.groups = groups
    }

    public var body: some View {
        Chart {
            ForEach(groups) { g in
                ForEach(Array(g.values.enumerated()), id: \.offset) { i, v in
                    column(g.label, name(i), v)
                }
            }
        }
        .chartForegroundStyleScale(domain: series, range: Array(Color.chart.prefix(series.count)))
        .chartLegend(position: .top, alignment: .leading)
        .frame(height: 200)
    }

    /// The series of the `i`-th value, the last for any past it.
    private func name(_ i: Int) -> String { series[min(i, series.count - 1)] }

    /// One column: its group's place, its series' colour.
    private func column(_ group: String, _ series: String, _ v: Int64) -> some ChartContent {
        BarMark(x: .value("Group", group), y: .value("Amount", v))
            .position(by: .value("Series", series))
            .foregroundStyle(by: .value("Series", series))
            .cornerRadius(3)
    }
}

/// A line through `points` (oldest first) over a light area that runs to
/// the chart's foot, whichever side of nothing the line is.
public struct LineChart: View {
    let points: [Int64]
    let area: Bool

    public init(_ points: [Int64], area: Bool = true) {
        self.points = points
        self.area = area
    }

    public var body: some View {
        let lo = points.min() ?? 0
        Chart(Array(points.enumerated()), id: \.offset) { i, p in
            if area {
                AreaMark(x: .value("Week", i), yStart: .value("Floor", lo), yEnd: .value("Worth", p))
                    .foregroundStyle(Color.chart[0].opacity(0.16))
            }
            LineMark(x: .value("Week", i), y: .value("Worth", p))
                .foregroundStyle(Color.chart[0])
        }
        .chartYScale(domain: .automatic(includesZero: false))
        .chartXAxis(.hidden)
        .frame(height: area ? 160 : nil)
    }
}

/// A small line with no axes and no area: a trend beside a figure.
public struct Sparkline: View {
    let points: [Int64]

    public init(_ points: [Int64]) { self.points = points }

    public var body: some View {
        LineChart(points, area: false)
            .chartYAxis(.hidden)
            .accessibilityLabel("Trend")
    }
}
#endif
