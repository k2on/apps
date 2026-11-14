// Who is signed in, at the top of a tab: their face at the trailing end of
// the large title's own line, centred on it, as Apple's own apps have it in
// iOS 26 (the Games app's Home, on the right) — not in a bar row above the
// title, and with no glass and no button chrome: the round picture, or the
// initials on the accent, alone. A menu the tab offers (Budget's •••) may sit
// just before it on the same line; a + does not — what a tab makes is a row
// of its own page. Tapped, it raises the account sheet rather than
// pushing Settings: who they are, the server and whether it answers, what the
// app adds (money's banks that want a look), Settings, Sign Out, and Done.
// Only a tab's root has it: a page pushed over a tab has its own bar.
//
// The line is the top of the page (`LargeTitleRow`; in a `List`, the header
// of its first section, `TitleSection`, so an inset grouped list's rounded
// cards never clip it), which scrolls with it, as the system's large title
// does: the title full size and bold
// at the leading edge, the face at the trailing edge. It is one row for
// every tab of every app — one top inset, one height, one font
// (`AccountSizes.titleTop`, `titleLine`, `titleBottom`, `.largeTitle.bold()`),
// and one gap under it to the page's first content (`titleBelow`)
// — and `largeTitleBar` takes away whatever top margin the scroll view
// under it would add (an inset grouped list's), so the title's baseline and
// the face are in the same place on every tab; an app adds none. The bar is
// inline and empty at rest (`largeTitleBar`); once the row has scrolled
// under it, the title comes into the bar's middle, rising in on
// `Springs.move` (`MotionTokens.rise`, transform and opacity only, as
// `inlineHeader` does) and going back the same way; under Reduce Motion it
// only fades. Not a toolbar item: a bar's items are cut short.
//
// Android's twins are arkui's `AccountFace`, `TabTitle`'s `account` (the
// same one row there), and `AccountSheet` (ui/kotlin, Account.kt), with
// the same words.
import SwiftUI

#if canImport(SwiftUI)
/// The account's sizes and words, written once.
public enum AccountSizes {
    /// The face at the top of the account sheet.
    public static let sheetFace: CGFloat = 72
    /// The dot on the face while something wants a look, and how far it sits out over the face's edge.
    public static let badge: CGFloat = 9
    public static let badgeOut: CGFloat = 2
    /// How small a long large title may shrink to fit its line beside the face.
    public static let shrink: CGFloat = 0.7
    /// The large title row (`LargeTitleRow`), written once for every tab: from
    /// the bar's foot to its line, the line the title and the face are centred
    /// on, and under it before the page's first row.
    public static let titleTop: CGFloat = 0
    public static let titleLine: CGFloat = 44
    public static let titleBottom: CGFloat = Space.s
    /// From the row's end to the page's first content, on every tab of
    /// every app, Android's too (`TabTitle.BELOW`): a section's own top
    /// margin, the one a heading keeps above itself, so a card, a field, a
    /// pager or a row of chips under the title sits where harken Home's
    /// "Recently Added" does. A heading that comes first has it already;
    /// anything else that comes first is padded by it, in the title's own
    /// header in a `List` (`TitleSection`, `HeaderRows`), so the list's
    /// section spacing does not add to it.
    public static let titleBelow: CGFloat = Space.xl
    /// Between the rows under the title in a `TitleSection` (a search field
    /// over a segmented control), unless the tab says otherwise.
    public static let belowGap: CGFloat = Space.m
    /// What a screen reader hears of the face, and of what its tap does.
    public static func label(_ name: String) -> String { "Signed in as \(name.isEmpty ? "?" : name)" }
    public static let hint = "Account and settings"
}

/// Who is signed in, for the end of a tab's large title (`LargeTitleRow`):
/// their `Face`, badged in the error colour while `attention` says what
/// wants a look (money's banks), raising the account sheet on a tap
/// (`action`). VoiceOver hears "Signed in as …", `attention`, and that it
/// is the account and settings.
public struct AccountFace: View {
    let name: String
    let picture: String
    let attention: String?
    let action: () -> Void

    public init(name: String, picture: String, attention: String? = nil, action: @escaping () -> Void) {
        self.name = name
        self.picture = picture
        self.attention = attention
        self.action = action
    }

