import Foundation
import Observation

/// The household's budget: accounts, categories, transactions and the money
/// assigned to each category each month. In memory only.
///
/// One store is shared by every window/scene; navigation state lives per
/// scene in `SceneModel`.
@Observable
final class BudgetStore {
    var household: Household
    var accounts: [Account]
    var groups: [CategoryGroup]
    var transactions: [Transaction]
    var assignments: [Month: [Category.ID: Decimal]]
    var banks: [BankConnection]
    var currencyCode: String

    /// "Now" for the sample household. Fixed, so every launch (and every
    /// screenshot) shows the same budget.
    let today: Date

    private var nextID: Transaction.ID

    init(
        household: Household,
        accounts: [Account],
        groups: [CategoryGroup],
        transactions: [Transaction],
        assignments: [Month: [Category.ID: Decimal]],
        banks: [BankConnection],
        currencyCode: String,
        today: Date
    ) {
        self.household = household
        self.accounts = accounts
        self.groups = groups
        self.transactions = transactions
        self.assignments = assignments
        self.banks = banks
        self.currencyCode = currencyCode
        self.today = today
        self.nextID = (transactions.map(\.id).max() ?? 0) + 1
    }

    // MARK: - Lookup

    var currentMonth: Month { Month(containing: today) }

    /// The earliest month with any data; the budget can't go back further.
    var firstMonth: Month {
        transactions.map(\.date).min().map { Month(containing: $0) } ?? currentMonth
    }

    /// Months the budget can show: from the first month with data to three
    /// months ahead, so people can plan.
    var budgetMonths: ClosedRange<Month> { firstMonth...currentMonth.advanced(by: 3) }

    var categories: [Category] { groups.flatMap(\.categories) }

    func account(_ id: Account.ID?) -> Account? {
        guard let id else { return nil }
        return accounts.first { $0.id == id }
    }

    /// Finds an account by id or by name, case-insensitively (launch arguments).
    func account(named name: String) -> Account? {
        let key = name.lowercased()
        return accounts.first { $0.id == key || $0.name.lowercased() == key }
            ?? accounts.first { $0.name.lowercased().contains(key) }
    }

    func category(_ id: Category.ID?) -> Category? {
        guard let id else { return nil }
        return categories.first { $0.id == id }
    }

    func group(containing categoryID: Category.ID) -> CategoryGroup? {
        groups.first { $0.categories.contains { $0.id == categoryID } }
    }

    func transaction(_ id: Transaction.ID?) -> Transaction? {
        guard let id else { return nil }
        return transactions.first { $0.id == id }
    }

    func bank(_ id: BankConnection.ID?) -> BankConnection? {
        guard let id else { return nil }
        return banks.first { $0.id == id }
    }

    var budgetAccounts: [Account] { accounts.filter(\.isBudget) }
    var trackingAccounts: [Account] { accounts.filter { !$0.isBudget } }

    func accounts(connectedTo bankID: BankConnection.ID) -> [Account] {
        accounts.filter { $0.bankID == bankID }
    }

    /// Payees seen before, most frequent first, for suggestions.
    var payees: [String] {
        let counts = Dictionary(
            transactions.filter { !$0.isTransfer && !$0.isStartingBalance }.map { ($0.payee, 1) },
            uniquingKeysWith: +
        )
        return counts.sorted { ($0.value, $1.key) > ($1.value, $0.key) }.map(\.key)
    }

    /// The category most recently used with a payee, to prefill new entries.
    func lastCategory(for payee: String) -> Category.ID? {
        transactions
            .filter { $0.payee.caseInsensitiveCompare(payee) == .orderedSame && !$0.isSplit }
            .max { $0.date < $1.date }?
            .categoryID
    }

    // MARK: - Display helpers

    func categoryName(_ id: Category.ID?) -> String? {
        guard let id else { return nil }
        if id == Category.readyToAssign { return "Ready to Assign" }
        return category(id)?.name
    }

    /// Whether a transaction must be given a category before the budget is
    /// right. Tracking accounts and transfers between budget accounts don't
    /// touch the budget, so they never need one.
    func needsCategory(_ transaction: Transaction) -> Bool {
        guard account(transaction.accountID)?.isBudget == true else { return false }
        if let other = account(transaction.transferAccountID), other.isBudget { return false }
        return transaction.hasUncategorizedLines
    }

