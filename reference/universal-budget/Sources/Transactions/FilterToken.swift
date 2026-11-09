import Foundation

/// A filter on the transaction list. Filters are search tokens, so active
/// filters sit in the search field where people can see and remove them,
/// the way Mail and Photos show search scopes.
enum FilterToken: Hashable, Identifiable {
    case account(Account.ID)
    case category(Category.ID)
    case period(Period)
    case inflow
    case outflow
    case needsReview
    case uncategorized
    case hasReceipt

    enum Period: String, CaseIterable, Identifiable {
        case thisMonth, lastMonth, lastThreeMonths
        var id: Self { self }
        var title: String {
            switch self {
            case .thisMonth: "This Month"
            case .lastMonth: "Last Month"
            case .lastThreeMonths: "Last 3 Months"
            }
        }
        func months(endingIn current: Month) -> ClosedRange<Month> {
            switch self {
            case .thisMonth: current...current
            case .lastMonth: current.previous...current.previous
            case .lastThreeMonths: current.advanced(by: -2)...current
            }
        }
    }

    var id: Self { self }

    /// Tokens of the same kind replace each other (one account at a time).
    var kind: String {
        switch self {
        case .account: "account"
        case .category: "category"
        case .period: "period"
        case .inflow, .outflow: "flow"
        case .needsReview: "review"
        case .uncategorized: "uncategorized"
        case .hasReceipt: "receipt"
        }
    }

    func title(in store: BudgetStore) -> String {
        switch self {
        case .account(let id): store.account(id)?.name ?? "Account"
        case .category(let id): store.categoryName(id) ?? "Category"
        case .period(let period): period.title
        case .inflow: "Inflow"
        case .outflow: "Outflow"
        case .needsReview: "Needs Review"
        case .uncategorized: "Uncategorized"
        case .hasReceipt: "Has Receipt"
        }
    }

    var systemImage: String {
        switch self {
        case .account: "building.columns"
        case .category: "tag"
        case .period: "calendar"
        case .inflow: "arrow.down.left"
        case .outflow: "arrow.up.right"
        case .needsReview: "tray.and.arrow.down"
        case .uncategorized: "questionmark.circle"
        case .hasReceipt: "doc.text"
        }
    }

    func matches(_ transaction: Transaction, in store: BudgetStore) -> Bool {
        switch self {
        case .account(let id): transaction.accountID == id
        case .category(let id): transaction.categoryLines.contains { $0.categoryID == id }
        case .period(let period):
            period.months(endingIn: store.currentMonth).contains(Month(containing: transaction.date))
        case .inflow: transaction.amount > 0
        case .outflow: transaction.amount < 0
        case .needsReview: !transaction.isApproved
        case .uncategorized: store.needsCategory(transaction)
        case .hasReceipt: transaction.receipt.map { !$0.isEmpty } ?? false
        }
    }
}

extension BudgetStore {
    /// Transactions matching every token and the search text (payee, memo,
    /// category, account or amount).
    func transactions(matching tokens: [FilterToken], text: String) -> [Transaction] {
        let query = text.trimmingCharacters(in: .whitespaces)
        return transactions.filter { transaction in
            guard tokens.allSatisfy({ $0.matches(transaction, in: self) }) else { return false }
            guard !query.isEmpty else { return true }
            let fields = [
                transaction.payee,
                transaction.memo,
                account(transaction.accountID)?.name ?? "",
                account(transaction.transferAccountID)?.name ?? "",
                categoryName(transaction.categoryID) ?? "",
                "\(transaction.amount < 0 ? -transaction.amount : transaction.amount)",
            ] + transaction.splits.compactMap { categoryName($0.categoryID) }
            return fields.contains { $0.localizedCaseInsensitiveContains(query) }
        }
    }
}