    public var body: some View {
        let who = name.isEmpty ? "?" : name
        Button(action: action) {
            Face(name: who, picture: picture)
                .overlay(alignment: .topTrailing) {
                    if attention != nil {
                        Circle().fill(Color.error)
                            .frame(width: AccountSizes.badge, height: AccountSizes.badge)
                            .offset(x: AccountSizes.badgeOut, y: -AccountSizes.badgeOut)
                            .motionTransition(.enter)
                    }
                }
                .motion(Springs.move, value: attention != nil)
                .contentShape(Circle())
        }
        .buttonStyle(.press)
        .accessibilityLabel(AccountSizes.label(name))
        .accessibilityHint(AccountSizes.hint)
        .accessibilityValue(attention ?? "")
    }
}

/// A tab's large title as the first row of its page: the title, full size
/// and bold, at the leading edge, and `trailing` — a menu where the tab has
/// one, then its `AccountFace`; never a + — at the trailing edge of the same
/// line, centred on it, none of them on glass. One inset, one height and one
/// font for every tab (`AccountSizes.titleTop`, `titleLine`, `titleBottom`),
/// so it is drawn alike wherever it is. It scrolls with the page, as the
/// system's large title does; `fold` is told how tall it is, for
/// `largeTitleBar` to bring the inline title in once it has gone under the
/// bar (none, for a row that stays put). In a `List` it is not a row but the
/// header of the page's first section (`TitleSection` puts it there, with
/// `padded` false, as the list already insets its headers from the edges by
/// the same 16 pt): an inset grouped list clips each section's rows to its
/// rounded card, and those corners would cut the title's first letter and
/// the face's edge.
@available(iOS 26.0, *)
public struct LargeTitleRow<Trailing: View>: View {
    let title: String
    let fold: Binding<CGFloat>?
    let padded: Bool
    let trailing: () -> Trailing

    public init(_ title: String, fold: Binding<CGFloat>? = nil, padded: Bool = true,
                @ViewBuilder trailing: @escaping () -> Trailing) {
        self.title = title
        self.fold = fold
        self.padded = padded
        self.trailing = trailing
    }

    public var body: some View {
        HStack(alignment: .center, spacing: Space.l) {
            Text(title)
                .font(.largeTitle.bold())
                .foregroundStyle(.ink)
                .lineLimit(1)
                .minimumScaleFactor(AccountSizes.shrink)
                .accessibilityAddTraits(.isHeader)
                .layoutPriority(1)
            Spacer(minLength: Space.s)
            HStack(spacing: Space.l) { trailing() }
                .imageScale(.large)
                // A row's buttons, each its own: a list row otherwise fires every one of them.
                .buttonStyle(.borderless)
        }
        .frame(height: AccountSizes.titleLine)
        .padding(.horizontal, padded ? Space.page : 0)
        .padding(.top, AccountSizes.titleTop)
        .padding(.bottom, AccountSizes.titleBottom)
        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { h in fold?.wrappedValue = h }
        .listRowInsets(EdgeInsets())
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }
}

/// A tab's large title at the top of a `List`, whatever its style: the
/// title's line (`LargeTitleRow`) and `below` — what comes first under it:
/// a pager, a card, a search field (`SearchField`) over a segmented control —
/// as the header of the page's first section, with `rows` its rows (most
/// often none). Not rows: an inset grouped list clips each section's rows
/// to its rounded card, and those corners cut the title's first letter, the
/// face's edge and a control's ends; a header is not clipped so. The header
/// keeps none of a header's own look (no capitals, no insets, no quiet small
/// type), so the title is drawn as it is over a `ScrollView` (harken's): its
/// baseline and the face where every tab has them, 16 pt from the edge, and
/// `below` one `AccountSizes.titleBelow` under it (`HeaderRows`).
@available(iOS 26.0, *)
public struct TitleSection<Trailing: View, Below: View, Rows: View>: View {
    let title: String
    let fold: Binding<CGFloat>?
    let spacing: CGFloat
    let trailing: () -> Trailing
    let below: () -> Below
    let rows: () -> Rows

