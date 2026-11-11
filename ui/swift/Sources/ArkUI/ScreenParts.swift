// The parts a screen laid out in Rust is composed of (ui/docs/app.md):
// each has the name and the inputs its Rust builder declares
// (`ark_app::view::Part::call`), so the generated views (`ark-ui-gen`) are
// only calls to these. Everything a part does is here, by hand, where Xcode
// previews and checks it. Two things an app adds: the face at a tab's
// title (`\.tabFace`) and the mark a row of its own kind wears
// (`\.rowMark`), each set once at the app's root.
#if canImport(SwiftUI)
import SwiftUI

// MARK: what the app provides

private struct TabFaceKey: EnvironmentKey {
    static let defaultValue: () -> AnyView = { AnyView(EmptyView()) }
}

private struct RowMarkKey: EnvironmentKey {
    static let defaultValue: (String) -> AnyView = { _ in AnyView(EmptyView()) }
}

private struct ChoiceSymbolKey: EnvironmentKey {
    static let defaultValue: (String, String) -> String = { _, glyph in ArkGlyphs.sf(glyph) }
}

extension EnvironmentValues {
    /// The face at the end of a tab's large title: the account's button.
    public var tabFace: () -> AnyView {
        get { self[TabFaceKey.self] }
        set { self[TabFaceKey.self] = newValue }
    }

    /// The mark a `ListRow(mark:)` wears, by its kind: an account's.
    public var rowMark: (String) -> AnyView {
        get { self[RowMarkKey.self] }
        set { self[RowMarkKey.self] = newValue }
    }

    /// The SF Symbol a `PickerChoice` shows, by its tag and its glyph: the app's
    /// own (an account kind's), or the glyph's.
    public var choiceSymbol: (String, String) -> String {
        get { self[ChoiceSymbolKey.self] }
        set { self[ChoiceSymbolKey.self] = newValue }
    }
}

extension ArkGlyphs {
    /// The SF Symbol for Lucide's `name`; a question mark for one not shared yet.
    public static func sf(_ name: String) -> String {
        symbol(name) ?? "questionmark"
    }
}

// MARK: a sheet's bar

extension View {
    /// A sheet's form under its bar: the hard scroll edge at its top.
    @ViewBuilder func sheetBarEdge() -> some View {
        if #available(iOS 26.0, macOS 26.0, *) {
            scrollEdgeEffectStyle(.hard, for: .top)
        } else {
            self
        }
    }
}

// MARK: a list's row as a button

extension View {
    /// A list's row that is a button, as the platform draws one: the list's
    /// own highlight on the iPhone and the iPad; on the Mac, where a bare
    /// `Button` is a bordered push button, a plain row darkening under the
    /// pointer (`.pressRow`), so every row is not drawn as a pressed capsule.
    @ViewBuilder public func listRowButton() -> some View {
        #if os(macOS)
        buttonStyle(.pressRow)
        #else
        self
        #endif
    }
}

// MARK: the page

/// A tab's page: an inset grouped list, `top` first, then `content`;
/// pulled down, `onRefresh`. Its title is the section's toolbar's
/// (`WithToolbar`'s `navigationTitle`): large on the iPhone and the iPad,
/// inline on the Mac, and nothing else at the title's end — who is signed
/// in is at the sidebar's foot.
public struct TabPage<Content: View, Top: View>: View {
    let title: String
    let onRefresh: (() -> Void)?
    let content: Content
    let top: Top

    public init(_ title: String, onRefresh: (() -> Void)? = nil, @ViewBuilder content: () -> Content, @ViewBuilder top: () -> Top) {
        self.title = title
        self.onRefresh = onRefresh
        self.content = content()
        self.top = top()
    }

    public var body: some View {
        List {
            top
            content
        }
        .groupedList()
        .refreshable { onRefresh?() }
        .navigationTitle(title)
    }
}

/// The page's figure in a card: its label, the amount large and bold in
/// the rounded face, the two it is made of under it (what is owned, what
/// is owed) and a sparkline of how it went, oldest first.
public struct SummaryCard: View {
    let label: String
    let cents: Int64
    let first: String
    let firstCents: Int64
    let second: String
    let secondCents: Int64
    let trend: [Int64]

