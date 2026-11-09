import SwiftUI

/// The top-level sections. On iPhone these are the tabs; on iPad and Mac
/// they are the sidebar, where each account also gets its own entry.
enum AppTab: Hashable {
    case budget, accounts, transactions, insights
    case account(Account.ID)
}

/// Destinations pushed onto a tab's navigation stack.
enum Route: Hashable {
    case category(Category.ID)
    case account(Account.ID)
    case transaction(Transaction.ID)
}

/// Modal tasks: each is a self-contained form with Cancel and Done.
enum SheetRoute: Identifiable {
    case addTransaction(Transaction)
    case editTransaction(Transaction.ID)
    case addAccount
    case reconcile(Account.ID)
    case settings

    var id: String {
        switch self {
        case .addTransaction: "add-transaction"
        case .editTransaction(let id): "edit-\(id)"
        case .addAccount: "add-account"
        case .reconcile(let id): "reconcile-\(id)"
        case .settings: "settings"
        }
    }
}

/// Navigation state for one window (scene). Each window on iPad and Mac has
/// its own, so two windows can show different months or accounts. Menu
/// commands reach the frontmost window's model through `focusedSceneValue`.
@Observable
final class SceneModel {
    var tab: AppTab = .budget
    var paths: [AppTab: [Route]] = [:]
    var month: Month
    var showsInspector = false
    var prefersSidebar = false

    var categorySelection: Category.ID?
    var transactionSelection: Set<Transaction.ID> = []

    var sheet: SheetRoute?

    var searchText = ""
    var searchTokens: [FilterToken] = []
    var isSearchFocused = false

    init(month: Month) {
        self.month = month
    }

    // MARK: - Navigation

    func path(for tab: AppTab) -> Binding<[Route]> {
        Binding(get: { self.paths[tab] ?? [] }, set: { self.paths[tab] = $0 })
    }

    /// Opens something chosen in a list. If the inspector is showing, the
    /// item is shown there, beside the list; otherwise it is pushed. That one
    /// rule gives the iPhone a push, and iPad and Mac a live inspector.
    func open(_ route: Route) {
        switch route {
        case .category(let id) where showsInspector:
            categorySelection = id
        case .transaction(let id) where showsInspector:
            transactionSelection = [id]
        default:
            paths[tab, default: []].append(route)
        }
    }

    /// Switches section. A selection belongs to the screen it was made on,
    /// so it's cleared: menu commands must not act on rows out of sight.
    func show(_ tab: AppTab) {
        guard tab != self.tab else { return }
        self.tab = tab
        transactionSelection = []
    }

    /// Shows transactions matching a filter, e.g. "Needs Review".
    func showTransactions(matching tokens: [FilterToken]) {
        tab = .transactions
        paths[.transactions] = []
        searchTokens = tokens
    }

    func findTransactions() {
        if tab != .transactions { tab = .transactions }
        isSearchFocused = true
    }

    // MARK: - Months

    func goToPreviousMonth(in store: BudgetStore) {
        if month > store.budgetMonths.lowerBound { month = month.previous }
    }

    func goToNextMonth(in store: BudgetStore) {
        if month < store.budgetMonths.upperBound { month = month.next }
    }

    func goToCurrentMonth(in store: BudgetStore) {
        month = store.currentMonth
    }

    // MARK: - Sheets

    func newTransaction(in store: BudgetStore, account: Account.ID? = nil) {
        let accountID: Account.ID? = switch tab {
        case .account(let id): account ?? id
        default: account ?? store.budgetAccounts.first?.id
        }
        guard let accountID else { return }
        sheet = .addTransaction(Transaction(
            id: 0, date: store.today, accountID: accountID, payee: "", amount: 0,
            cleared: .uncleared
        ))
    }

    /// The account the frontmost screen is about, if any.
    var focusedAccount: Account.ID? {
        if case .account(let id) = tab { return id }
        for route in (paths[tab] ?? []).reversed() {
            if case .account(let id) = route { return id }
        }
        return nil
    }

    // MARK: - Launch arguments

    func apply(_ options: LaunchOptions, store: BudgetStore) {
        if let month = options.month { self.month = month }
        showsInspector = options.showsInspector
        prefersSidebar = options.showsAllColumns
        if let search = options.search { searchText = search }

        switch options.screen {
        case .budget, nil:
            tab = .budget
        case .category(let name):
            tab = .budget
            if let category = store.categories.first(where: {
                $0.id == name.lowercased() || $0.name.localizedCaseInsensitiveContains(name)
            }) {
                if showsInspector { categorySelection = category.id }
                else { paths[.budget] = [.category(category.id)] }
            }
        case .accounts:
            tab = .accounts
        case .account(let name):
            tab = .accounts
            if let account = store.account(named: name) {
                #if os(macOS)
                // The Mac's sidebar lists each account: select it (a path set
                // at launch was not pushed on the Mac; BUILD-NOTES.md).
                tab = .account(account.id)
                #else
                paths[.accounts] = [.account(account.id)]
                #endif
            }
        case .reconcile(let name):
            tab = .accounts
            if let account = store.account(named: name) {
                #if os(macOS)
                tab = .account(account.id)
                #else
                paths[.accounts] = [.account(account.id)]
                #endif
                sheet = .reconcile(account.id)
            }
        case .addAccount:
            tab = .accounts
            sheet = .addAccount
        case .transactions:
            tab = .transactions
        case .review:
            showTransactions(matching: [.needsReview])
        case .transaction(let key):
            tab = .transactions
            if let id = resolveTransaction(key, in: store) {
                if showsInspector { transactionSelection = [id] }
                else { paths[.transactions] = [.transaction(id)] }
            }
        case .editTransaction(let key):
            tab = .transactions
            if let id = resolveTransaction(key, in: store) {
                transactionSelection = [id]
                sheet = .editTransaction(id)
            }
        case .addTransaction:
            tab = .transactions
            newTransaction(in: store)
        case .insights:
            tab = .insights
        case .settings:
            sheet = .settings
        }
    }

    /// Accepts a numeric id, a keyword (`receipt`, `split`, `transfer`,
    /// `review`) or a payee (the most recent transaction with that payee).
    private func resolveTransaction(_ key: String, in store: BudgetStore) -> Transaction.ID? {
        if let id = Int(key), store.transaction(id) != nil { return id }
        let newestFirst = store.transactions.sorted { $0.date > $1.date }
        let match: Transaction? = switch key.lowercased() {
        case "receipt": newestFirst.first { $0.receipt != nil }
        case "split": newestFirst.first(where: \.isSplit)
        case "transfer": newestFirst.first(where: \.isTransfer)
        case "review": newestFirst.first { !$0.isApproved }
        default: newestFirst.first { $0.payee.localizedCaseInsensitiveContains(key) }
        }
        return match?.id
    }
}
