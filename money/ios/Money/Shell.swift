// money, once the first run has handed it a session: the system's tab bar
// in Liquid Glass — Budget, Accounts, Transactions (badged with what needs a
// look) and Insights — which shrinks as a list scrolls
// (`tabBarMinimizeBehavior(.onScrollDown)`); each tab its own
// `NavigationStack`, so a tab switch finds the tab as it was left; the
// sheets a screen raises, each with its detents, one at a time; Plaid's
// page in Safari's view when a bank is being connected; and what the app
// says once, on glass at the top. This file wires; what each
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

/// The tabs. (`Tab` is SwiftUI's.)
enum Pane: Hashable {
    case budget, accounts, transactions, insights
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
    @Environment(Watched<MoneyView>.self) private var watched
    @State private var tab: Pane = .budget
    @State private var sheet: Sheet? = nil
    /// Each tab's back stack (ArkUI's `Navigator`), of money's routes.
    @State private var navs: [Pane: Navigator<Route>] = [:]

    private func path(_ t: Pane) -> Binding<[Route]> {
        Binding(get: { navs[t]?.path ?? [] }, set: { navs[t, default: Navigator()].path = $0 })
    }

    var body: some View {
        TabView(selection: $tab) {
            Tab("Budget", systemImage: "dollarsign.circle.fill", value: Pane.budget) {
                stack(.budget) { BudgetView() }
            }
            Tab("Accounts", systemImage: "building.columns.fill", value: Pane.accounts) {
                stack(.accounts) { AccountsView() }
            }
            Tab("Transactions", systemImage: "list.bullet.rectangle.portrait.fill", value: Pane.transactions) {
                stack(.transactions) { TransactionsView() }
            }
            .badge(model.snapshot.inbox.count)
            Tab("Insights", systemImage: "chart.pie.fill", value: Pane.insights) {
                stack(.insights) { InsightsView() }
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
        .environment(\.present) { s in present(s) }
        .sheet(item: $sheet) { s in
            sheetView(s)
                .environmentObject(model)
                .environment(watched)
                .environment(\.present) { next in present(next) }
        }
        .toast($model.notice)
        // A link the app was opened with: its route, navigated to on the tab shown.
        .onChange(of: model.link) {
            guard let url = model.link else { return }
            model.link = nil
            navs[tab, default: Navigator()].handle(url, scheme: AppInfo.money.scheme, parse: { routeFromPath(path: $0) }, take: model.takes)
        }
        // Plaid's page, as soon as the server answers with it, wherever the
        // person is; closed by its Done, or once the server is finished.
        .safari(model.bankPage, tint: Color.accent) { model.bankPageClosed() }
    }

    private func present(_ s: Sheet) {
        if sheet == nil {
            sheet = s
            return
        }
        sheet = nil
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { sheet = s }
    }

    /// A tab's stack: its root, and every page it can open.
    private func stack<Root: View>(_ t: Pane, @ViewBuilder root: () -> Root) -> some View {
        NavigationStack(path: path(t)) {
            root()
                .navigationDestination(for: Route.self) { r in destination(r) }
        }
        .environment(\.open) { r in navs[t, default: Navigator()].push(r) }
    }

    /// What a route on the back stack draws. An account and a category are
    /// found by the route's id (an older link's by its name, `names`), so a
    /// renamed one keeps its page; a bank by its name, its key. An account
    /// that is gone is the route's fallback, All Accounts.
    @ViewBuilder private func destination(_ r: Route) -> some View {
        switch r {
        case .category:
            if let l = model.snapshot.lines.first(where: { r.names(id: $0.categoryId.hex, name: $0.name) }) { CategoryView(id: l.categoryId) } else { ContentUnavailableView("This Category Is Gone", systemImage: "tray") }
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