    public init(_ title: String, fold: Binding<CGFloat>? = nil, spacing: CGFloat = AccountSizes.belowGap,
                @ViewBuilder trailing: @escaping () -> Trailing,
                @ViewBuilder below: @escaping () -> Below,
                @ViewBuilder rows: @escaping () -> Rows) {
        self.title = title
        self.fold = fold
        self.spacing = spacing
        self.trailing = trailing
        self.below = below
        self.rows = rows
    }

    public var body: some View {
        Section {
            rows()
        } header: {
            VStack(spacing: 0) {
                LargeTitleRow(title, fold: fold, padded: false, trailing: trailing)
                HeaderRows(spacing: spacing, content: below)
            }
            // None of a header's own look: what is under the title is drawn as a row is.
            .textCase(nil)
            .font(.body)
            .foregroundStyle(.ink)
            .frame(maxWidth: .infinity, alignment: .leading)
            .listRowInsets(EdgeInsets())
        }
    }
}

@available(iOS 26.0, *)
extension TitleSection where Rows == EmptyView {
    /// The title and what is under it, with no rows of the section's own.
    public init(_ title: String, fold: Binding<CGFloat>? = nil, spacing: CGFloat = AccountSizes.belowGap,
                @ViewBuilder trailing: @escaping () -> Trailing,
                @ViewBuilder below: @escaping () -> Below) {
        self.init(title, fold: fold, spacing: spacing, trailing: trailing, below: below) { EmptyView() }
    }
}

/// What a tab has first under its large title, in a `TitleSection`'s
/// header: its rows `spacing` apart (`AccountSizes.belowGap`), the whole one
/// `AccountSizes.titleBelow` under the title, across the list's width (16 pt
/// in from the edges, as a row is), and never clipped to a row's card.
@available(iOS 26.0, *)
public struct HeaderRows<Content: View>: View {
    let spacing: CGFloat
    let content: () -> Content

    public init(spacing: CGFloat = AccountSizes.belowGap, @ViewBuilder content: @escaping () -> Content) {
        self.spacing = spacing
        self.content = content
    }

    public var body: some View {
        VStack(spacing: spacing) { content() }
            .frame(maxWidth: .infinity)
            .padding(.top, AccountSizes.titleBelow)
    }
}

@available(iOS 26.0, *)
extension View {
    /// On a tab's root (its scroll view): `title` as its navigation title,
    /// the bar inline and, at rest, empty — the large title is the page's
    /// own first row (`LargeTitleRow`) — and once the page has scrolled
    /// `fold` (the row's height) under the bar, the title small in the bar's
    /// middle, rising in on `Springs.move` (a fade under Reduce Motion) and
    /// going back the same way. With no `fold` the row stays put, and so
    /// does the empty bar. With a `fold` the row is the scroll view's first,
    /// so the scroll view's own top margin goes (an inset grouped list's
    /// 35 pt): the row is at the bar's foot in a `List` as in a `ScrollView`.
    public func largeTitleBar(_ title: String, fold: CGFloat? = nil) -> some View {
        modifier(LargeTitleBar(title: title, fold: fold))
    }
}

@available(iOS 26.0, *)
private struct LargeTitleBar: ViewModifier {
    let title: String
    let fold: CGFloat?
    @State private var under = false
    @Environment(\.reducesMotion) private var still

    func body(content: Content) -> some View {
        content
            .contentMargins(.top, fold == nil ? nil : CGFloat(0), for: .scrollContent)
            .navigationTitle(title)
            .inlineTitle()
            .onScrollGeometryChange(for: Bool.self) { g in
                guard let fold, fold > 0 else { return false }
                return g.contentOffset.y + g.contentInsets.top >= fold
            } action: { _, now in
                under = now
            }
            .toolbar {
                // Always there, so the system's own inline title never is; seen
                // only once the large title has gone under the bar.
                ToolbarItem(placement: .principal) {
                    Text(title)
                        .font(.headline)
                        .foregroundStyle(.ink)
                        .lineLimit(1)
                        .opacity(under ? 1 : 0)
                        .offset(y: under || still ? 0 : MotionTokens.rise)
                        .motion(Springs.move, value: under)
                        .accessibilityHidden(!under)
                        .accessibilityAddTraits(.isHeader)
                }
                .sharedBackgroundVisibility(.hidden)
            }
    }
}

