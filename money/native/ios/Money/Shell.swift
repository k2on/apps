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

/// The sheets a screen can raise.
enum Sheet: Identifiable {
    /// The transaction form: a new one or one corrected.
    case transaction(TxnForm)
    /// A category's assigned amount, typed.
    case assign(Id)
    /// Money moved out of a category, or into an overspent one; none is from Ready to Assign.
    case move(Id?)
    case target(Id)
    case addAccount
    case reconcile(Id)
    /// Transactions given a category, and approved with it when `approve`.
    case categorize([Id], approve: Bool)
    case newCategory(group: Id?)

    var id: String {
        switch self {
        case let .transaction(d): return "txn-\(d.editing?.hex ?? "new")"
        case let .assign(i): return "assign-\(i.hex)"
        case let .move(i): return "move-\(i?.hex ?? "rta")"
        case let .target(i): return "target-\(i.hex)"
        case .addAccount: return "add-account"
        case let .reconcile(i): return "reconcile-\(i.hex)"
        case let .categorize(ids, a): return "categorize-\(a)-\(ids.map(\.hex).joined())"
        case let .newCategory(g): return "new-\(g?.hex ?? "group")"
        }
    }
}

private struct PresentKey: EnvironmentKey {
    static let defaultValue: (Sheet) -> Void = { _ in }
}

private struct OpenKey: EnvironmentKey {
    static let defaultValue: (Route) -> Void = { _ in }
}

extension EnvironmentValues {
    /// Raise a sheet over the app.
    var present: (Sheet) -> Void {
        get { self[PresentKey.self] }
        set { self[PresentKey.self] = newValue }
    }

    /// Navigate to a route on the tab's own back stack: what the account sheet's rows open, once it has gone.
    var open: (Route) -> Void {
        get { self[OpenKey.self] }
        set { self[OpenKey.self] = newValue }
    }
}

struct Shell: View {
    @EnvironmentObject private var model: Model
    @EnvironmentObject private var watched: Watched<MoneyView>
    @State private var sheet: Sheet? = nil
    /// Each section's back stack, by its tag, of money's routes.
    @State private var paths: [String: NavigationPath] = [:]
    /// What the inspector shows of a transaction, by its id (a category's
    /// is the shell's own Category Details, written from Rust).
    @State private var inspecting: Inspecting? = nil
    /// What the launch asked for that the household had not yet synced:
    /// tried again as the snapshot moves.
    @State private var pending: [MoneyRequest] = []
    /// Settings asked for by the launch: the Mac's window, a sheet elsewhere.
    @State private var settings = false