    public init(_ label: String, cents: Int64, first: String, firstCents: Int64, second: String, secondCents: Int64, trend: [Int64]) {
        self.label = label
        self.cents = cents
        self.first = first
        self.firstCents = firstCents
        self.second = second
        self.secondCents = secondCents
        self.trend = trend
    }

    public var body: some View {
        Section {
            VStack(alignment: .leading, spacing: Space.xs) {
                Text(label).font(.subheadline).foregroundStyle(.quiet)
                CentsText(cents)
                    .font(.system(.largeTitle, design: .rounded, weight: .bold))
                    .foregroundStyle(cents < 0 ? Color.error : Color.ink)
                    .contentTransition(.numericText(value: Double(cents)))
                    .minimumScaleFactor(0.5)
                    .lineLimit(1)
                // A figure never wraps: the facts and the sparkline side by
                // side while they fit, else the sparkline under the facts;
                // each figure is as wide as it is written.
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .bottom, spacing: Space.l) {
                        facts
                        Spacer(minLength: Space.s)
                        Sparkline(trend).frame(width: 140, height: 34)
                    }
                    VStack(alignment: .leading, spacing: Space.s) {
                        facts
                        Sparkline(trend).frame(maxWidth: .infinity).frame(height: 34)
                    }
                }
            }
            .padding(.vertical, Space.xs)
            .accessibilityElement(children: .combine)
        }
    }

    private var facts: some View {
        HStack(alignment: .bottom, spacing: Space.l) {
            fact(first, firstCents)
            fact(second, secondCents)
        }
    }

    private func fact(_ title: String, _ c: Int64) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(title).font(.caption).foregroundStyle(.quiet).lineLimit(1)
            CentsText(c).font(.footnote.weight(.semibold)).foregroundStyle(c < 0 ? Color.error : Color.ink)
                .lineLimit(1)
                .fixedSize(horizontal: true, vertical: false)
        }
    }
}

/// A list's section in its card: a heading with a total at its end (the
/// error's red below nothing) or words in the caution's tint in its place,
/// a note under it. On a regular width two more figures stand before the
/// total, each under its column's title. A collapsible one's heading is a
/// button with its chevron, its rows shown while `open`; held, its menu.
public struct CardSection<Content: View>: View {
    let title: String?
    let total: Int64?
    let showsTotal: Bool
    let footer: String?
    let totalNote: String?
    let titles: [String]?
    let first: Int64?
    let second: Int64?
    let open: Bool
    let onToggle: (() -> Void)?
    let menu: [MenuAction]
    let content: Content
    @Environment(\.reducesMotion) private var still
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var size
    private var columns: Bool { size != .compact }
    #else
    private let columns = true
    #endif

    public init(title: String? = nil, total: Int64? = nil, showsTotal: Bool = true, footer: String? = nil,
                totalNote: String? = nil, firstTitle: String? = nil, secondTitle: String? = nil, totalTitle: String? = nil,
                first: Int64? = nil, second: Int64? = nil, open: Bool = true, onToggle: (() -> Void)? = nil,
                menu: [MenuAction] = [], @ViewBuilder content: () -> Content) {
        self.title = title
        self.total = total
        self.showsTotal = showsTotal
        self.footer = footer
        self.totalNote = totalNote
        if let firstTitle, let secondTitle, let totalTitle { titles = [firstTitle, secondTitle, totalTitle] } else { titles = nil }
        self.first = first
        self.second = second
        self.open = open
        self.onToggle = onToggle
        self.menu = menu
        self.content = content()
    }

    public var body: some View {
        Section {
            if open { content }
        } header: {
            if let title {
                if let onToggle {
                    Button { withAnimation(Springs.move.reduced(still)) { onToggle() } } label: { heading(title) }
                        .buttonStyle(.plain)
                        .accessibilityHint(open ? "Collapses the group" : "Expands the group")
                        .contextMenu { MenuActions(menu: menu) }
                } else {
                    heading(title).contextMenu { MenuActions(menu: menu) }
                }
            }
        } footer: {
            if let footer { Text(footer) }
        }
    }

