// money, once the first run has handed it a session: the shell written
// from Rust (Generated/MoneyShellView.swift, ui/docs/apps/money.md) — the
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
    @EnvironmentObject private var model: Model
    @EnvironmentObject private var watched: Watched<MoneyView>
    /// Each section's back stack, by its tag, of money's routes.
    @State private var paths: [String: NavigationPath] = [:]
    /// What the launch asked for that the household had not yet synced:
    /// tried again as the snapshot moves.
    @State private var pending: [MoneyRequest] = []
    /// Settings asked for by the launch: the Mac's window, a sheet elsewhere.
    @State private var settings = false

    private func path(_ tag: String) -> Binding<NavigationPath> {
        Binding(get: { paths[tag] ?? NavigationPath() }, set: { paths[tag] = $0 })
    }

    private func push(_ tag: String, _ r: Route) {
        // A category's month is Category Details, the shell's inspector
        // (written from Rust): beside the budget when regular, a sheet when
        // compact. Never a page of this app's own.
        if case let .category(id, _) = r { return inspect(category: id) }
        paths[tag, default: NavigationPath()].append(r)
    }

    /// Category Details shown for the category of this hex id.
    private func inspect(category id: String) {
        model.shell.send(.select(tag: "budget"))
        model.shell.send(.inspectCategory(id: id))
        model.shell.send(.inspector(shown: true))
    }

    var body: some View {
        // Written from Rust (Generated/MoneyShellView.swift), over its
        // runtime: the sections, their toolbars, search and inspectors.
        MoneyShellView()
            .environmentObject(model.shell)
            .environment(\.hostedScreens) { name in
                // A transaction's receipt, in its sheet: this app's own until receipts move into Rust.
                name == "transaction.receipt" ? AnyView(TransactionReceipt()) : AnyView(hosted(name))
            }
            .environment(\.sectionPath) { tag in path(tag) }
            // The sheets any screen raises, written from Rust (Generated/SheetsView.swift).
            .background {
                if let s = model.sheets { SheetsView().environmentObject(s) }
            }
            .onAppear {
                model.shell.request = { r in run(r) }
                model.shell.send(.month(month: model.month))
                // Where the window opens (ark_app::shell::Launch): once, for the process.
                if !Launched.done {
                    Launched.done = true
                    model.shell.send(.launch(args: ProcessInfo.processInfo.arguments))
                }
            }
            .onChange(of: model.snapshot.moved) {
                let waiting = pending
                pending = []
                for r in waiting { run(r) }
            }
            // The launch's `-appearance`, over the system's: the shell's.
            .preferredColorScheme(model.shell.page.appearance.map { $0 == .dark ? .dark : .light })
            .settingsShown($settings) {
                MoneySettingsView()
                    .environmentObject(model.shell)
                    .environmentObject(model)
                    .environmentObject(watched)
                    .environment(\.hostedScreens) { name in AnyView(SettingsHosted(name: name)) }
            }
            // The budget's title is its month: the shell is told as it moves.
            .onChange(of: model.month) { model.shell.send(.month(month: model.month)) }
            .toast($model.notice)
            // A link for the app, kept by the first run until the app showed:
            // taken as the shell appears, and as another comes.
            .onAppear { takeLink() }
            .onChange(of: model.links) { takeLink() }
            // Plaid's page, as soon as the server answers with it, wherever the
            // person is; closed by its Done, or once the server is finished.
            .safari(model.bankPage, tint: Color.accent) { model.bankPageClosed() }
    }

    /// The link the first run kept, taken. A launch link (`money://?screen=…`)
    /// is a launch as the app runs: what this shows of its own put away (a
    /// sheet, a section's pages, a transaction inspected, Settings), then the
    /// shell told as a launch tells it, which puts back what it showed itself
    /// (ark_app's Launch). Any other is its route, pushed on the section shown.
    private func takeLink() {
        guard let url = model.takeLink() else { return }
        Ready.shared.changed()
        if let args = Navigator<Route>.launchArgs(of: url, scheme: AppInfo.money.scheme) {
            model.sheets?.send(.dismiss)
            paths = [:]
            settings = false
            pending = []
            // The inspector put away first, while the section it is in is
            // still the one shown, and the launch only on the next turn: on
            // the iPhone it is a sheet of that section's tab, and a tab
            // switched away from in the same update never dismisses it
            // (dry run 43083062235: Category Details, "Nothing Selected",
            // over 03-accounts, 04-account-visa and 05-review). The screen is
            // not ready meanwhile.
            Ready.shared.begin("launch link")
            model.shell.send(.inspector(shown: false))
            Task { @MainActor in
                model.shell.send(.launch(args: args))
                Ready.shared.end("launch link")
            }
            return
        }
        let tag = model.shell.page.section
        var nav = Navigator<Route>()
        if nav.handle(url, scheme: AppInfo.money.scheme, parse: { routeFromUrl(url: $0) }, take: model.takes) {
            for r in nav.path { push(tag, r) }
        }
    }

    /// A screen the shell hosts (ark_app::shell::hosted): money's own,
    /// until each moves into Rust; each opens what it opens on its
    /// section's stack.
    @ViewBuilder private func hosted(_ name: String) -> some View {
        let section = String(name.split(separator: ".").first ?? "")
        SwiftUI.Group {
            switch name {
            case "budget":
                if let b = model.budget {
                    // Written from Rust (Generated/BudgetView.swift), over its runtime.
                    BudgetView()
                        .environmentObject(b)
                        .environment(\.rowMark) { kind in AnyView(AccountMark(kind: kind, side: 20)) }
                        .onAppear {
                            // A category's month: Category Details, the shell's inspector.
                            b.open = { r in push("budget", r) }
                        }
                }
            case "accounts":
                if let a = model.accounts {
                    // Written from Rust (Generated/AccountsView.swift), over its runtime.
                    AccountsView()
                        .environmentObject(a)
                        .environment(\.rowMark) { kind in AnyView(AccountMark(kind: kind)) }
                        .environment(\.choiceSymbol) { kind, _ in Accounts.symbol(kind) }
                        .onAppear {
                            a.open = { r in push("accounts", r) }
                            a.request = { r in run(r) }
                        }
                }
            case "transactions":
                if let t = model.transactions {
                    // Written from Rust (Generated/TransactionsView.swift), over its runtime.
                    TransactionsView()
                        .environmentObject(t)
                        .onAppear { t.request = { r in run(r) } }
                        .onChange(of: model.shell.page.search, initial: true) { _, q in t.send(.search(text: q)) }
                }
            case "insights":
                // Written from Rust (Generated/InsightsView.swift), over its runtime.
                if let i = model.insights { InsightsView().environmentObject(i) }
            case "accounts.register":
                // The account the shell shows (its place's tag, `accounts/<id>`), under its search field.
                if model.shell.page.section.hasPrefix("accounts/") {
                    register(String(model.shell.page.section.dropFirst("accounts/".count)), search: model.shell.page.search)
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
            case "settings.general": SettingsView()
            case "settings.banks": BanksView()
            case "settings.household": ContentUnavailableView("Household", systemImage: "person.2", description: Text("Who shares this budget, and their roles."))
            default: ContentUnavailableView("Nothing Selected", systemImage: "sidebar.right")
            }
        }
        .navigationDestination(for: Route.self) { r in destination(r) }
        .environment(\.open) { r in push(section, r) }
    }

    private var nothingSelected: some View {
        ContentUnavailableView("Nothing Selected", systemImage: "sidebar.trailing", description: Text("Select a row to see its details here."))
    }

    /// What the shell's runtime asks of this app (`MoneyRequest`): what
    /// only money's own screens do yet, or a message for another runtime.
    private func run(_ r: MoneyRequest) {
        switch r {
        case let .openCategory(key):
            guard let l = categoryNamed(key) else { pending.append(r); return }
            inspect(category: l.categoryId.hex)
        case let .openTransaction(key):
            // Transaction Details in the inspector while it shows; its form otherwise.
            if model.shell.page.inspector { return model.shell.send(.inspectTransaction(id: key)) }
            guard let sheets = model.sheets else { pending.append(r); return }
            sheets.send(.open(key: key))
            // Not synced yet: tried again as the household arrives.
            if sheets.page.transaction == nil { pending.append(r) }
        case let .sheet(msg): model.sheets?.send(msg)
        case .settings: settings = true
        case let .accounts(msg):
            model.shell.send(.select(tag: "accounts"))
            model.accounts?.send(msg)
        case let .stepMonth(by): model.stepMonth(Int64(by))
        case .thisMonth: model.showThisMonth()
        case let .budget(msg): model.budget?.send(msg)
        case .refresh: Task { await model.refresh() }
        // What the bank brought, to look over: Transactions' To Review.
        case .review:
            model.shell.send(.select(tag: "transactions"))
            model.transactions?.send(.show(review: true))
        case .filter, .importReceipt, .keepPhoto: break
        }
    }

    /// A register, written from Rust (Generated/RegisterView.swift): the
    /// account of this hex id's, or every account's for none.
    @ViewBuilder private func register(_ account: String?, search: String) -> some View {
        if let s = model.session {
            let m = model
            RegisterHost(session: s, devices: m.devices, account: account, search: search,
                         refresh: { Task { await m.refresh() } }, today: { m.today }, run: { r in run(r) })
                .id(account ?? "all")
        }
    }

    /// The category a launch names: by its name in any case, or a word of it.
    private func categoryNamed(_ n: String) -> BudgetLine? {
        let w = n.lowercased()
        let lines = model.snapshot.lines
        return lines.first { $0.name.lowercased() == w } ?? lines.first { $0.name.lowercased().contains(w) }
    }

    /// What a route on the back stack draws. An account and a category are
    /// found by the route's id (an older link's by its name, `names`), so a
    /// renamed one keeps its page; a bank by its name, its key. An account
    /// that is gone is the route's fallback, All Accounts.
    @ViewBuilder private func destination(_ r: Route) -> some View {
        switch r {
        case .accounts: register(nil, search: "")
        case let .account(id, _): register(id, search: "")
        case .banks: BanksView()
        case let .bank(name): BankView(name: name)
        case .settings: SettingsView()
        default: EmptyView()
        }
    }
}