    enum Inspecting: Equatable { case transaction(Id) }

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
            .environment(\.hostedScreens) { name in AnyView(hosted(name)) }
            .environment(\.sectionPath) { tag in path(tag) }
            .environment(\.present) { s in present(s) }
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
            .sheet(item: $sheet) { s in
                sheetView(s)
                    .environmentObject(model)
                    .environmentObject(watched)
                    .environment(\.present) { next in present(next) }
            }
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
            sheet = nil
            paths = [:]
            inspecting = nil
            settings = false
            pending = []
            model.shell.send(.launch(args: args))
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
                            a.reconcile = { id in present(.reconcile(id)) }
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
            case "insights": InsightsView()
            case "accounts.register":
                // The account the shell shows (its place's tag, `accounts/<id>`).
                if let id = model.snapshot.accounts.first(where: { "accounts/\($0.id.hex)" == model.shell.page.section })?.id {
                    // Under the shell's own search field (WithSearch).
                    RegisterView(account: id, shellSearch: model.shell.page.search)
                } else {
                    ContentUnavailableView("No Account Selected", systemImage: "building.columns")
                }
            case "budget.inspector":
                nothingSelected
            case "transactions.inspector":
                if case let .transaction(id)? = inspecting, let t = model.register(nil)?.first(where: { $0.id == id }) {
                    TransactionDetail(txn: t)
                } else { nothingSelected }
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
            guard let t = transactionNamed(key) else { pending.append(r); return }
            if model.shell.page.inspector { inspecting = .transaction(t.id) } else { present(.transaction(TxnForm(t))) }
        case .settings: settings = true
        case .newTransaction:
            let a = usualAccount(accounts: model.snapshot.accounts)
            present(.transaction(TxnForm(account: a, accounts: model.snapshot.accounts, date: model.today)))
        case let .accounts(msg):
            if case .add = msg { present(.addAccount) } else { model.accounts?.send(msg) }
        case let .stepMonth(by): model.stepMonth(Int64(by))
        case .thisMonth: model.showThisMonth()
        case let .budget(msg): model.budget?.send(msg)
        case .refresh: Task { await model.refresh() }
        case let .reconcile(account): present(.reconcile(Id(account)))
        case .review, .filter, .importReceipt, .keepPhoto: break
        }
    }

    /// The category a launch names: by its name in any case, or a word of it.
    private func categoryNamed(_ n: String) -> BudgetLine? {
        let w = n.lowercased()
        let lines = model.snapshot.lines
        return lines.first { $0.name.lowercased() == w } ?? lines.first { $0.name.lowercased().contains(w) }
    }

    /// The transaction a launch names (SCREENS.md's keys): `receipt`, the
    /// newest with a receipt; `review`, the newest waiting for a look; a
    /// number, the n-th oldest; else a payee.
    private func transactionNamed(_ key: String) -> TxnLine? {
        guard let all = model.register(nil), !all.isEmpty else { return nil }
        let k = key.lowercased()
        switch k {
        case "receipt": return all.first { !$0.receipt.isEmpty } ?? all.first { $0.photos > 0 }
        case "review": return all.first { !$0.approved }
        case "transfer": return all.first { $0.transfer }
        default:
            if let t = all.first(where: { $0.id.hex == k }) { return t }
            if let n = Int(k), n >= 1, n <= all.count { return all.reversed()[n - 1] }
            return all.first { $0.payee.lowercased().contains(k) }
        }
    }

    private func present(_ s: Sheet) {
        if sheet == nil {
            sheet = s
            return
        }
        sheet = nil
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { sheet = s }
    }

    /// What a route on the back stack draws. An account and a category are
    /// found by the route's id (an older link's by its name, `names`), so a
    /// renamed one keeps its page; a bank by its name, its key. An account
    /// that is gone is the route's fallback, All Accounts.
    @ViewBuilder private func destination(_ r: Route) -> some View {
        switch r {
        case .accounts: RegisterView(account: nil)
        case .account: RegisterView(account: model.snapshot.accounts.first { r.names(id: $0.id.hex, name: $0.name) }?.id)
        case .banks: BanksView()
        case let .bank(name): BankView(name: name)
        case .settings: SettingsView()
        default: EmptyView()
        }
    }

    @ViewBuilder private func sheetView(_ s: Sheet) -> some View {
        switch s {
        case let .transaction(d): TransactionSheet(draft: d).presentationDetents([.large])
        case let .assign(id): AssignSheet(id: id).presentationDetents([.medium])
        case let .move(id): MoveSheet(id: id).presentationDetents([.medium, .large])
        case let .target(id): TargetSheet(id: id).presentationDetents([.medium])
        case .addAccount: AddAccountSheet().presentationDetents([.large])
        case let .reconcile(id): ReconcileSheet(id: id).presentationDetents([.medium])
        case let .categorize(ids, approve): CategorizeSheet(ids: ids, approve: approve).presentationDetents([.medium, .large])
        case let .newCategory(group): NewCategorySheet(group: group).presentationDetents([.height(320)])
        }
    }
}

/// The launch arguments are read once a process.
@MainActor private enum Launched { static var done = false }

/// A transaction's details in the inspector: its payee and amount, then
/// where and when, and what it was for.
struct TransactionDetail: View {
    let txn: TxnLine
    @Environment(\.present) private var present

    var body: some View {
        Form {
            Section {
                VStack(spacing: 6) {
                    Text(txn.payee).font(.headline)
                    CentsText(txn.amount, signed: txn.amount > 0).font(.system(.largeTitle, design: .rounded, weight: .bold))
                        .foregroundStyle(txn.amount > 0 ? Color.positive : Color.ink)
                }
                .frame(maxWidth: .infinity)
            }
            Section {
                LabeledContent("Account", value: txn.account)
                LabeledContent("Date", value: txn.date)
                LabeledContent("Category", value: txn.category)
                LabeledContent("Status", value: txn.approved ? "Approved" : "Needs Review")
                if !txn.memo.isEmpty { LabeledContent("Memo", value: txn.memo) }
            }
            Section { Button("Edit Transaction") { present(.transaction(TxnForm(txn))) } }
        }
        .formStyle(.grouped)
    }
}

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

/// The toolbar's add button: a new transaction in `account`, or the usual one.
struct AddTransactionButton: View {
    var account: Id? = nil
    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present

    var body: some View {
        Button {
            let a = account ?? usualAccount(accounts: model.snapshot.accounts)
            present(.transaction(TxnForm(account: a, accounts: model.snapshot.accounts, date: model.today)))
        } label: {
            Image(systemName: "plus")
        }
        .accessibilityLabel("Add Transaction")
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