    // MARK: - Balances

    func balance(of accountID: Account.ID) -> Decimal {
        transactions.lazy.filter { $0.accountID == accountID }.reduce(0) { $0 + $1.amount }
    }

    func clearedBalance(of accountID: Account.ID) -> Decimal {
        transactions.lazy
            .filter { $0.accountID == accountID && $0.cleared != .uncleared }
            .reduce(0) { $0 + $1.amount }
    }

    func unclearedBalance(of accountID: Account.ID) -> Decimal {
        balance(of: accountID) - clearedBalance(of: accountID)
    }

    func total(of accounts: [Account]) -> Decimal {
        accounts.reduce(0) { $0 + balance(of: $1.id) }
    }

    var netWorth: Decimal { total(of: accounts) }
    var assets: Decimal { accounts.map { balance(of: $0.id) }.filter { $0 > 0 }.reduce(0, +) }
    var liabilities: Decimal { accounts.map { balance(of: $0.id) }.filter { $0 < 0 }.reduce(0, +) }

    // MARK: - Transactions waiting for review

    var unapproved: [Transaction] { transactions.filter { !$0.isApproved } }

    func unapproved(in accountID: Account.ID) -> [Transaction] {
        transactions.filter { !$0.isApproved && $0.accountID == accountID }
    }

    // MARK: - Budget arithmetic

    func assigned(_ categoryID: Category.ID, in month: Month) -> Decimal {
        assignments[month]?[categoryID] ?? 0
    }

    func activity(_ categoryID: Category.ID, in month: Month) -> Decimal {
        var total: Decimal = 0
        for transaction in transactions where month.contains(transaction.date) {
            for line in transaction.categoryLines where line.categoryID == categoryID {
                total += line.amount
            }
        }
        return total
    }

    /// Positive balances roll into next month; overspending does not (it is
    /// taken from next month's Ready to Assign instead).
    func carriedOver(_ categoryID: Category.ID, into month: Month) -> Decimal {
        guard month > firstMonth else { return 0 }
        return max(0, available(categoryID, in: month.previous))
    }

    func available(_ categoryID: Category.ID, in month: Month) -> Decimal {
        guard month >= firstMonth else { return 0 }
        return carriedOver(categoryID, into: month)
            + assigned(categoryID, in: month)
            + activity(categoryID, in: month)
    }

    func totalAssigned(in month: Month) -> Decimal {
        assignments[month]?.values.reduce(0, +) ?? 0
    }

    func totalActivity(in month: Month) -> Decimal {
        categories.reduce(0) { $0 + activity($1.id, in: month) }
    }

    func totalAvailable(in month: Month) -> Decimal {
        categories.reduce(0) { $0 + available($1.id, in: month) }
    }

    /// Cash overspending in a month: the sum of negative available balances.
    func overspent(in month: Month) -> Decimal {
        categories.reduce(0) { $0 + min(0, available($1.id, in: month)) }
    }

    /// Money received but not yet given a job, as of `month`.
    func readyToAssign(in month: Month) -> Decimal {
        guard month >= firstMonth else { return 0 }
        var income: Decimal = 0
        for transaction in transactions where transaction.date < month.end {
            for line in transaction.categoryLines where line.categoryID == Category.readyToAssign {
                income += line.amount
            }
        }
        var assigned: Decimal = 0
        var month2 = firstMonth
        var overspentBefore: Decimal = 0
        while month2 <= month {
            assigned += totalAssigned(in: month2)
            if month2 < month { overspentBefore += overspent(in: month2) }
            month2 = month2.next
        }
        return income - assigned + overspentBefore
    }

