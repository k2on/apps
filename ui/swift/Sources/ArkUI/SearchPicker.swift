// One of many, found by typing (`ark_app::view::search_picker`, ui/docs/
// parts.md "SearchPicker"): a form's row saying what is chosen, and
// pressed, the searchable list — the choices by their groups, the chosen
// one checked, a search field with the keyboard. On the iPhone (a compact
// width) it is a sheet: its title, Cancel, the search field in its bar
// (`.searchable`), at half height and pulled to full (`.medium`,
// `.large`), floating on glass. On the iPad and the Mac it is a popover
// anchored to the row, holding the same list. Android's is a bottom sheet
// when compact and an anchored panel when wider, the same way.
//
// What is found is `ark_app::view::search`'s rule, in Foundation's words:
// folded (case and diacritics off), every word of what is typed in the
// title or the group's name, in the app's order under the groups.
//
// The choices are data, not rows: each `SearchChoice` hands its title,
// glyph, tag, group and detail up as a container value, and the picker
// reads them with `Group(subviews:)` and draws its own list, so the
// generated `ForEach` and `if` work as they do in any other part's slot.
//
// Motion: the sheet and the popover are the system's (motion.md, "Sheet",
// "Menu, popover"), and nothing in them moves: the keys move the lit row
// at once. Rows press as a list's rows do; a choice made ticks
// (`.sensoryFeedback(.selection)`), on the change itself.
import SwiftUI

/// One choice as the picker reads it.
public struct SearchChoiceValue: Hashable, Sendable {
    public let title: String
    public let symbol: String
    public let tag: String
    public let group: String
    public let detail: String
    public let tint: String
}

private struct SearchChoiceKey: ContainerValueKey {
    static let defaultValue: SearchChoiceValue? = nil
}

extension ContainerValues {
    /// The choice a `SearchChoice` stands for, read by its `SearchPicker`.
    var searchChoice: SearchChoiceValue? {
        get { self[SearchChoiceKey.self] }
        set { self[SearchChoiceKey.self] = newValue }
    }
}

/// One choice of a `SearchPicker`: its title, its glyph's symbol, the tag choosing
/// it sends, the group it is listed under, and words at its end in a tint.
/// Drawn by the picker, not by itself.
public struct SearchChoice: View {
    let title: String
    let glyph: String
    let tag: String
    let group: String
    let detail: String
    let tint: String

    public init(_ title: String, glyph: String, tag: String, group: String = "", detail: String = "", tint: String = "neutral") {
        self.title = title
        self.glyph = glyph
        self.tag = tag
        self.group = group
        self.detail = detail
        self.tint = tint
    }

    public var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .containerValue(\.searchChoice, SearchChoiceValue(
                title: title, symbol: ArkGlyphs.sf(glyph), tag: tag, group: group, detail: detail, tint: tint))
    }
}

/// What a search picker finds: `ark_app::view::search`'s rule.
public enum SearchFinding {
    /// `s` as it is matched: case and accents off.
    public static func fold(_ s: String) -> String {
        s.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil).lowercased()
    }

    /// Whether every word of `query` is in `c`'s title or group.
    public static func matches(_ query: String, _ c: SearchChoiceValue) -> Bool {
        let hay = fold(c.title + " " + c.group)
        return fold(query).split(whereSeparator: \.isWhitespace).allSatisfy { hay.contains($0) }
    }

    /// What `query` finds of `choices`, under their groups in the order given.
    public static func found(_ choices: [SearchChoiceValue], _ query: String) -> [SearchGroup] {
        var groups: [SearchGroup] = []
        for c in choices where matches(query, c) {
            if let i = groups.firstIndex(where: { $0.name == c.group }) {
                groups[i].choices.append(c)
            } else {
                groups.append(SearchGroup(name: c.group, choices: [c]))
            }
        }
        return groups
    }
}

/// The choices found under one group.
public struct SearchGroup: Identifiable, Sendable {
    public let name: String
    public var choices: [SearchChoiceValue]
    public var id: String { name }
}

/// One of many, found by typing: the row says `label` and the chosen one's
/// title (or `unset` after a question mark, in the caution tint, while none
/// is); pressed, the dropdown. Choosing sends `onChange` with its tag.
/// VoiceOver hears a button, its label and its value; the list as a list
/// of buttons under headings, the chosen one selected; the search field by
/// its prompt.
public struct SearchPicker<Content: View>: View {
    let label: String
    let selection: String
    let unset: String
    let prompt: String
    let onChange: (String) -> Void
    let content: Content
    @State private var open = false

    public init(_ label: String, selection: String, unset: String = "", prompt: String = "Search",
                onChange: @escaping (String) -> Void, @ViewBuilder content: () -> Content) {
        self.label = label
        self.selection = selection
        self.unset = unset
        self.prompt = prompt
        self.onChange = onChange
        self.content = content()
    }

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var size
    private var compact: Bool { size == .compact }
    #else
    private let compact = false
    #endif

    public var body: some View {
        Group(subviews: content) { subviews in
            let choices = subviews.compactMap { $0.containerValues.searchChoice }
            let chosen = choices.first { $0.tag == selection }
            let list = SearchList(label: label, prompt: prompt, choices: choices, selection: selection,
                                  choose: { tag in
                                      open = false
                                      if tag != selection { onChange(tag) }
                                  },
                                  close: { open = false })
            Button { open = true } label: { face(chosen).contentShape(Rectangle()) }
                .listRowButton()
                .foregroundStyle(Color.ink)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(label)
                .accessibilityValue(chosen?.title ?? unset)
                .sheet(isPresented: Binding(get: { open && compact }, set: { open = $0 })) {
                    list.presentationDetents([.medium, .large])
                }
                .popover(isPresented: Binding(get: { open && !compact }, set: { open = $0 }),
                         attachmentAnchor: .rect(.bounds), arrowEdge: .top) {
                    list.frame(width: CGFloat(LayoutTokens.dropdownWidth), height: CGFloat(LayoutTokens.dropdownHeight))
                }
        }
        .sensoryFeedback(.selection, trigger: selection)
    }

