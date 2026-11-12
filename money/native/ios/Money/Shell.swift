// money, once the first run has handed it a session: the shell written
// from Rust (Generated/MoneyShellView.swift, ui/docs/apps/money.md) over the
// runtimes money's host made (MoneyApp.swift's `MoneyScreens`) — the
// sections Budget, Accounts (badged with what needs a look), Transactions
// and Insights, a tab bar on the iPhone and a sidebar on the iPad and the
// Mac, each its toolbar, search and inspector — hosting money's own
// screens by name, each section its own back stack so a switch finds it as
// it was left; the sheets a screen raises, each with its detents, one at a
// time; Plaid's page in Safari's view when a bank is being connected; and
// what the app says once, on glass at the top. This file wires; what each
// screen draws is the screen's.
import ArkUI
import SwiftUI

private struct OpenKey: EnvironmentKey {
    static let defaultValue: (Route) -> Void = { _ in }
}

extension EnvironmentValues {
    /// Navigate to a route on the tab's own back stack: what the account sheet's rows open, once it has gone.
    var open: (Route) -> Void {
        get { self[OpenKey.self] }
        set { self[OpenKey.self] = newValue }
    }
}

struct Shell: View {
    @EnvironmentObject private var app: ArkApp<MoneyHost>
    @EnvironmentObject private var screens: MoneyScreens
    /// Each section's back stack, by its tag, of money's routes.
    @State private var paths: [String: NavigationPath] = [:]

    private func path(_ tag: String) -> Binding<NavigationPath> {
        Binding(get: { paths[tag] ?? NavigationPath() }, set: { paths[tag] = $0 })
    }

    /// A page on the stack of the section showing (or of `tag`): an
    /// account's register, All Accounts, Connected Banks, a bank. A
    /// category's month is the shell's inspector, opened in Rust.
    private func push(_ r: Route, on tag: String? = nil) {
        let t = tag ?? String(screens.shell.page.section.split(separator: "/").first ?? "budget")
        paths[t, default: NavigationPath()].append(r)
    }

    var body: some View {
        // Written from Rust (Generated/MoneyShellView.swift), over its
        // runtime: the sections, their toolbars, search and inspectors.
        MoneyShellView()
            .environmentObject(screens.shell)
            .environment(\.hostedScreens) { name in AnyView(hosted(name)) }
            .environment(\.sectionPath) { tag in path(tag) }
            // The sheets any screen raises, written from Rust (Generated/SheetsView.swift).
            .background {
                if let s = screens.sheets { SheetsView().environmentObject(s) }
            }
            .onAppear {
                // What money's host asks of this shell: a page pushed; a link
                // taken as a launch puts away what this shows of its own.
                screens.push = { r in push(r) }
                screens.reset = { paths = [:] }
            }
            // The launch's `-appearance` or Settings' Light or Dark, over the
            // device's: the shell's. None (System, the default) asks for
            // nothing at all, so the window follows the device: a
            // `preferredColorScheme(nil)` here left app-screens' dark iPhone
            // and iPad passes drawing light (43131582873).
            .modifier(Appearing(appearance: screens.shell.page.appearance))
            .settingsShown($screens.settings) {
                MoneySettingsView()
                    .moneySettings(app, screens)
            }
            // Plaid's page, as soon as the server answers with it, wherever the
            // person is; closed by its Done, or once the server is finished
            // (money_app's shell says which: `SettingsPage.plaid`).
            .safari(screens.shell.page.settings.plaid.flatMap { URL(string: $0) }, tint: Color.accent) {
                if let url = screens.shell.page.settings.plaid { screens.shell.send(.pageClosed(url: url)) }
            }
    }