    private func heading(_ title: String) -> some View {
        HStack(alignment: .lastTextBaseline) {
            if onToggle != nil {
                Image(systemName: "chevron.right")
                    .font(.caption.weight(.bold))
                    .rotationEffect(.degrees(open ? 90 : 0))
            }
            Text(title)
            Spacer()
            if let titles, columns, let first, let second {
                column(titles[0]) { CentsText(first) }
                column(titles[1]) { CentsText(second) }
                column(titles[2]) { figure }
            } else {
                figure
            }
        }
        .contentShape(Rectangle())
    }

    @ViewBuilder private var figure: some View {
        if let totalNote {
            Text(totalNote).font(.subheadline.weight(.semibold)).monospacedDigit().foregroundStyle(Color.caution)
        } else if let total, showsTotal {
            CentsText(total).foregroundStyle(total < 0 ? Color.error : Color.quiet)
        }
    }

    private func column<V: View>(_ title: String, @ViewBuilder _ v: () -> V) -> some View {
        VStack(alignment: .trailing, spacing: 0) {
            Text(title).font(.caption2).foregroundStyle(.tertiary)
            v()
        }
        .frame(width: LayoutTokens.amountColumnWidth, alignment: .trailing)
    }
}

/// A menu's actions, those offered, as a context menu lists them.
struct MenuActions: View {
    let menu: [MenuAction]

    var body: some View {
        ForEach(menu.indices, id: \.self) { i in
            let m = menu[i]
            if m.shown {
                Button(role: m.destructive ? .destructive : nil, action: m.onTap) {
                    Label(m.title, systemImage: ArkGlyphs.sf(m.glyph))
                }
            }
        }
    }
}

/// A row of a thing toward a goal: the app's mark before its title (faded
/// while hidden), a slim bar of how far in its tint, words under it in a
/// tint with quiet ones at their end; on a regular width two figures in
/// columns; its figure in a status badge at its end, rolling as it
/// changes. Tapped, `onTap`; held, its menu.
public struct MeterRow: View {
    let title: String
    let status: String
    let statusTint: Tint
    var mark: String? = nil
    var faded = false
    var fraction: Double? = nil
    var meterTint: Tint = .positive
    var note: String? = nil
    var noteTint: Tint = .neutral
    var noteEnd: String? = nil
    var first: Int64? = nil
    var second: Int64? = nil
    var said: String? = nil
    var menu: [MenuAction] = []
    var onTap: (() -> Void)? = nil
    @Environment(\.rowMark) private var rowMark
    @Environment(\.dynamicTypeSize) private var type
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var size
    private var regular: Bool { size != .compact }
    #else
    private let regular = true
    #endif

    public init(_ title: String, status: String, statusTint: String, mark: String? = nil, faded: Bool = false,
                fraction: Double? = nil, meterTint: String = "positive", note: String? = nil, noteTint: String = "neutral",
                noteEnd: String? = nil, first: Int64? = nil, second: Int64? = nil, said: String? = nil,
                menu: [MenuAction] = [], onTap: (() -> Void)? = nil) {
        self.title = title
        self.status = status
        self.statusTint = Tint(named: statusTint)
        self.mark = mark
        self.faded = faded
        self.fraction = fraction
        self.meterTint = Tint(named: meterTint)
        self.note = note
        self.noteTint = Tint(named: noteTint)
        self.noteEnd = noteEnd
        self.first = first
        self.second = second
        self.said = said
        self.menu = menu
        self.onTap = onTap
    }

    public var body: some View {
        Button { onTap?() } label: { line.contentShape(Rectangle()) }
            .listRowButton()
            .foregroundStyle(Color.ink)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(said ?? title)
            .contextMenu { MenuActions(menu: menu) }
    }