    private func face(_ chosen: SearchChoiceValue?) -> some View {
        HStack(spacing: Space.s) {
            Text(label)
            Spacer(minLength: Space.s)
            if let chosen {
                Image(systemName: chosen.symbol).foregroundStyle(.quiet)
                Text(chosen.title).foregroundStyle(.quiet).lineLimit(1)
            } else {
                Image(systemName: ArkGlyphs.sf("circle-help")).foregroundStyle(Color.caution)
                Text(unset).foregroundStyle(Color.caution).lineLimit(1)
            }
            Image(systemName: ArkGlyphs.sf("chevrons-up-down"))
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.quiet)
        }
    }
}

/// The searchable list, in the sheet or the popover: on Apple's phones and
/// tablets a `NavigationStack` under its title, Cancel, and the search field
/// in its bar, focused as it opens; on the Mac, where a popover has no bar,
/// the search field over the list.
private struct SearchList: View {
    let label: String
    let prompt: String
    let choices: [SearchChoiceValue]
    let selection: String
    let choose: (String) -> Void
    let close: () -> Void
    @State private var query = ""
    @State private var lit: String?
    @State private var searching = false

    init(label: String, prompt: String, choices: [SearchChoiceValue], selection: String,
         choose: @escaping (String) -> Void, close: @escaping () -> Void) {
        self.label = label
        self.prompt = prompt
        self.choices = choices
        self.selection = selection
        self.choose = choose
        self.close = close
        _lit = State(initialValue: choices.contains { $0.tag == selection } ? selection : choices.first?.tag)
    }

    private var found: [SearchGroup] { SearchFinding.found(choices, query) }
    private var listed: [SearchChoiceValue] { found.flatMap(\.choices) }

    var body: some View {
        #if os(macOS)
        VStack(spacing: 0) {
            SearchField(text: $query, prompt: prompt, autofocus: true, onSearch: chooseLit)
                .padding(Space.s)
            Divider()
            rows
        }
        .keys(step: step, close: close)
        .onChange(of: query) { _, _ in lit = listed.first?.tag }
        #else
        NavigationStack {
            rows
                .navigationTitle(label)
                .navigationBarTitleDisplayMode(.inline)
                .searchable(text: $query, isPresented: $searching,
                            placement: .navigationBarDrawer(displayMode: .always), prompt: prompt)
                .onSubmit(of: .search, chooseLit)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button(role: .cancel, action: close) { Image(systemName: ArkGlyphs.sf("x")) }
                            .accessibilityLabel("Cancel")
                    }
                }
        }
        .keys(step: step, close: close)
        .onChange(of: query) { _, _ in lit = listed.first?.tag }
        .task { searching = true }
        #endif
    }

    @ViewBuilder private var rows: some View {
        let groups = found
        if groups.isEmpty {
            ContentUnavailableView("No Matches", systemImage: ArkGlyphs.sf("search"))
        } else {
            ScrollViewReader { proxy in
                List {
                    ForEach(groups) { g in
                        Section {
                            ForEach(g.choices, id: \.tag) { c in line(c) }
                        } header: {
                            if !g.name.isEmpty { Text(g.name) }
                        }
                    }
                }
                .accessibilityLabel(label)
                // The keys' row kept in sight, at once: what the keys move
                // does not glide (motion.md, the frequency rule).
                .onChange(of: lit) { _, tag in if let tag { proxy.scrollTo(tag) } }
            }
        }
    }

    private func line(_ c: SearchChoiceValue) -> some View {
        let chosen = c.tag == selection
        return Button { choose(c.tag) } label: {
            HStack(spacing: Space.m) {
                Image(systemName: c.symbol).foregroundStyle(.quiet)
                Text(c.title).foregroundStyle(.ink)
                Spacer(minLength: Space.s)
                if !c.detail.isEmpty {
                    Text(c.detail).font(.subheadline).monospacedDigit().foregroundStyle(Tint(named: c.tint).color)
                }
                Image(systemName: ArkGlyphs.sf("check"))
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.accentText)
                    .opacity(chosen ? 1 : 0)
                    .accessibilityHidden(true)
            }
            .contentShape(Rectangle())
        }
        .listRowButton()
        .listRowBackground(c.tag == lit ? Color.containerHigh : nil)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(chosen ? .isSelected : [])
        .id(c.tag)
    }

    private func chooseLit() {
        if let lit, listed.contains(where: { $0.tag == lit }) { choose(lit) }
    }

    /// The lit row `by` rows on, stopping at either end.
    private func step(_ by: Int) {
        let shown = listed
        guard !shown.isEmpty else { return }
        let here = shown.firstIndex { $0.tag == lit }
        let next = here.map { min(max($0 + by, 0), shown.count - 1) } ?? 0
        lit = shown[next].tag
    }
}

extension View {
    /// A hardware keyboard's arrows move the lit row, Esc closes.
    fileprivate func keys(step: @escaping (Int) -> Void, close: @escaping () -> Void) -> some View {
        onKeyPress(.downArrow) { step(1); return .handled }
            .onKeyPress(.upArrow) { step(-1); return .handled }
            .onKeyPress(.escape) { close(); return .handled }
    }
}