    /// A screen the shell hosts (ark_app::shell::hosted): money's runtimes'
    /// generated views; each opens what it opens on its section's stack.
    @ViewBuilder private func hosted(_ name: String) -> some View {
        let section = String(name.split(separator: ".").first ?? "")
        SwiftUI.Group {
            switch name {
            case "budget":
                if let b = screens.budget {
                    // Written from Rust (Generated/BudgetView.swift), over its runtime.
                    BudgetView()
                        .environmentObject(b)
                        .environment(\.rowMark) { kind in AnyView(AccountMark(kind: kind, side: 20)) }
                }
            case "accounts":
                if let a = screens.accounts {
                    // Written from Rust (Generated/AccountsView.swift), over its runtime.
                    AccountsView()
                        .environmentObject(a)
                        .environment(\.rowMark) { kind in AnyView(AccountMark(kind: kind)) }
                        .environment(\.choiceSymbol) { kind, _ in Accounts.symbol(kind) }
                }
            case "transactions":
                // Written from Rust (Generated/TransactionsView.swift), over its
                // runtime; the shell's search is its search (money's host).
                if let t = screens.transactions { TransactionsView().environmentObject(t) }
            case "insights":
                // Written from Rust (Generated/InsightsView.swift), over its runtime.
                if let i = screens.insights { InsightsView().environmentObject(i) }
            case "accounts.register":
                // The account the shell shows (its place's tag, `accounts/<id>`), under its search field.
                if screens.shell.page.section.hasPrefix("accounts/") {
                    register(String(screens.shell.page.section.dropFirst("accounts/".count)), search: screens.shell.page.search)
                } else {
                    ContentUnavailableView("No Account Selected", systemImage: "building.columns")
                }
            case "budget.inspector":
                nothingSelected
            case "transactions.inspector":
                nothingSelected
            case "budget.inspector.none":
                ContentUnavailableView("Nothing Selected", systemImage: "sidebar.trailing", description: Text("Select a row to see its details here."))
            case "household": AccountButton()
            default: ContentUnavailableView("Nothing Selected", systemImage: "sidebar.right")
            }
        }
        .navigationDestination(for: Route.self) { r in destination(r) }
        .environment(\.open) { r in push(r, on: section) }
    }

    private var nothingSelected: some View {
        ContentUnavailableView("Nothing Selected", systemImage: "sidebar.trailing", description: Text("Select a row to see its details here."))
    }

    /// A register, written from Rust (Generated/RegisterView.swift): the
    /// account of this hex id's, or every account's for none.
    @ViewBuilder private func register(_ account: String?, search: String) -> some View {
        if screens.accounts != nil {
            RegisterHost(screens: screens, account: account, search: search)
                .id(account ?? "all")
        }
    }

    /// What a route on the back stack draws. An account is found by the
    /// route's id, so a renamed one keeps its page; a bank by its name, its
    /// key. An account that is gone is the route's fallback, All Accounts.
    @ViewBuilder private func destination(_ r: Route) -> some View {
        switch r {
        case .accounts: register(nil, search: "")
        case let .account(id, _): register(id, search: "")
        // Connected Banks, written from Rust (Generated/MoneyBanksView.swift).
        case .banks:
            MoneyBanksView().environmentObject(screens.shell).environment(\.rowMark) { kind in AnyView(AccountMark(kind: kind)) }
        case let .bank(name):
            MoneyBanksView().environmentObject(screens.shell).environment(\.rowMark) { kind in AnyView(AccountMark(kind: kind)) }
                .onAppear { screens.shell.send(.openBank(name: name)) }
        default: EmptyView()
        }
    }
}

/// The shell's appearance over the device's, only while it names one.
private struct Appearing: ViewModifier {
    let appearance: Appearance?

    func body(content: Content) -> some View {
        switch appearance {
        case .dark?: content.preferredColorScheme(.dark)
        case .light?: content.preferredColorScheme(.light)
        case nil: content
        }
    }
}

/// The account at the end of every tab's large title (ArkUI's `AccountFace`,
/// on the title's own line by `largeTitleBar`): who is using the budget, as
/// their face, marked while a bank wants somebody there, raising the account
/// sheet — who and where they are, the banks that want a look, Settings, and
/// the way out. What its rows open is pushed on the tab once it has gone.
struct AccountButton: View {
    @EnvironmentObject private var app: ArkApp<MoneyHost>
    @EnvironmentObject private var screens: MoneyScreens
    @Environment(\.open) private var open
    @State private var showing = false
    /// What the sheet's row asked for, done once the sheet has gone.
    @State private var then: Then? = nil

