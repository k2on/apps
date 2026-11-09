// The parts a screen laid out in Rust is composed of (ui/docs/app.md):
// each has the name and the inputs its Rust builder declares
// (`ark_app::view::Part::call`), so the generated views (`ark-ui-gen`) are
// only calls to these. Everything a part does is here, by hand, where Xcode
// previews and checks it. Two things an app adds: the face at a tab's
// title (`\.tabFace`) and the mark a row of its own kind wears
// (`\.rowMark`), each set once at the app's root.
#if os(iOS)
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

// MARK: the page

/// A tab's page: an inset grouped list under the large title (the face at
/// its end), `top` under the title, then `content`; pulled down, `onRefresh`.
public struct TabPage<Content: View, Top: View>: View {
    let title: String
    let onRefresh: (() -> Void)?
    let content: Content
    let top: Top
    @Environment(\.tabFace) private var face
    /// How tall the large title's row is: past it, the inline title comes into the bar.
    @State private var fold: CGFloat = 0

    public init(_ title: String, onRefresh: (() -> Void)? = nil, @ViewBuilder content: () -> Content, @ViewBuilder top: () -> Top) {
        self.title = title
        self.onRefresh = onRefresh
        self.content = content()
        self.top = top()
    }

    public var body: some View {
        List {
            TitleSection(title, fold: $fold) { face() } below: { top } rows: { EmptyView() }
            content
        }
        .listStyle(.insetGrouped)
        .refreshable { onRefresh?() }
        .largeTitleBar(title, fold: fold)
    }
}

/// A list's section in its card: a heading with a total at its end (the
/// error's red below nothing), a note under it.
public struct CardSection<Content: View>: View {
    let title: String?
    let total: Int64?
    let showsTotal: Bool
    let footer: String?
    let content: Content

    public init(title: String? = nil, total: Int64? = nil, showsTotal: Bool = true, footer: String? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.total = total
        self.showsTotal = showsTotal
        self.footer = footer
        self.content = content()
    }

    public var body: some View {
        Section {
            content
        } header: {
            if let title {
                HStack {
                    Text(title)
                    Spacer()
                    if let total, showsTotal {
                        CentsText(total).foregroundStyle(total < 0 ? Color.error : Color.quiet)
                    }
                }
            }
        } footer: {
            if let footer { Text(footer) }
        }
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
    var quiet = false
    var accent = false
    var pushes = false
    var said: String? = nil
    var menu: [MenuAction] = []
    var onTap: (() -> Void)? = nil
    @Environment(\.rowMark) private var rowMark

    public init(_ title: String, subtitle: String? = nil, tile: String? = nil, mark: String? = nil, glyph: String? = nil,
                badge: Int64? = nil, amount: Int64? = nil, quiet: Bool = false, accent: Bool = false, pushes: Bool = false,
                said: String? = nil, menu: [MenuAction] = [], onTap: (() -> Void)? = nil) {
        self.title = title
        self.subtitle = subtitle
        self.tile = tile
        self.mark = mark
        self.glyph = glyph
        self.badge = badge
        self.amount = amount
        self.quiet = quiet
        self.accent = accent
        self.pushes = pushes
        self.said = said
        self.menu = menu
        self.onTap = onTap
    }

    public var body: some View {
        Button { onTap?() } label: { line }
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
                        .navigationTitle(title)
                        .navigationBarTitleDisplayMode(.inline)
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
                .keyboardType(.decimalPad)
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
#endif
