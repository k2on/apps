// A search field as a row of the page (`SearchField`, the name Android's
// and the desktop's have too), where a tab searches what it lists
// in place — money's Transactions, under its large title — rather than in
// the bar's drawer, which would stand over the large title and push it down
// from where every other tab has it. Drawn as the system's own field is in
// iOS 26: a capsule in the container's fill, the magnifying glass and the
// prompt quiet at its leading end, the clear button (xmark.circle.fill) at
// its trailing end only once something is typed, and the keyboard's key
// Search. A tap anywhere on the capsule puts the cursor in it, as a tap on
// the system's does. Its measures are `SearchSizes`; the colours are the
// theme's (`containerHigh`, `quiet`), so it follows light, dark and the
// accent with the rest of the page.
//
// A tab whose search is the tab bar's own (harken's Search, `Tab(role:
// .search)` with `.searchable`) keeps the system's field: this is for a
// search that is one row of a page, not a page of its own.
import SwiftUI

#if canImport(SwiftUI)
/// The search row's measures, written once.
public enum SearchSizes {
    /// The capsule's height, at the least: the system's field's, and a touch target's.
    public static let height: CGFloat = 44
    /// From the capsule's ends to the glass and the clear button.
    public static let inset: CGFloat = Space.m
    /// Between the glass, the words and the clear button.
    public static let gap: CGFloat = Space.s
}

/// A search field as a row of the page: the glass, `prompt` until something
/// is typed into `text`, and a way to empty it. `onSearch` is told when the
/// keyboard's Search key is pressed. With `autofocus` it takes the keyboard
/// as it appears (a dropdown's search). VoiceOver hears it as Search, with
/// the field and its Clear button inside.
public struct SearchField: View {
    @Binding var text: String
    let prompt: String
    let autofocus: Bool
    let onSearch: () -> Void
    @FocusState private var focused: Bool

    public init(text: Binding<String>, prompt: String, autofocus: Bool = false, onSearch: @escaping () -> Void = {}) {
        self._text = text
        self.prompt = prompt
        self.autofocus = autofocus
        self.onSearch = onSearch
    }

    public var body: some View {
        HStack(spacing: SearchSizes.gap) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(.quiet)
                .accessibilityHidden(true)
            TextField(prompt, text: $text)
                .focused($focused)
                .submitLabel(.search)
                .onSubmit(onSearch)
                .autocorrectionDisabled()
                .capitals(.never)
            if !text.isEmpty {
                Button { text = "" } label: {
                    Image(systemName: "xmark.circle.fill").foregroundStyle(.quiet)
                }
                // Its own button in a list row, not the row's; no press of
                // its own, as the system's clear button has none.
                .buttonStyle(.borderless)
                .accessibilityLabel("Clear")
            }
        }
        .padding(.horizontal, SearchSizes.inset)
        .frame(minHeight: SearchSizes.height)
        .background(Color.containerHigh, in: Capsule())
        .contentShape(Capsule())
        // The glass and the capsule's ends put the cursor in the field, as
        // the system's do: focusing a field, not a press, so no press feedback.
        .onTapGesture { focused = true }
        .task { if autofocus { focused = true } }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(prompt)
    }
}
#endif
