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

/// What a screen can push onto its tab's stack.
enum Route: Hashable {
    /// A category's month, the desktop's inspector.
    case category(Id)
    /// An account's register; none is every account's.
    case register(Id?)
    /// The banks, as Settings lists them, from the Accounts tab.
    case banks
    /// One connected bank, by the name the server keeps it under.
    case bank(String)
    case settings
}

/// The sheets a screen can raise.
enum Sheet: Identifiable {
    /// The transaction form: a new one or one corrected.
    case transaction(Draft)
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

extension EnvironmentValues {
    /// Raise a sheet over the app.
    var present: (Sheet) -> Void {
        get { self[PresentKey.self] }
        set { self[PresentKey.self] = newValue }
    }
}

struct Shell: View {
    @EnvironmentObject private var model: Model
    @State private var tab: Pane = .budget
    @State private var sheet: Sheet? = nil
    @State private var paths: [Pane: NavigationPath] = [:]

    private func path(_ t: Pane) -> Binding<NavigationPath> {
        Binding(get: { paths[t] ?? NavigationPath() }, set: { paths[t] = $0 })
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
                .environment(\.present) { next in present(next) }
        }
        .toast($model.notice)
        // Plaid's page, as soon as the server answers with it, wherever the
        // person is; closed by its Done, or once the server is finished.
        .safari(model.bankPage, tint: Color.money) { model.bankPageClosed() }
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
                .navigationDestination(for: Route.self) { r in
                    switch r {
                    case let .category(id): CategoryView(id: id)
                    case let .register(account): RegisterView(account: account)
                    case .banks: BanksView()
                    case let .bank(name): BankView(name: name)
                    case .settings: SettingsView()
                    }
                }
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

/// The toolbar's account button: who is using the budget, where, and the
/// way out — marked when a bank wants somebody in Settings.
struct AccountButton: View {
    @EnvironmentObject private var model: Model

    var body: some View {
        let wanting = Banks.wanting(model.snapshot.banks)
        NavigationLink(value: Route.settings) {
            Image(systemName: wanting > 0 ? "person.crop.circle.badge.exclamationmark"
                : model.isDemo ? "person.crop.circle.badge.questionmark" : "person.crop.circle")
        }
        .accessibilityLabel("Account and settings")
        .accessibilityValue(wanting > 0 ? "Banks need a look" : "")
    }
}

/// The toolbar's add button: a new transaction in `account`, or the usual one.
struct AddTransactionButton: View {
    var account: Id? = nil
    @EnvironmentObject private var model: Model
    @Environment(\.present) private var present

    var body: some View {
        Button {
            let a = account ?? Accounts.usual(model.snapshot.accounts)?.id
            present(.transaction(Draft(account: a, date: model.today)))
        } label: {
            Image(systemName: "plus")
        }
        .accessibilityLabel("Add Transaction")
    }
}
