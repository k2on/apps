// A table laid out in Rust (`ark_app::view`'s `data_table`, ui/docs/parts.md,
// Tables) and a day picked in a form: the parts the generated views call by
// these names. A table is rows of cells under its columns' headings on a
// regular width and the Mac, and a list's two-line rows when compact — each
// cell where its column's slot puts it, as the iPhone's `List` rows have
// them. The parts decide which; the app never does.
#if canImport(SwiftUI)
import SwiftUI

/// Where a column's cells line up.
public enum ColumnAlign { case start, end, center }

/// How wide a column is: the theme's figure or mark column, or a share of what is left.
public enum ColumnWidth { case figure, mark, share }

/// Where a column's cell goes in a compact row: none leaves it out.
public enum ColumnSlot { case none, title, subtitle, trailing, trailingSub }

/// A table's column: its heading, its width (the theme's, or `share` parts
/// of what is left), how its cells line up, where they go when compact.
public struct DataColumn {
    let heading: String
    let width: ColumnWidth
    let share: Int
    let align: ColumnAlign
    let slot: ColumnSlot

    public init(_ heading: String, width: ColumnWidth, share: Int, align: ColumnAlign, slot: ColumnSlot) {
        self.heading = heading
        self.width = width
        self.share = share
        self.align = align
        self.slot = slot
    }
}

private struct DataColumnsKey: EnvironmentKey {
    static let defaultValue: [DataColumn] = []
}

private struct DataCompactKey: EnvironmentKey {
    static let defaultValue = true
}

extension EnvironmentValues {
    /// The columns of the table a row is in.
    var dataColumns: [DataColumn] {
        get { self[DataColumnsKey.self] }
        set { self[DataColumnsKey.self] = newValue }
    }

    /// Whether the table a row is in is a list's rows.
    var dataCompact: Bool {
        get { self[DataCompactKey.self] }
        set { self[DataCompactKey.self] = newValue }
    }
}

/// A table: its headings over its rows on a regular width, a list's rows
/// when compact.
public struct DataTable<Content: View>: View {
    let columns: [DataColumn]
    let content: Content
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var size
    private var compact: Bool { size == .compact }
    #else
    private let compact = false
    #endif

    public init(columns: [DataColumn], @ViewBuilder content: () -> Content) {
        self.columns = columns
        self.content = content()
    }

    public var body: some View {
        Section {
            content
        } header: {
            if !compact {
                HStack(spacing: Space.m) {
                    Color.clear.frame(width: DataCell.gutter, height: 1)
                    ForEach(columns.indices, id: \.self) { i in
                        DataCell.framed(Text(columns[i].heading).font(.caption.weight(.semibold)).foregroundStyle(.quiet), columns[i])
                    }
                }
                .accessibilityHidden(true)
            }
        }
        .environment(\.dataColumns, columns)
        .environment(\.dataCompact, compact)
    }
}

/// A heading among a table's rows (a day).
public struct DataTableHeading: View {
    let title: String

    public init(_ title: String) { self.title = title }

    public var body: some View {
        Text(title)
            .font(.footnote.weight(.semibold))
            .foregroundStyle(.quiet)
            .accessibilityAddTraits(.isHeader)
            .listRowBackground(Color.clear)
    }
}

/// One cell's words and glyph, in its tint.
enum DataCell {
    /// The column a row's dot sits in.
    static let gutter: CGFloat = 8

    static func tint(_ name: String) -> Color {
        switch name {
        case "quiet": return .quiet
        case "positive": return .positive
        case "caution": return .caution
        case "negative": return .error
        case "accent": return .accentText
        default: return .ink
        }
    }

    @ViewBuilder static func cell(_ text: String, _ tint: String, _ glyph: String) -> some View {
        HStack(spacing: Space.xs) {
            if !glyph.isEmpty { Image(systemName: ArkGlyphs.sf(glyph)).font(.caption) }
            if !text.isEmpty { Text(text).lineLimit(1) }
        }
        .foregroundStyle(Self.tint(tint))
    }