    private enum Then { case page(Route), settings, signOut }

    var body: some View {
        let h = screens.shell.page.household
        let wanting = Int(h.banksWanting)
        AccountFace(name: h.who, picture: h.picture, attention: wanting > 0 ? "Banks need a look" : nil) { showing = true }
            .sheet(isPresented: $showing, onDismiss: after) {
                AccountSheet(name: h.who, picture: h.picture, detail: h.detail, server: h.server, linked: h.linked,
                             signOut: h.demo ? "Leave the Demo" : "Sign Out",
                             confirm: h.demo ? "Leave the Demo?" : "Sign Out?",
                             onSettings: { leave(.settings) },
                             onSignOut: { leave(.signOut) }) {
                    if wanting > 0 {
                        Section {
                            Button { leave(.page(.banks)) } label: {
                                HStack {
                                    Label {
                                        Text(wanting == 1 ? "A Bank Needs a Look" : "\(wanting) Banks Need a Look")
                                    } icon: {
                                        Image(systemName: "exclamationmark.circle.fill").foregroundStyle(Color.error)
                                    }
                                    Spacer()
                                    Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.quiet)
                                }
                                .contentShape(Rectangle())
                            }
                            .foregroundStyle(.ink)
                        }
                    }
                }
                .presentationDetents([.medium, .large])
            }
    }

    private func leave(_ t: Then) {
        then = t
        showing = false
    }

    private func after() {
        let t = then
        then = nil
        switch t {
        case .settings?: screens.settings = true
        case let .page(r)?: open(r)
        case .signOut?: app.signOut()
        case nil: break
        }
    }
}

/// A register over its runtime: its own for as long as it shows, read
/// again as the household moves, searched by the shell's field, its row the
/// one the shell's inspector shows.
struct RegisterHost: View {
    @StateObject private var held: Held
    @ObservedObject private var screens: MoneyScreens
    let search: String

    /// The register's runtime, made once for the view's life.
    @MainActor final class Held: ObservableObject {
        let screen: RegisterScreen?
        init(_ screen: RegisterScreen?) { self.screen = screen }
    }

    init(screens: MoneyScreens, account: String?, search: String) {
        _held = StateObject(wrappedValue: Held(screens.register(account)))
        self.screens = screens
        self.search = search
    }

    var body: some View {
        if let screen = held.screen {
            RegisterView()
                .environmentObject(screen)
                .onChange(of: screens.moves) { screen.reload() }
                .onChange(of: search, initial: true) { _, q in screen.send(.search(text: q)) }
                .onChange(of: screens.shell.page.transaction?.id, initial: true) { _, id in screen.send(.select(id: id)) }
        }
    }
}

/// The Settings scene (Generated/MoneySettingsView.swift) as money shows
/// it: over the shell's runtime, accounts drawn with their marks, nothing
/// of its own in General (only the desktop adds there), and Add a Device's
/// shared page when the shell asks for it.
private struct MoneySettings: ViewModifier {
    @ObservedObject var app: ArkApp<MoneyHost>
    @ObservedObject var screens: MoneyScreens

    func body(content: Content) -> some View {
        content
            .environmentObject(screens.shell)
            .environmentObject(app)
            .environmentObject(screens)
            .environment(\.rowMark) { kind in AnyView(AccountMark(kind: kind)) }
            .environment(\.hostedScreens) { _ in AnyView(EmptyView()) }
            .sheet(isPresented: $screens.addingDevice) {
                let app = app
                NavigationStack {
                    AddDeviceView { await app.deviceLink() }
                        .toolbar { Button("Done") { screens.addingDevice = false } }
                }
            }
    }
}

extension View {
    func moneySettings(_ app: ArkApp<MoneyHost>, _ screens: MoneyScreens) -> some View { modifier(MoneySettings(app: app, screens: screens)) }
}
