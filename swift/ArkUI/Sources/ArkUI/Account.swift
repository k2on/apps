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
// The line is the navigation bar's own, in its inline mode: the title, large
// and bold, at its leading end, and the face and the +s at its trailing end,
// none of them on the bar's glass (`sharedBackgroundVisibility(.hidden)`).
// As the page scrolls under the bar, the large title hands over to the inline
// one in the bar's middle — it rises away and the small one rises in, on
// `Springs.move`, transform and opacity only, as `inlineHeader` does — and
// back over the top it comes back. Under Reduce Motion they only fade. The
// face and the +s stay where they are, as a bar's items do.
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
    /// How far a page scrolls under the bar before its large title hands over to the inline one.
    public static let fold: CGFloat = 12
    /// How small a long large title may shrink to fit its line beside the face.
    public static let shrink: CGFloat = 0.7
    /// What a screen reader hears of the face, and of what its tap does.
    public static func label(_ name: String) -> String { "Signed in as \(name.isEmpty ? "?" : name)" }
    public static let hint = "Account and settings"
}

/// Who is signed in, for the end of a tab's large title (`largeTitleBar`):
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

@available(iOS 26.0, *)
extension View {
    /// On a tab's root (its scroll view, or what holds it): `title` as the
    /// large title, on the navigation bar's own line, with `trailing` — the
    /// tab's +s, then its `AccountFace` — at that line's other end, none of
    /// them on glass; and, once the page has scrolled under the bar, the
    /// title small in the bar's middle instead. Sets the page's navigation
    /// title, so a page pushed over it says where back goes.
    public func largeTitleBar<Trailing: View>(_ title: String, @ViewBuilder trailing: @escaping () -> Trailing) -> some View {
        modifier(LargeTitleBar(title: title, trailing: trailing))
    }
}

@available(iOS 26.0, *)
private struct LargeTitleBar<Trailing: View>: ViewModifier {
    let title: String
    let trailing: () -> Trailing
    @State private var under = false
    @Environment(\.reducesMotion) private var still

    func body(content: Content) -> some View {
        content
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .onScrollGeometryChange(for: Bool.self) { g in
                g.contentOffset.y + g.contentInsets.top > AccountSizes.fold
            } action: { _, now in
                withAnimation(Springs.move.reduced(still)) { under = now }
            }
            .toolbar {
                // The large title, at the line's leading end while the page is at its top.
                // Gone, rather than only faded, once it has handed over: the inline title
                // in the middle then has the line to itself.
                ToolbarItem(placement: .topBarLeading) {
                    if !under {
                        Text(title)
                            .font(.largeTitle.bold())
                            .foregroundStyle(.ink)
                            .lineLimit(1)
                            .minimumScaleFactor(AccountSizes.shrink)
                            .dynamicTypeSize(...DynamicTypeSize.xLarge)
                            .accessibilityAddTraits(.isHeader)
                            .motionTransition(.opacity.combined(with: .offset(y: -MotionTokens.rise)))
                    }
                }
                .sharedBackgroundVisibility(.hidden)
                ToolbarItem(placement: .principal) {
                    if under {
                        Text(title)
                            .font(.headline)
                            .foregroundStyle(.ink)
                            .lineLimit(1)
                            .accessibilityAddTraits(.isHeader)
                            .motionTransition(.opacity.combined(with: .offset(y: MotionTokens.rise)))
                    }
                }
                .sharedBackgroundVisibility(.hidden)
                ToolbarItem(placement: .topBarTrailing) {
                    HStack(spacing: Space.l) { trailing() }
                        .imageScale(.large)
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