    private var line: some View {
        HStack(spacing: Space.m) {
            VStack(alignment: .leading, spacing: Space.xs) {
                HStack(spacing: Space.s) {
                    if let mark { rowMark(mark) }
                    Text(title).foregroundStyle(faded ? Color.quiet : Color.ink).lineLimit(2)
                }
                if let fraction { GoalBar(fraction, tint: meterTint) }
                if note != nil || noteEnd != nil {
                    HStack {
                        if let note { Text(note).foregroundStyle(noteTint == .neutral ? Color.quiet : noteTint.color) }
                        Spacer(minLength: Space.s)
                        if let noteEnd { Text(noteEnd).foregroundStyle(.quiet) }
                    }
                    .font(.caption)
                    .monospacedDigit()
                }
            }
            Spacer(minLength: Space.s)
            if regular && !type.isAccessibilitySize, let first, let second {
                CentsText(first).frame(width: LayoutTokens.amountColumnWidth, alignment: .trailing)
                CentsText(second).foregroundStyle(.quiet).frame(width: LayoutTokens.amountColumnWidth, alignment: .trailing)
                StatusBadge(status, tint: statusTint).rolls(on: status)
                    .frame(width: LayoutTokens.amountColumnWidth, alignment: .trailing)
            } else {
                StatusBadge(status, tint: statusTint).rolls(on: status)
            }
        }
        .padding(.vertical, 2)
    }
}

extension HeroFigure where Footer == EmptyView {
    /// The figure in the neutral tint.
    public init(_ caption: String, cents: Int64, note: String) {
        self.init(caption, cents: cents, tint: .neutral, note: note)
    }
}

// MARK: rows

/// One thing a row's menu offers.
public struct MenuAction {
    let title: String
    let glyph: String
    let destructive: Bool
    /// Offered only while this holds.
    let shown: Bool
    let onTap: () -> Void

    public init(_ title: String, glyph: String, destructive: Bool = false, shown: Bool = true, onTap: @escaping () -> Void) {
        self.title = title
        self.glyph = glyph
        self.destructive = destructive
        self.shown = shown
        self.onTap = onTap
    }
}

/// A row of a list: its picture (a tile, the app's mark, or a bare glyph
/// in the accent), its title and the line under it, a count in a badge, an
/// amount; tapped, `onTap`; held, its menu. One that `pushes` opens a page
/// and looks it, as a `NavigationLink`'s row does: the list's own
/// highlight under a finger, the disclosure chevron at its end. (A
/// `NavigationLink` itself needs the route as a value its stack resolves;
/// the route here is the app's update's to choose, so the row is a button
/// the shell's navigator answers, dressed as the link.)
public struct ListRow: View {
    let title: String
    var subtitle: String? = nil
    var tile: String? = nil
    var mark: String? = nil
    var glyph: String? = nil
    var badge: Int64? = nil
    var amount: Int64? = nil
    var value: String? = nil
    var quiet = false
    var accent = false
    var pushes = false
    var said: String? = nil
    var menu: [MenuAction] = []
    var onTap: (() -> Void)? = nil
    @Environment(\.rowMark) private var rowMark

    public init(_ title: String, subtitle: String? = nil, tile: String? = nil, mark: String? = nil, glyph: String? = nil,
                badge: Int64? = nil, amount: Int64? = nil, value: String? = nil, quiet: Bool = false, accent: Bool = false, pushes: Bool = false,
                said: String? = nil, menu: [MenuAction] = [], onTap: (() -> Void)? = nil) {
        self.title = title
        self.subtitle = subtitle
        self.tile = tile
        self.mark = mark
        self.glyph = glyph
        self.badge = badge
        self.amount = amount
        self.value = value
        self.quiet = quiet
        self.accent = accent
        self.pushes = pushes
        self.said = said
        self.menu = menu
        self.onTap = onTap
    }

    public var body: some View {
        Button { onTap?() } label: { line.contentShape(Rectangle()) }
            .listRowButton()
            .foregroundStyle(accent ? Color.accent : Color.ink)
            .accessibilityLabel(said ?? title)
            .accessibilityHint(pushes ? "Opens" : "")
            .contextMenu {
                ForEach(menu.indices, id: \.self) { i in
                    let m = menu[i]
                    if m.shown {
                        Button(role: m.destructive ? .destructive : nil, action: m.onTap) {
                            Label(m.title, systemImage: ArkGlyphs.sf(m.glyph))
                        }
                    }
                }
            }
    }

