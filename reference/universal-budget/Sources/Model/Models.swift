import Foundation

// MARK: - Household

struct Household: Hashable, Sendable {
    var name: String
    var members: [Member]
}

struct Member: Identifiable, Hashable, Sendable {
    enum Role: String, CaseIterable, Sendable {
        case owner, member, invited

        var title: String {
            switch self {
            case .owner: "Owner"
            case .member: "Member"
            case .invited: "Invitation Sent"
            }
        }
    }

    var id: String
    var name: String
    var email: String
    var role: Role

    var initials: String {
        name.split(separator: " ").prefix(2).compactMap(\.first).map(String.init).joined()
    }
}

// MARK: - Accounts

enum AccountKind: String, CaseIterable, Identifiable, Sendable {
    case checking, savings, cash, creditCard
    case mortgage, loan, investment, asset

    var id: Self { self }

    /// Budget accounts hold money you can give a job; tracking accounts
    /// only count towards net worth.
    var isBudget: Bool {
        switch self {
        case .checking, .savings, .cash, .creditCard: true
        case .mortgage, .loan, .investment, .asset: false
        }
    }

    var isLiability: Bool {
        switch self {
        case .creditCard, .mortgage, .loan: true
        default: false
        }
    }

    var title: String {
        switch self {
        case .checking: "Checking"
        case .savings: "Savings"
        case .cash: "Cash"
        case .creditCard: "Credit Card"
        case .mortgage: "Mortgage"
        case .loan: "Loan"
        case .investment: "Investment"
        case .asset: "Asset"
        }
    }

    var systemImage: String {
        switch self {
        case .checking: "building.columns"
        case .savings: "banknote"
        case .cash: "dollarsign"
        case .creditCard: "creditcard"
        case .mortgage: "house"
        case .loan: "doc.text"
        case .investment: "chart.line.uptrend.xyaxis"
        case .asset: "car"
        }
    }

    static var budgetKinds: [AccountKind] { allCases.filter(\.isBudget) }
    static var trackingKinds: [AccountKind] { allCases.filter { !$0.isBudget } }
}

struct Account: Identifiable, Hashable, Sendable {
    typealias ID = String

    var id: ID
    var name: String
    var kind: AccountKind
    /// The bank connection that syncs this account, if any.
    var bankID: BankConnection.ID?
    var lastReconciled: Date?

    var isBudget: Bool { kind.isBudget }
}

struct BankConnection: Identifiable, Hashable, Sendable {
    typealias ID = String

    enum Status: Hashable, Sendable {
        case connected
        case needsAttention(String)
    }

    var id: ID
    var institution: String
    var lastSynced: Date
    var status: Status
}

// MARK: - Categories

struct CategoryGroup: Identifiable, Hashable, Sendable {
    typealias ID = String

    var id: ID
    var name: String
    var categories: [Category]
}

struct Category: Identifiable, Hashable, Sendable {
    typealias ID = String

    /// Inflows given this category go to Ready to Assign.
    static let readyToAssign: ID = "ready-to-assign"

    var id: ID
    var name: String
    var emoji: String?
    var goal: Goal?
}

/// What a category is working towards.
enum Goal: Hashable, Sendable {
    /// Assign a fixed amount every month (bills, groceries).
    case monthly(Decimal)
    /// Build up a balance, optionally by a given month (savings goals).
    case targetBalance(Decimal, by: Month?)

    var amount: Decimal {
        switch self {
        case .monthly(let amount), .targetBalance(let amount, _): amount
        }
    }

    enum Kind: String, CaseIterable, Identifiable {
        case monthly, targetBalance, targetBalanceByDate
        var id: Self { self }
        var title: String {
            switch self {
            case .monthly: "Monthly Amount"
            case .targetBalance: "Target Balance"
            case .targetBalanceByDate: "Target Balance by Date"
            }
        }
    }

    var kind: Kind {
        switch self {
        case .monthly: .monthly
        case .targetBalance(_, nil): .targetBalance
        case .targetBalance: .targetBalanceByDate
        }
    }
}

// MARK: - Transactions

enum ClearedState: String, CaseIterable, Sendable {
    case uncleared, cleared, reconciled

    var title: String {
        switch self {
        case .uncleared: "Uncleared"
        case .cleared: "Cleared"
        case .reconciled: "Reconciled"
        }
    }

    var systemImage: String {
        switch self {
        case .uncleared: "circle"
        case .cleared: "checkmark.circle.fill"
        case .reconciled: "lock.circle.fill"
        }
    }
}

struct Split: Identifiable, Hashable, Sendable {
    var id = UUID()
    var categoryID: Category.ID?
    var amount: Decimal
    var memo: String = ""
}

struct ReceiptItem: Identifiable, Hashable, Sendable {
    var id = UUID()
    var name: String
    var quantity: Int = 1
    var price: Decimal

    var total: Decimal { price * Decimal(quantity) }
}

struct Receipt: Hashable, Sendable {
    /// Image data from the photo library or a scan. Never sample data.
    var imageData: Data?
    var items: [ReceiptItem] = []

    var itemsTotal: Decimal { items.reduce(0) { $0 + $1.total } }
    var isEmpty: Bool { imageData == nil && items.isEmpty }
}

struct Transaction: Identifiable, Hashable, Sendable {
    typealias ID = Int

    var id: ID
    var date: Date
    var accountID: Account.ID
    var payee: String
    /// Negative for money leaving the account, positive for money arriving.
    var amount: Decimal
    /// Ignored when `splits` is not empty.
    var categoryID: Category.ID?
    var splits: [Split] = []
    /// For a transfer, the other account. The other side is its own
    /// transaction in that account, linked by `transferID`.
    var transferAccountID: Account.ID?
    var transferID: ID?
    var memo: String = ""
    var cleared: ClearedState = .cleared
    /// Imported from a bank and not yet reviewed by a person.
    var isApproved: Bool = true
    var receipt: Receipt?

    var isTransfer: Bool { transferAccountID != nil }
    var isSplit: Bool { !splits.isEmpty }
    var isInflow: Bool { amount > 0 }

    /// Each (category, amount) this transaction contributes to the budget.
    var categoryLines: [(categoryID: Category.ID?, amount: Decimal)] {
        isSplit ? splits.map { ($0.categoryID, $0.amount) } : [(categoryID, amount)]
    }

    /// Whether any line is missing a category. Whether that matters depends
    /// on the accounts involved: see `BudgetStore.needsCategory(_:)`.
    var hasUncategorizedLines: Bool {
        isSplit ? splits.contains { $0.categoryID == nil } : categoryID == nil
    }

    /// Opening balances are inflows, but they are not income.
    var isStartingBalance: Bool { payee == Transaction.startingBalancePayee }

    static let startingBalancePayee = "Starting Balance"
    static let reconciliationPayee = "Reconciliation Balance Adjustment"
}

// MARK: - Budget summaries

/// Where a category stands in a month, used for colour and wording.
enum CategoryStatus: Hashable {
    case overspent(Decimal)
    case underfunded(Decimal)
    case funded
    case zero
    case positive

    var isAttentionNeeded: Bool {
        switch self {
        case .overspent, .underfunded: true
        default: false
        }
    }
}

struct CategorySummary: Hashable {
    var carriedOver: Decimal
    var assigned: Decimal
    var activity: Decimal
    var available: Decimal
    /// Amount still to assign this month to stay on track with the goal.
    var neededThisMonth: Decimal
    /// 0...1 progress towards the goal, if there is one.
    var goalProgress: Double?
    var status: CategoryStatus
}