    /// `v` in its column's width and alignment.
    @ViewBuilder static func framed<V: View>(_ v: V, _ c: DataColumn) -> some View {
        let align: Alignment = c.align == .end ? .trailing : c.align == .center ? .center : .leading
        switch c.width {
        case .share: v.frame(maxWidth: .infinity, alignment: align).layoutPriority(Double(c.share))
        case .figure: v.frame(width: CGFloat(LayoutTokens.amountColumnWidth), alignment: align)
        case .mark: v.frame(width: CGFloat(LayoutTokens.markColumnWidth), alignment: align)
        }
    }
}

/// One row of a table: a cell for each column (their tints and glyphs
/// beside them), a dot before it while `marked`, lit while `selected`;
/// tapped, `onTap`; held, its menu.
public struct DataTableRow: View {
    let cells: [String]
    let tints: [String]
    let glyphs: [String]
    let marked: Bool
    let selected: Bool
    let said: String?
    let menu: [MenuAction]
    let onTap: (() -> Void)?
    @Environment(\.dataColumns) private var columns
    @Environment(\.dataCompact) private var compact

    public init(_ cells: [String], tints: [String] = [], glyphs: [String] = [], marked: Bool = false, selected: Bool = false,
                said: String? = nil, menu: [MenuAction] = [], onTap: (() -> Void)? = nil) {
        self.cells = cells
        self.tints = tints
        self.glyphs = glyphs
        self.marked = marked
        self.selected = selected
        self.said = said
        self.menu = menu
        self.onTap = onTap
    }

    private func at(_ i: Int) -> some View {
        DataCell.cell(cells[i], i < tints.count ? tints[i] : "", i < glyphs.count ? glyphs[i] : "")
    }

    private func slotted(_ s: ColumnSlot) -> [Int] {
        cells.indices.filter { $0 < columns.count && columns[$0].slot == s }
    }

    public var body: some View {
        Button { onTap?() } label: { line.contentShape(Rectangle()) }
            .listRowButton()
            .foregroundStyle(Color.ink)
            .listRowBackground(selected ? Color.accent.opacity(0.18) : nil)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(said ?? cells.first ?? "")
            .accessibilityAddTraits(selected ? .isSelected : [])
            .contextMenu { MenuActions(menu: menu) }
    }

    private var dot: some View {
        Circle().fill(marked ? Color.accent : Color.clear).frame(width: DataCell.gutter, height: DataCell.gutter)
    }

    @ViewBuilder private var line: some View {
        if compact {
            HStack(spacing: Space.m) {
                dot
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(slotted(.title), id: \.self) { i in at(i).font(.body.weight(marked ? .semibold : .regular)) }
                    HStack(spacing: Space.xs) {
                        ForEach(slotted(.subtitle), id: \.self) { i in at(i) }
                    }
                    .font(.caption)
                }
                Spacer(minLength: Space.s)
                VStack(alignment: .trailing, spacing: 2) {
                    ForEach(slotted(.trailing), id: \.self) { i in at(i).monospacedDigit() }
                    HStack(spacing: Space.xs) {
                        ForEach(slotted(.trailingSub), id: \.self) { i in at(i) }
                    }
                    .font(.caption)
                }
            }
        } else {
            HStack(spacing: Space.m) {
                dot
                ForEach(cells.indices.filter { $0 < columns.count }, id: \.self) { i in
                    DataCell.framed(at(i).monospacedDigit(), columns[i])
                }
            }
        }
    }
}

/// Bars against the largest (`LabelledBars`), from a view's three lists.
public struct BarChart: View {
    let bars: [LabelledBar]

    public init(_ labels: [String], values: [Int64], said: [String]) {
        bars = zip(zip(labels, values), said).map { LabelledBar($0.0, value: $0.1, said: $1) }
    }

    public var body: some View { LabelledBars(bars) }
}

/// A day picked: the system's date picker in a form's row, over the day as
/// the domain writes one (`2026-11-11`).
public struct DateField: View {
    let label: String
    let day: String
    let onChange: (String) -> Void

    public init(_ label: String, day: String, onChange: @escaping (String) -> Void) {
        self.label = label
        self.day = day
        self.onChange = onChange
    }

    private static let format: DateFormatter = {
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = "yyyy-MM-dd"
        return f
    }()

    public var body: some View {
        DatePicker(label,
                   selection: Binding(get: { Self.format.date(from: day) ?? Date() }, set: { onChange(Self.format.string(from: $0)) }),
                   displayedComponents: .date)
            .environment(\.timeZone, TimeZone(identifier: "UTC")!)
    }
}
#endif