    func summary(for category: Category, in month: Month) -> CategorySummary {
        let carried = carriedOver(category.id, into: month)
        let assigned = self.assigned(category.id, in: month)
        let activity = self.activity(category.id, in: month)
        let available = carried + assigned + activity

        var needed: Decimal = 0
        var progress: Double?
        switch category.goal {
        case .monthly(let target)?:
            needed = max(0, target - assigned)
            progress = target > 0 ? min(1, (assigned / target).doubleValue) : 1
        case .targetBalance(let target, let deadline)?:
            if let deadline {
                let monthsLeft = max(1, (deadline.year - month.year) * 12 + deadline.month - month.month + 1)
                let perMonth = max(0, target - carried) / Decimal(monthsLeft)
                needed = max(0, perMonth.rounded(scale: 2, mode: .up) - assigned)
            }
            progress = target > 0 ? min(1, max(0, (available / target).doubleValue)) : 1
        case nil:
            break
        }

        let status: CategoryStatus =
            if available < 0 { .overspent(-available) }
            else if needed > 0 { .underfunded(needed) }
            else if category.goal != nil { .funded }
            else if available == 0 { .zero }
            else { .positive }

        return CategorySummary(
            carriedOver: carried, assigned: assigned, activity: activity, available: available,
            neededThisMonth: needed, goalProgress: progress, status: status
        )
    }

    /// Categories that need attention this month, for the summary line.
    func attentionCounts(in month: Month) -> (overspent: Int, underfunded: Int) {
        var overspent = 0, underfunded = 0
        for category in categories {
            switch summary(for: category, in: month).status {
            case .overspent: overspent += 1
            case .underfunded: underfunded += 1
            default: break
            }
        }
        return (overspent, underfunded)
    }

    // MARK: - Assigning money

    func assign(_ amount: Decimal, to categoryID: Category.ID, in month: Month) {
        assignments[month, default: [:]][categoryID] = amount
    }

    /// Moves available money between categories by changing what is assigned.
    func move(_ amount: Decimal, from source: Category.ID, to destination: Category.ID, in month: Month) {
        assign(assigned(source, in: month) - amount, to: source, in: month)
        assign(assigned(destination, in: month) + amount, to: destination, in: month)
    }

    enum AutoAssign: String, CaseIterable, Identifiable {
        case underfunded, assignedLastMonth, spentLastMonth, reset

        var id: Self { self }

        var title: String {
            switch self {
            case .underfunded: "Underfunded"
            case .assignedLastMonth: "Assigned Last Month"
            case .spentLastMonth: "Spent Last Month"
            case .reset: "Reset Assigned Amounts"
            }
        }

        var systemImage: String {
            switch self {
            case .underfunded: "target"
            case .assignedLastMonth: "arrow.uturn.backward"
            case .spentLastMonth: "cart"
            case .reset: "arrow.counterclockwise"
            }
        }
    }

    /// The amount `strategy` would assign in total, shown next to each option.
    func preview(_ strategy: AutoAssign, in month: Month) -> Decimal {
        plan(strategy, in: month).reduce(0) { $0 + $1.value - assigned($1.key, in: month) }
    }

    func autoAssign(_ strategy: AutoAssign, in month: Month) {
        for (categoryID, amount) in plan(strategy, in: month) {
            assign(amount, to: categoryID, in: month)
        }
    }

    /// New assigned amounts, by category, for a strategy.
    private func plan(_ strategy: AutoAssign, in month: Month) -> [Category.ID: Decimal] {
        var result: [Category.ID: Decimal] = [:]
        switch strategy {
        case .underfunded:
            // Fund goals in budget order until Ready to Assign runs out.
            var remaining = max(0, readyToAssign(in: month))
            for category in categories {
                let summary = self.summary(for: category, in: month)
                let need = max(summary.neededThisMonth, -min(0, summary.available))
                guard need > 0, remaining > 0 else { continue }
                let amount = min(need, remaining)
                result[category.id] = summary.assigned + amount
                remaining -= amount
            }
        case .assignedLastMonth:
            for category in categories {
                result[category.id] = assigned(category.id, in: month.previous)
            }
        case .spentLastMonth:
            for category in categories {
                result[category.id] = max(0, -activity(category.id, in: month.previous))
            }
        case .reset:
            for category in categories { result[category.id] = 0 }
        }
        return result
    }

    func setGoal(_ goal: Goal?, for categoryID: Category.ID) {
        for g in groups.indices {
            if let c = groups[g].categories.firstIndex(where: { $0.id == categoryID }) {
                groups[g].categories[c].goal = goal
            }
        }
    }