    private var line: some View {
        HStack(spacing: Space.m) {
            if let tile { Tile(symbol: ArkGlyphs.sf(tile), color: .quiet) }
            if let mark { rowMark(mark) }
            if let glyph { Image(systemName: ArkGlyphs.sf(glyph)).foregroundStyle(Color.accent) }
            VStack(alignment: .leading, spacing: 2) {
                Text(title).lineLimit(1)
                if let subtitle {
                    Text(subtitle).font(.caption).foregroundStyle(.quiet).lineLimit(1)
                }
            }
            Spacer(minLength: Space.s)
            if let badge { Badge(Int(badge), said: "\(badge) to review") }
            if let amount {
                CentsText(amount).foregroundStyle(quiet ? Color.quiet : amount < 0 ? Color.error : Color.ink)
            }
            if let value { Text(value).foregroundStyle(.quiet).lineLimit(1) }
            if pushes {
                // The disclosure a NavigationLink's row draws.
                Image(systemName: "chevron.forward")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

// MARK: what rises over a page

/// A sheet up for as long as it is drawn (inside the `if let` of what it
/// edits); put away by the person, `onDismiss`. With a `confirm`, its bar
/// has the system's two: Cancel (an ✕) and the confirming ✓, named
/// `confirm` and enabled while `canConfirm`.
public struct FormSheet<Content: View>: View {
    let title: String
    let onDismiss: () -> Void
    let confirm: String?
    let canConfirm: Bool
    let onConfirm: (() -> Void)?
    let content: Content

    public init(_ title: String, onDismiss: @escaping () -> Void, confirm: String? = nil, canConfirm: Bool = true,
                onConfirm: (() -> Void)? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.onDismiss = onDismiss
        self.confirm = confirm
        self.canConfirm = canConfirm
        self.onConfirm = onConfirm
        self.content = content()
    }

    public var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .listRowBackground(Color.clear)
            .accessibilityHidden(true)
            .sheet(isPresented: Binding(get: { true }, set: { if !$0 { onDismiss() } })) {
                NavigationStack {
                    Form { content }
                        // What scrolls under the sheet's bar is cut off there, not blurred
                        // through it: a large figure under the title read as a smudge.
                        .sheetBarEdge()
                        .navigationTitle(title)
                        .inlineTitle()
                        .toolbar {
                            if let confirm, let onConfirm {
                                ToolbarItem(placement: .cancellationAction) {
                                    Button(role: .cancel, action: onDismiss) { Image(systemName: "xmark") }
                                        .accessibilityLabel("Cancel")
                                }
                                ToolbarItem(placement: .confirmationAction) {
                                    Button(role: .confirm, action: onConfirm) { Image(systemName: "checkmark") }
                                        .accessibilityLabel(confirm)
                                        .disabled(!canConfirm)
                                }
                            }
                        }
                }
            }
    }
}

/// An alert with one text field, up for as long as it is drawn.
public struct Prompt: View {
    let title: String
    let text: String
    let onChange: (String) -> Void
    let confirm: String
    let onConfirm: () -> Void
    let onCancel: () -> Void

    public init(_ title: String, text: String, onChange: @escaping (String) -> Void, confirm: String,
                onConfirm: @escaping () -> Void, onCancel: @escaping () -> Void) {
        self.title = title
        self.text = text
        self.onChange = onChange
        self.confirm = confirm
        self.onConfirm = onConfirm
        self.onCancel = onCancel
    }

    public var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .listRowBackground(Color.clear)
            .accessibilityHidden(true)
            .alert(title, isPresented: Binding(get: { true }, set: { if !$0 { onCancel() } })) {
                TextField("Name", text: Binding(get: { text }, set: onChange))
                Button("Cancel", role: .cancel, action: onCancel)
                Button(confirm, action: onConfirm)
            }
    }
}

// MARK: a form's fields

extension NameField {
    /// The field over text the app keeps: what is typed goes to `onChange`.
    public init(_ label: String, text: String, problem: String?, onChange: @escaping (String) -> Void) {
        self.init(name: Binding(get: { text }, set: onChange), problem: problem, label: label)
    }
}

/// An amount typed as text, the decimal pad; the core says what it means.
public struct AmountField: View {
    let label: String
    let text: String
    let problem: String?
    let onChange: (String) -> Void

    public init(_ label: String, text: String, problem: String?, onChange: @escaping (String) -> Void) {
        self.label = label
        self.text = text
        self.problem = problem
        self.onChange = onChange
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: Space.xs) {
            TextField(label, text: Binding(get: { text }, set: onChange))
                .keypad(.decimal)
                .monospacedDigit()
            if let problem {
                Text(problem).font(.caption).foregroundStyle(Color.error)
            }
        }
    }
}

/// A group of a form's fields: a `Form`'s section, its heading over it and
/// its note under it.
public struct FormSection<Content: View>: View {
    let header: String?
    let footer: String?
    let content: Content