/// The account sheet the face raises, in its own navigation stack: their
/// face large, their name and `detail` (an email, a user name, or what the
/// demo is); the `server` (its host; none for the demo) and whether it is
/// `linked` (nil while it is still being asked); what the app adds (`rows`,
/// sections of a list); Settings; `signOut`, confirmed first when `confirm`
/// says what to ask; and Done. Settings and Sign Out are the app's to do
/// (`onSettings`, `onSignOut`), after it has put the sheet away.
public struct AccountSheet<Rows: View>: View {
    let name: String
    let picture: String
    let detail: String
    let server: String?
    let linked: Bool?
    let signOut: String
    let confirm: String?
    let onSettings: () -> Void
    let onSignOut: () -> Void
    let rows: () -> Rows
    @Environment(\.dismiss) private var dismiss
    @State private var asking = false

    public init(name: String, picture: String, detail: String, server: String?, linked: Bool?,
                signOut: String = "Sign Out", confirm: String? = nil,
                onSettings: @escaping () -> Void, onSignOut: @escaping () -> Void,
                @ViewBuilder rows: @escaping () -> Rows) {
        self.name = name
        self.picture = picture
        self.detail = detail
        self.server = server
        self.linked = linked
        self.signOut = signOut
        self.confirm = confirm
        self.onSettings = onSettings
        self.onSignOut = onSignOut
        self.rows = rows
    }

    public var body: some View {
        NavigationStack {
            List {
                Section {
                    VStack(spacing: Space.s) {
                        Face(name: name.isEmpty ? "?" : name, picture: picture, side: AccountSizes.sheetFace)
                            .padding(.bottom, Space.xs)
                        Text(name.isEmpty ? "Signed In" : name).font(.title2.bold()).foregroundStyle(.ink)
                        if !detail.isEmpty {
                            Text(detail).foregroundStyle(.quiet)
                        }
                    }
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: .infinity)
                    .listRowBackground(Color.clear)
                    .accessibilityElement(children: .combine)
                    .accessibilityAddTraits(.isHeader)
                }
                if let server {
                    Section {
                        ServerLine(host: server, linked: linked)
                    }
                }
                rows()
                Section {
                    Button(action: onSettings) {
                        HStack {
                            Label("Settings", systemImage: "gearshape")
                            Spacer()
                            Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.quiet)
                        }
                        .contentShape(Rectangle())
                    }
                    .foregroundStyle(.ink)
                }
                Section {
                    Button(role: .destructive) {
                        if confirm != nil { asking = true } else { onSignOut() }
                    } label: {
                        Label(signOut, systemImage: "rectangle.portrait.and.arrow.right")
                    }
                }
            }
            .groupedList()
            .navigationTitle("Account")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .confirmationDialog(confirm ?? "", isPresented: $asking, titleVisibility: .visible) {
                Button(signOut, role: .destructive) { onSignOut() }
            }
        }
    }
}

extension AccountSheet where Rows == EmptyView {
    /// The sheet with nothing of the app's own in it.
    public init(name: String, picture: String, detail: String, server: String?, linked: Bool?,
                signOut: String = "Sign Out", confirm: String? = nil,
                onSettings: @escaping () -> Void, onSignOut: @escaping () -> Void) {
        self.init(name: name, picture: picture, detail: detail, server: server, linked: linked,
                  signOut: signOut, confirm: confirm, onSettings: onSettings, onSignOut: onSignOut) { EmptyView() }
    }
}

/// The server, and whether it answers: "Connected to …", or that it cannot
/// be reached right now and what is on the phone still works.
private struct ServerLine: View {
    let host: String
    let linked: Bool?

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: Space.xs / 2) {
                switch linked {
                case true?: Text("Connected to \(host)").foregroundStyle(.ink)
                case false?:
                    Text("Can't Reach \(host)").foregroundStyle(.ink)
                    Text("What's on this phone still works").font(.footnote).foregroundStyle(.quiet)
                case nil: Text("Connecting to \(host)…").foregroundStyle(.ink)
                }
            }
        } icon: {
            Image(systemName: linked == false ? "icloud.slash" : "checkmark.icloud")
                .foregroundStyle(linked == false ? Color.error : Color.accent)
        }
        .accessibilityElement(children: .combine)
    }
}
#endif