    // MARK: - Editing transactions

    /// Adds a transaction (its `id` is ignored) and, for a transfer, the
    /// matching transaction in the other account. Returns the new id.
    @discardableResult
    func add(_ draft: Transaction) -> Transaction.ID {
        var transaction = draft
        transaction.id = nextID
        nextID += 1
        if let other = transaction.transferAccountID {
            let mirrorID = nextID
            nextID += 1
            transaction.transferID = mirrorID
            transactions.append(mirror(of: transaction, in: other, id: mirrorID))
        }
        transactions.append(transaction)
        return transaction.id
    }

    func update(_ transaction: Transaction) {
        guard let index = transactions.firstIndex(where: { $0.id == transaction.id }) else { return }
        let old = transactions[index]
        transactions[index] = transaction

        // Keep the other side of a transfer in step.
        if let mirrorID = old.transferID, transaction.transferAccountID == nil {
            transactions.removeAll { $0.id == mirrorID }
            transactions[transactions.firstIndex { $0.id == transaction.id }!].transferID = nil
        } else if let other = transaction.transferAccountID {
            let mirrorID = old.transferID ?? nextID
            if old.transferID == nil { nextID += 1 }
            transactions.removeAll { $0.id == mirrorID }
            var updated = transaction
            updated.transferID = mirrorID
            transactions[transactions.firstIndex { $0.id == transaction.id }!] = updated
            transactions.append(mirror(of: updated, in: other, id: mirrorID))
        }
    }

    private func mirror(of transaction: Transaction, in accountID: Account.ID, id: Transaction.ID) -> Transaction {
        let crossesBudget = account(accountID)?.isBudget != account(transaction.accountID)?.isBudget
        return Transaction(
            id: id,
            date: transaction.date,
            accountID: accountID,
            payee: transaction.payee,
            amount: -transaction.amount,
            // Only the budget side of a budget↔tracking transfer is categorised.
            categoryID: crossesBudget && account(accountID)?.isBudget == true ? transaction.categoryID : nil,
            transferAccountID: transaction.accountID,
            transferID: transaction.id,
            memo: transaction.memo,
            cleared: transaction.cleared,
            isApproved: true
        )
    }

    func delete(_ ids: Set<Transaction.ID>) {
        let mirrors = transactions.filter { ids.contains($0.id) }.compactMap(\.transferID)
        transactions.removeAll { ids.contains($0.id) || mirrors.contains($0.id) }
    }

    func duplicate(_ ids: Set<Transaction.ID>) {
        for transaction in transactions where ids.contains(transaction.id) {
            // Duplicating both sides of a transfer would make two transfers.
            if let mirror = transaction.transferID, ids.contains(mirror), transaction.amount > 0 { continue }
            var copy = transaction
            copy.date = today
            copy.cleared = .uncleared
            copy.isApproved = true
            copy.transferID = nil
            add(copy)
        }
    }

    func approve(_ ids: Set<Transaction.ID>) {
        for index in transactions.indices where ids.contains(transactions[index].id) {
            transactions[index].isApproved = true
        }
    }

    func setCleared(_ state: ClearedState, for ids: Set<Transaction.ID>) {
        for index in transactions.indices where ids.contains(transactions[index].id) {
            transactions[index].cleared = state
        }
    }

    func categorize(_ ids: Set<Transaction.ID>, as categoryID: Category.ID) {
        for index in transactions.indices where ids.contains(transactions[index].id) {
            guard !transactions[index].isSplit else { continue }
            transactions[index].categoryID = categoryID
        }
    }

    // MARK: - Accounts

    @discardableResult
    func addAccount(name: String, kind: AccountKind, balance: Decimal, bankID: BankConnection.ID?) -> Account.ID {
        var id = name.lowercased().filter { $0.isLetter || $0.isNumber }
        if id.isEmpty || account(id) != nil { id += "-\(accounts.count + 1)" }
        accounts.append(Account(id: id, name: name, kind: kind, bankID: bankID))
        add(Transaction(
            id: 0, date: today, accountID: id, payee: Transaction.startingBalancePayee,
            amount: balance, categoryID: kind.isBudget ? Category.readyToAssign : nil,
            cleared: .cleared
        ))
        return id
    }