    public init(header: String? = nil, footer: String? = nil, @ViewBuilder content: () -> Content) {
        self.header = header
        self.footer = footer
        self.content = content()
    }

    public var body: some View {
        Section {
            content
        } header: {
            if let header { Text(header) }
        } footer: {
            if let footer { Text(footer) }
        }
    }
}

/// One of several, chosen with the system's picker (a menu in a form's
/// row); its choices are `PickerChoice`s, each tagged.
public struct OptionPicker<Content: View>: View {
    let label: String
    let selection: String
    let onChange: (String) -> Void
    let content: Content

    public init(_ label: String, selection: String, onChange: @escaping (String) -> Void, @ViewBuilder content: () -> Content) {
        self.label = label
        self.selection = selection
        self.onChange = onChange
        self.content = content()
    }

    public var body: some View {
        Picker(label, selection: Binding(get: { selection }, set: onChange)) { content }
    }
}

/// One choice of an `OptionPicker`: its symbol (the app's, by its tag,
/// through `\.choiceSymbol`) and its title, tagged.
public struct PickerChoice: View {
    let title: String
    let glyph: String
    let tag: String
    @Environment(\.choiceSymbol) private var symbol

    public init(_ title: String, glyph: String, tag: String) {
        self.title = title
        self.glyph = glyph
        self.tag = tag
    }

    public var body: some View {
        Label(title, systemImage: symbol(tag, glyph)).tag(tag)
    }
}

extension AmountEntry {
    /// The amount as money takes it on the iPhone: the figure over the
    /// keypad, in cents (`onCents`), `prompt` under it, in a form's row.
    /// The label, the text and the problem are the text field's elsewhere.
    public init(_ label: String, prompt: String, text: String, cents: Int64, problem: String?,
                onType: @escaping (String) -> Void, onCents: @escaping (Int64) -> Void) {
        self.init(cents: Binding(get: { cents }, set: onCents), prompt: prompt, inset: Space.m)
    }
}

/// Chips in a row that scrolls sideways.
public struct ChipRow<Content: View>: View {
    let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: Space.s) { content }
        }
    }
}

extension Chip {
    /// A chip by its label; its glyph is the desktop's and Android's.
    public init(_ label: String, glyph: String, selected: Bool, onTap: @escaping () -> Void) {
        self.init(label, selected: selected, action: onTap)
    }
}

/// A switch with its title and a note under it.
public struct SwitchRow: View {
    let title: String
    let note: String?
    let isOn: Bool
    let onChange: (Bool) -> Void

    public init(_ title: String, note: String? = nil, isOn: Bool, onChange: @escaping (Bool) -> Void) {
        self.title = title
        self.note = note
        self.isOn = isOn
        self.onChange = onChange
    }

    public var body: some View {
        Toggle(isOn: Binding(get: { isOn }, set: onChange)) {
            Text(title)
            if let note { Text(note).font(.caption).foregroundStyle(.quiet) }
        }
    }
}

/// A form's button.
public struct FormButton: View {
    let title: String
    let enabled: Bool
    let onTap: () -> Void

    public init(_ title: String, enabled: Bool, onTap: @escaping () -> Void) {
        self.title = title
        self.enabled = enabled
        self.onTap = onTap
    }

    public var body: some View {
        Button(title, action: onTap).disabled(!enabled)
    }
}

// MARK: A first run's parts (ark_app::welcome)

/// A first run's page: the app's mark on glass in its accent, a large
/// title, a sentence, and what to do (`WelcomePage`'s look).
@available(iOS 26.0, *)
public struct FirstRunPage<Content: View>: View {
    let glyph: String
    let title: String
    let sentence: String
    let content: Content