/// The launch arguments are read once a process.
@MainActor private enum Launched { static var done = false }

/// The account at the end of every tab's large title (ArkUI's `AccountFace`,
/// on the title's own line by `largeTitleBar`): who is using the budget, as
/// their face, marked while a bank wants somebody there, raising the account
/// sheet — who and where they are, the banks that want a look, Settings, and
/// the way out. What its rows open is pushed on the tab once it has gone.
struct AccountButton: View {
    @EnvironmentObject private var model: Model
    @Environment(\.open) private var open
    @State private var showing = false
    /// What the sheet's row asked for, done once the sheet has gone.
    @State private var then: Then? = nil

    private enum Then { case page(Route), signOut }

    var body: some View {
        let wanting = Int(banksWanting(banks: model.snapshot.banks))
        AccountFace(name: model.who, picture: model.picture, attention: wanting > 0 ? "Banks need a look" : nil) { showing = true }
            .sheet(isPresented: $showing, onDismiss: after) {
                AccountSheet(name: model.who, picture: model.picture,
                             detail: model.isDemo ? "The demo's household, on this phone alone" : model.login.map { $0.email.isEmpty ? $0.user : $0.email } ?? "",
                             server: model.isDemo ? nil : Probe.display(model.server),
                             linked: model.snapshot.status.map { $0.linked },
                             signOut: model.isDemo ? "Leave the Demo" : "Sign Out",
                             confirm: model.isDemo ? "Leave the demo?" : "Sign out?",
                             onSettings: { leave(.page(.settings)) },
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
        case let .page(r)?: open(r)
        case .signOut?: model.signOut()
        case nil: break
        }
    }
}

/// The receipt section of the transaction the sheets' form corrects
/// (ReceiptViews.swift), and what it raises.
struct TransactionReceipt: View {
    @EnvironmentObject private var model: Model
    @State private var acts = ReceiptActs()

