// Who is signed in, at the top of a tab: their face at the trailing end of
// the large title's own line, centred on it, as Apple's own apps have it in
// iOS 26 (the Games app's Home, on the right) — not in a bar row above the
// title, and with no glass and no button chrome: the round picture, or the
// initials on the accent, alone. What else the tab offers (a +) sits just
// before it on the same line. Tapped, it raises the account sheet rather than
// pushing Settings: who they are, the server and whether it answers, what the
// app adds (money's banks that want a look), Settings, Sign Out, and Done.
// Only a tab's root has it: a page pushed over a tab has its own bar.
//
// The line is the page's own first row (`LargeTitleRow`), which scrolls
// with it, as the system's large title does: the title full size and bold
// at the leading edge, the +s and the face at the trailing edge. The bar is
// inline and empty at rest (`largeTitleBar`); once the row has scrolled
// under it, the title comes into the bar's middle, rising in on
// `Springs.move` (`MotionTokens.rise`, transform and opacity only, as
// `inlineHeader` does) and going back the same way; under Reduce Motion it
// only fades. Not a toolbar item: a bar's items are cut short.
//
// Android's twins are arkui's `AccountFace`, `TabTitle`'s and
// `LargeTitleLine`'s `account`, and `AccountSheet` (kotlin/arkui, Account.kt),
// with the same words.
import SwiftUI

#if os(iOS)
/// The account's sizes and words, written once.
public enum AccountSizes {
    /// The face at the top of the account sheet.
    public static let sheetFace: CGFloat = 72
    /// The dot on the face while something wants a look, and how far it sits out over the face's edge.
    public static let badge: CGFloat = 9
    public static let badgeOut: CGFloat = 2
    /// How small a long large title may shrink to fit its line beside the face.
    public static let shrink: CGFloat = 0.7
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
/// and bold, at the leading edge, and `trailing` — the tab's +s, then its
/// `AccountFace` — at the trailing edge of the same line, centred on it, none
/// of them on glass. It scrolls with the page, as the system's large title
/// does; `fold` is told how tall it is, for `largeTitleBar` to bring the
/// inline title in once it has gone under the bar (none, for a row that
/// stays put). In a `List` it is a row of its own with no background, no
/// separator and no insets of its own (`padded` false where the list's
/// sections already inset their rows from the edges).
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
        .padding(.horizontal, padded ? Space.page : 0)
        .padding(.bottom, Space.s)
        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { h in fold?.wrappedValue = h }
        .listRowInsets(EdgeInsets())
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
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
    /// does the empty bar.
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
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
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
            .listStyle(.insetGrouped)
            .navigationTitle("Account")
            .navigationBarTitleDisplayMode(.inline)
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