    public init(_ glyph: String, title: String, sentence: String, @ViewBuilder content: () -> Content) {
        self.glyph = glyph
        self.title = title
        self.sentence = sentence
        self.content = content()
    }

    public var body: some View {
        WelcomePage(symbol: glyph == "app" ? glyph : ArkGlyphs.sf(glyph), tint: Color.accent, title: title, sentence: sentence) {
            VStack(spacing: Space.m) { content }
        }
    }
}

/// A server's address: the keyboard for a URL, no capitals or corrections,
/// Go to go on; its note under it until there is a problem.
public struct AddressField: View {
    let label: String
    let prompt: String
    let text: String
    let note: String
    let problem: String?
    let enabled: Bool
    let onChange: (String) -> Void
    let onSubmit: () -> Void

    public init(_ label: String, prompt: String, text: String, note: String, problem: String?, enabled: Bool,
                onChange: @escaping (String) -> Void, onSubmit: @escaping () -> Void) {
        self.label = label
        self.prompt = prompt
        self.text = text
        self.note = note
        self.problem = problem
        self.enabled = enabled
        self.onChange = onChange
        self.onSubmit = onSubmit
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: Space.s) {
            TextField(prompt, text: Binding(get: { text }, set: onChange))
                .textContentType(.URL)
                .keypad(.url)
                .capitals(.never)
                .autocorrectionDisabled()
                .submitLabel(.go)
                .onSubmit(onSubmit)
                .disabled(!enabled)
                .padding(Space.l)
                .surface(in: RoundedRectangle(cornerRadius: 16, style: .continuous), interactive: true)
                .accessibilityLabel(label)
            Text(problem ?? note)
                .font(.footnote)
                .foregroundStyle(problem == nil ? Color.quiet : Color.error)
                .padding(.horizontal, Space.xs)
        }
    }
}

/// A button the width of its page: the accent filled on glass, or glass;
/// while busy, a spinner before its title and no press.
@available(iOS 26.0, *)
public struct WideButton: View {
    let title: String
    let prominent: Bool
    let enabled: Bool
    let busy: Bool
    let onTap: () -> Void

    public init(_ title: String, prominent: Bool, enabled: Bool, busy: Bool, onTap: @escaping () -> Void) {
        self.title = title
        self.prominent = prominent
        self.enabled = enabled
        self.busy = busy
        self.onTap = onTap
    }

    public var body: some View {
        let b = Button(action: onTap) {
            HStack {
                if busy { ProgressView() }
                Text(title)
            }
            .frame(maxWidth: .infinity)
        }
        .controlSize(.large)
        .disabled(!enabled || busy)
        // Its title is its name, whatever the spinner beside it says.
        .accessibilityLabel(title)
        if prominent {
            b.buttonStyle(.glassProminent).tint(Color.accent)
        } else {
            b.buttonStyle(.glass)
        }
    }
}

/// What a page waits on, quietly, a spinner beside it while busy; what
/// went wrong in the error colour.
public struct StatusLine: View {
    let text: String?
    let busy: Bool
    let problem: String?

    public init(_ text: String?, busy: Bool, problem: String?) {
        self.text = text
        self.busy = busy
        self.problem = problem
    }

    public var body: some View {
        VStack(spacing: Space.s) {
            if let problem {
                Text(problem).font(.callout).foregroundStyle(Color.error).multilineTextAlignment(.center)
            }
            if text != nil || busy {
                HStack(spacing: Space.m) {
                    if busy { ProgressView() }
                    if let text { Text(text) }
                }
                .font(.footnote)
                .foregroundStyle(.quiet)
                .multilineTextAlignment(.center)
            }
        }
    }
}

/// A QR code (`QrCode`), no wider than a phone's hand can hold up to another.
public struct CodeImage: View {
    let size: Int64
    let dark: [Bool]

    public init(_ size: Int64, dark: [Bool]) {
        self.size = size
        self.dark = dark
    }

    public var body: some View {
        QrCode(size: Int(size), dark: dark).frame(maxWidth: 260)
    }
}
#endif