    /// Locks every cleared transaction. If the bank's statement disagrees
    /// with the cleared balance, records the difference as an adjustment.
    func reconcile(_ accountID: Account.ID, statementBalance: Decimal) {
        let difference = statementBalance - clearedBalance(of: accountID)
        if difference != 0 {
            add(Transaction(
                id: 0, date: today, accountID: accountID, payee: Transaction.reconciliationPayee,
                amount: difference,
                categoryID: account(accountID)?.isBudget == true ? Category.readyToAssign : nil,
                cleared: .cleared
            ))
        }
        for index in transactions.indices
        where transactions[index].accountID == accountID && transactions[index].cleared == .cleared {
            transactions[index].cleared = .reconciled
        }
        if let index = accounts.firstIndex(where: { $0.id == accountID }) {
            accounts[index].lastReconciled = today
        }
    }

    /// Simulated sync: no network, so a sync just refreshes the timestamp
    /// and clears any connection problem.
    func sync(_ bankID: BankConnection.ID) {
        guard let index = banks.firstIndex(where: { $0.id == bankID }) else { return }
        banks[index].lastSynced = today
        banks[index].status = .connected
    }

    func syncAll() { banks.map(\.id).forEach(sync) }

    func disconnect(_ bankID: BankConnection.ID) {
        banks.removeAll { $0.id == bankID }
        for index in accounts.indices where accounts[index].bankID == bankID {
            accounts[index].bankID = nil
        }
    }

    // MARK: - Reports

    struct CategorySpending: Identifiable {
        var category: Category
        var group: CategoryGroup
        var amount: Decimal
        var id: Category.ID { category.id }
    }

    /// Spending (as a positive amount) by category over a range of months.
    func spendingByCategory(in months: ClosedRange<Month>) -> [CategorySpending] {
        groups.flatMap { group in
            group.categories.compactMap { category in
                var total: Decimal = 0
                var month = months.lowerBound
                while month <= months.upperBound {
                    total += activity(category.id, in: month)
                    month = month.next
                }
                return total < 0 ? CategorySpending(category: category, group: group, amount: -total) : nil
            }
        }
        .sorted { $0.amount > $1.amount }
    }

    struct MonthlyFlow: Identifiable {
        var month: Month
        var income: Decimal
        var spending: Decimal
        var id: Month { month }
        var net: Decimal { income - spending }
    }

    func incomeVsSpending(in months: ClosedRange<Month>) -> [MonthlyFlow] {
        var result: [MonthlyFlow] = []
        var month = months.lowerBound
        while month <= months.upperBound {
            var income: Decimal = 0
            for transaction in transactions where month.contains(transaction.date) && !transaction.isStartingBalance {
                for line in transaction.categoryLines where line.categoryID == Category.readyToAssign {
                    income += line.amount
                }
            }
            result.append(MonthlyFlow(month: month, income: income, spending: -totalActivity(in: month)))
            month = month.next
        }
        return result
    }

    struct NetWorthPoint: Identifiable {
        var date: Date
        var netWorth: Decimal
        var id: Date { date }
    }

    /// Net worth at the end of each day from the first transaction to today.
    func netWorthHistory() -> [NetWorthPoint] {
        let calendar = Calendar.budget
        let sorted = transactions.sorted { $0.date < $1.date }
        guard let first = sorted.first?.date else { return [] }
        var points: [NetWorthPoint] = []
        var day = calendar.startOfDay(for: first)
        var index = 0
        var running: Decimal = 0
        while day <= today {
            let end = calendar.date(byAdding: .day, value: 1, to: day)!
            while index < sorted.count, sorted[index].date < end {
                running += sorted[index].amount
                index += 1
            }
            points.append(NetWorthPoint(date: day, netWorth: running))
            day = end
        }
        return points
    }
}

// MARK: - Decimal helpers

extension Decimal {
    var doubleValue: Double { NSDecimalNumber(decimal: self).doubleValue }

    func rounded(scale: Int = 2, mode: NSDecimalNumber.RoundingMode = .plain) -> Decimal {
        var source = self
        var result = Decimal()
        NSDecimalRound(&result, &source, scale, mode)
        return result
    }
}