    var body: some View {
        if let t = model.sheets?.page.transaction, t.hasReceipt {
            ReceiptSection(txn: Id(t.id), amount: t.signed, day: t.day, payee: t.payee, acts: $acts)
                .modifier(ReceiptSheets(txn: Id(t.id), acts: $acts))
        }
    }
}

/// A register over its runtime: its own for as long as it shows, read
/// again as the household moves, searched by the shell's field.
struct RegisterHost: View {
    @StateObject private var screen: RegisterScreen
    @EnvironmentObject private var model: Model
    let search: String
    let run: (MoneyRequest) -> Void

    init(session: Session, devices: Devices?, account: String?, search: String,
         refresh: @escaping () -> Void, today: @escaping () -> String, run: @escaping (MoneyRequest) -> Void) {
        _screen = StateObject(wrappedValue: RegisterScreen(session: session, devices: devices, account: account, refresh: refresh, today: today))
        self.search = search
        self.run = run
    }

    var body: some View {
        RegisterView()
            .environmentObject(screen)
            .onAppear { screen.request = run }
            .onChange(of: model.moves) { screen.reload() }
            .onChange(of: search, initial: true) { _, q in screen.send(.search(text: q)) }
            .onChange(of: model.shell.page.transaction?.id, initial: true) { _, id in screen.send(.select(id: id)) }
    }
}

/// A pane of Settings (Generated/MoneySettingsView.swift): money's own
/// screens, until each moves into Rust.
struct SettingsHosted: View {
    let name: String

    var body: some View {
        switch name {
        case "settings.server": SettingsView()
        case "settings.banks": BanksView()
        case "settings.household": ContentUnavailableView("Household", systemImage: "house", description: Text("Who shares this budget, and their roles."))
        default: ContentUnavailableView("Appearance", systemImage: "circle.lefthalf.filled", description: Text("Follows the system's light and dark."))
        }
    }
}
