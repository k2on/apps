import Foundation

/// A deterministic sample household: two people, six accounts, fifteen
/// categories and about a hundred transactions from September to the middle
/// of November 2026. Every name, payee and institution is invented.
///
/// Nothing here depends on the clock: "today" is fixed, and amounts vary by
/// a fixed table rather than a random generator, so every launch and every
/// screenshot is identical.
enum SampleData {
    /// The sample household's "today".
    static let today = date(2026, 11, 18, hour: 15)

    static func makeStore() -> BudgetStore {
        var builder = Builder()
        builder.build()

        let store = BudgetStore(
            household: household,
            accounts: accounts,
            groups: groups,
            transactions: builder.finishedTransactions(today: today),
            assignments: [:],
            banks: banks,
            currencyCode: "USD",
            today: today
        )
        assignBudget(in: store)
        return store
    }

    // MARK: - Household, banks and accounts

    static let household = Household(
        name: "Alder Household",
        members: [
            Member(id: "maya", name: "Maya Alder", email: "maya@example.com", role: .owner),
            Member(id: "theo", name: "Theo Alder", email: "theo@example.com", role: .member),
            Member(id: "june", name: "June Alder", email: "june@example.com", role: .invited),
        ]
    )

    static let banks: [BankConnection] = [
        BankConnection(id: "harbor", institution: "Harbor Credit Union",
                       lastSynced: date(2026, 11, 18, hour: 13), status: .connected),
        BankConnection(id: "summit", institution: "Summit Card Services",
                       lastSynced: date(2026, 11, 18, hour: 9), status: .connected),
        BankConnection(id: "juniper", institution: "Juniper Investments",
                       lastSynced: date(2026, 11, 12, hour: 8),
                       status: .needsAttention("Sign in again to keep syncing.")),
    ]

    static let accounts: [Account] = [
        Account(id: "checking", name: "Everyday Checking", kind: .checking, bankID: "harbor",
                lastReconciled: date(2026, 10, 31)),
        Account(id: "savings", name: "High-Yield Savings", kind: .savings, bankID: "harbor"),
        Account(id: "cash", name: "Wallet", kind: .cash),
        Account(id: "visa", name: "Rewards Visa", kind: .creditCard, bankID: "summit"),
        Account(id: "mortgage", name: "Home Mortgage", kind: .mortgage),
        Account(id: "retirement", name: "Retirement Fund", kind: .investment, bankID: "juniper"),
    ]

    // MARK: - Categories

    static let groups: [CategoryGroup] = [
        CategoryGroup(id: "bills", name: "Bills", categories: [
            Category(id: "mortgage", name: "Mortgage", emoji: "🏠", goal: .monthly(1850)),
            Category(id: "electricity", name: "Electricity", emoji: "⚡️", goal: .monthly(125)),
            Category(id: "water", name: "Water", emoji: "💧", goal: .monthly(50)),
            Category(id: "internet", name: "Internet", emoji: "🌐", goal: .monthly(70)),
            Category(id: "phone", name: "Phone", emoji: "📱", goal: .monthly(85)),
            Category(id: "insurance", name: "Insurance", emoji: "☂️", goal: .monthly(145)),
        ]),
        CategoryGroup(id: "everyday", name: "Everyday", categories: [
            Category(id: "groceries", name: "Groceries", emoji: "🛒", goal: .monthly(650)),
            Category(id: "dining", name: "Dining Out", emoji: "🍜", goal: .monthly(200)),
            Category(id: "transport", name: "Transportation", emoji: "🚆", goal: .monthly(160)),
            Category(id: "household", name: "Household", emoji: "🧺", goal: nil),
            Category(id: "fun", name: "Fun Money", emoji: "🎟️", goal: .monthly(60)),
        ]),
        CategoryGroup(id: "goals", name: "Savings Goals", categories: [
            Category(id: "emergency", name: "Emergency Fund", emoji: "🛟", goal: .targetBalance(25_000, by: nil)),
            Category(id: "vacation", name: "Summer Vacation", emoji: "🏖️",
                     goal: .targetBalance(3_000, by: Month(year: 2027, month: 6))),
            Category(id: "laptop", name: "New Laptop", emoji: "💻",
                     goal: .targetBalance(1_800, by: Month(year: 2027, month: 3))),
            Category(id: "gifts", name: "Holiday Gifts", emoji: "🎁",
                     goal: .targetBalance(600, by: Month(year: 2026, month: 12))),
        ]),
    ]

    // MARK: - Assignments

    /// Assigns the same plan each month, then puts whatever is left into the
    /// Emergency Fund so Ready to Assign ends each month at a known amount:
    /// zero in past months, $412.37 this month.
    private static func assignBudget(in store: BudgetStore) {
        let plan: [Category.ID: Decimal] = [
            "mortgage": 1850, "electricity": 125, "water": 50, "internet": 70, "phone": 85,
            "insurance": 145, "groceries": 650, "dining": 200, "transport": 160,
            "household": 100, "fun": 60, "vacation": 300, "laptop": 300, "gifts": 150,
        ]
        var month = store.firstMonth
        while month <= store.currentMonth {
            for (category, amount) in plan { store.assign(amount, to: category, in: month) }
            if month == store.currentMonth {
                // This month is still being planned: dining was cut back
                // (and is now overspent), vacation and gifts are behind.
                store.assign(60, to: "dining", in: month)
                store.assign(150, to: "vacation", in: month)
                store.assign(0, to: "gifts", in: month)
            }
            store.assign(0, to: "emergency", in: month)
            let leave: Decimal = month == store.currentMonth ? 412.37 : 0
            store.assign(store.readyToAssign(in: month) - leave, to: "emergency", in: month)
            month = month.next
        }
    }

    // MARK: - Transactions

    private struct Builder {
        var transactions: [Transaction] = []
        private var nextID = 1
        private var variation = 0

        /// Fixed multipliers so recurring amounts look natural but never change.
        private static let factors: [Decimal] = [
            1.00, 0.82, 1.17, 0.93, 1.29, 0.71, 1.08, 0.88, 1.22, 0.97, 1.11, 0.76,
        ]

        mutating func vary(_ base: Decimal) -> Decimal {
            defer { variation += 1 }
            return (base * Self.factors[variation % Self.factors.count]).rounded(scale: 2)
        }

        @discardableResult
        mutating func add(
            _ date: Date, _ account: Account.ID, _ payee: String, _ amount: Decimal,
            _ category: Category.ID?, memo: String = "", splits: [Split] = [], receipt: Receipt? = nil
        ) -> Transaction.ID {
            let id = nextID
            nextID += 1
            transactions.append(Transaction(
                id: id, date: date, accountID: account, payee: payee, amount: amount,
                categoryID: splits.isEmpty ? category : nil, splits: splits, memo: memo, receipt: receipt
            ))
            return id
        }

        mutating func transfer(
            _ date: Date, from source: Account.ID, to destination: Account.ID, _ amount: Decimal,
            category: Category.ID? = nil, memo: String = ""
        ) {
            let outID = add(date, source, "Transfer", -amount, category, memo: memo)
            let inID = add(date, destination, "Transfer", amount, nil, memo: memo)
            for index in transactions.indices {
                if transactions[index].id == outID {
                    transactions[index].transferAccountID = destination
                    transactions[index].transferID = inID
                } else if transactions[index].id == inID {
                    transactions[index].transferAccountID = source
                    transactions[index].transferID = outID
                }
            }
        }

        mutating func build() {
            let start = SampleData.date(2026, 9, 1, hour: 0)
            add(start, "checking", Transaction.startingBalancePayee, 4_210.55, Category.readyToAssign)
            add(start, "savings", Transaction.startingBalancePayee, 12_400.00, Category.readyToAssign)
            add(start, "cash", Transaction.startingBalancePayee, 160.00, Category.readyToAssign)
            add(start, "visa", Transaction.startingBalancePayee, -842.17, Category.readyToAssign)
            add(start, "mortgage", Transaction.startingBalancePayee, -318_450.00, nil)
            add(start, "retirement", Transaction.startingBalancePayee, 48_920.00, nil)

            let marketChanges: [Decimal] = [612.40, -248.15, 405.80]
            for (index, month) in [9, 10, 11].enumerated() {
                let last = month == 11 ? 18 : 31
                func on(_ day: Int) -> Date? {
                    day <= last && day <= SampleData.daysIn(month) ? SampleData.date(2026, month, day) : nil
                }

                // Income
                for day in [1, 15] {
                    if let d = on(day) { add(d, "checking", "Lumen Studio Payroll", 2_340.00, Category.readyToAssign, memo: "Maya’s pay") }
                }
                if let d = on(21) { add(d, "checking", "Riverside Clinic Payroll", 3_250.00, Category.readyToAssign, memo: "Theo’s pay") }
                if let d = on(28) { add(d, "savings", "Interest Paid", 18.20, Category.readyToAssign) }

                // Bills
                if let d = on(1) {
                    transfer(d, from: "checking", to: "mortgage", 1_850.00, category: "mortgage", memo: "Monthly payment")
                    add(d, "mortgage", "Harbor Home Loans", -1_236.40, nil, memo: "Interest")
                }
                if let d = on(2) {
                    transfer(d, from: "checking", to: "savings", 500.00, memo: "Monthly saving")
                    transfer(d, from: "checking", to: "cash", 100.00, memo: "ATM withdrawal")
                }
                if let d = on(3) { add(d, "checking", "Evergreen Insurance", -142.18, "insurance", memo: "Home & auto") }
                if let d = on(5) { add(d, "visa", "Brightwave Internet", -69.99, "internet") }
                if let d = on(9) { add(d, "visa", "Cove Mobile", -84.50, "phone") }
                if let d = on(12) { let amount = vary(108); add(d, "checking", "City Power & Light", -amount, "electricity") }
                if let d = on(20) { add(d, "checking", "Municipal Water", -46.75, "water") }

                // Everyday
                for day in [4, 11, 17, 25] {
                    guard let d = on(day) else { continue }
                    if month == 11 && day == 11 {
                        addSplitGroceryRun(d)
                    } else {
                        let amount = vary(128)
                        add(d, "visa", "Greenleaf Market", -amount, "groceries")
                    }
                }
                for day in [8, 22] {
                    if let d = on(day) { let amount = vary(36); add(d, "checking", "Corner Grocer", -amount, "groceries") }
                }
                if let d = on(9) { add(d, "cash", "Saturday Farmers Market", -28.00, "groceries") }
                let restaurants = ["Saffron Kitchen", "Blue Door Café", "Noodle Bar", "Saffron Kitchen"]
                for (offset, day) in [6, 14, 19, 27].enumerated() {
                    if let d = on(day) { let amount = vary(46); add(d, "visa", restaurants[offset], -amount, "dining") }
                }
                if let d = on(15) { add(d, "cash", "Daily Grind Coffee", -12.40, "dining") }
                for day in [10, 24] {
                    if let d = on(day) { let amount = vary(51); add(d, "visa", "Coastline Fuel", -amount, "transport") }
                }
                if let d = on(16) { add(d, "checking", "Metro Transit", -40.00, "transport", memo: "Monthly pass") }
                if let d = on(13) { let amount = vary(58); add(d, "visa", "Hardware Depot", -amount, "household") }
                if let d = on(26) { let amount = vary(34); add(d, "visa", "Home Goods Co.", -amount, "household") }
                if let d = on(18) { add(d, "visa", "Cinema 8", -31.50, "fun") }
                if let d = on(23) { let amount = vary(22); add(d, "visa", "Paper Lantern Books", -amount, "fun") }

                // Paying the card, and the investment account drifting.
                if let d = on(28) { transfer(d, from: "checking", to: "visa", month == 9 ? 1_420.00 : 1_310.00, memo: "Card payment") }
                if let d = on(30) { add(d, "retirement", "Market Change", marketChanges[index], nil) }
            }

            // One-offs.
            add(SampleData.date(2026, 10, 3), "visa", "Seaside Rail", -186.00, "vacation", memo: "Train tickets for June")
            add(SampleData.date(2026, 10, 30), "checking", "Paper Lantern Books", -38.00, "gifts", memo: "Early gift shopping")
            add(SampleData.date(2026, 11, 17), "visa", "SQ *Harbor Bakery", -18.60, nil)
        }

        /// A grocery run split between Groceries and Household, with an
        /// itemised receipt: the transaction worth showing in detail.
        mutating func addSplitGroceryRun(_ d: Date) {
            let groceries: [ReceiptItem] = [
                ReceiptItem(name: "Organic milk", quantity: 2, price: 4.29),
                ReceiptItem(name: "Sourdough loaf", price: 6.50),
                ReceiptItem(name: "Chicken thighs", price: 14.82),
                ReceiptItem(name: "Salmon fillet", price: 18.40),
                ReceiptItem(name: "Seasonal vegetables", price: 21.35),
                ReceiptItem(name: "Apples", quantity: 6, price: 0.95),
                ReceiptItem(name: "Greek yogurt", quantity: 2, price: 5.25),
                ReceiptItem(name: "Coffee beans", price: 10.55),
            ]
            let household: [ReceiptItem] = [
                ReceiptItem(name: "Dish soap", price: 4.95),
                ReceiptItem(name: "Paper towels", price: 9.80),
                ReceiptItem(name: "Sponges", quantity: 2, price: 3.70),
            ]
            let groceryTotal = groceries.reduce(Decimal(0)) { $0 + $1.total }
            let householdTotal = household.reduce(Decimal(0)) { $0 + $1.total }
            add(d, "visa", "Greenleaf Market", -(groceryTotal + householdTotal), nil,
                memo: "Weekly shop",
                splits: [
                    Split(categoryID: "groceries", amount: -groceryTotal, memo: "Food"),
                    Split(categoryID: "household", amount: -householdTotal, memo: "Cleaning"),
                ],
                receipt: Receipt(items: groceries + household))
        }

        /// Sorts by date, renumbers from 1 (oldest first) and sets the review
        /// state a bank feed would leave: older transactions cleared and
        /// approved; the last few days imported and waiting for review.
        func finishedTransactions(today: Date) -> [Transaction] {
            let sorted = transactions.enumerated()
                .sorted { ($0.element.date, $0.offset) < ($1.element.date, $1.offset) }
                .map(\.element)
            var newIDs: [Transaction.ID: Transaction.ID] = [:]
            for (index, transaction) in sorted.enumerated() { newIDs[transaction.id] = index + 1 }

            let connected: Set<Account.ID> = Set(SampleData.accounts.filter { $0.bankID != nil }.map(\.id))
            let reviewFrom = SampleData.date(2026, 11, 15, hour: 0)
            let clearedBefore = SampleData.date(2026, 11, 14, hour: 0)
            let reconciledBefore = SampleData.date(2026, 11, 1, hour: 0)

            return sorted.map { original in
                var transaction = original
                transaction.id = newIDs[original.id]!
                transaction.transferID = original.transferID.flatMap { newIDs[$0] }

                let imported = connected.contains(transaction.accountID)
                if imported && !transaction.isTransfer && transaction.date >= reviewFrom {
                    transaction.isApproved = false
                    transaction.cleared = .cleared
                    // The bank couldn't match these payees to a category.
                    if ["Metro Transit", "Cinema 8", "SQ *Harbor Bakery"].contains(transaction.payee) {
                        transaction.categoryID = nil
                    }
                } else if transaction.date < reconciledBefore && transaction.accountID == "checking" {
                    transaction.cleared = .reconciled
                } else {
                    transaction.cleared = transaction.date < clearedBefore ? .cleared : .uncleared
                }
                return transaction
            }
        }
    }

    // MARK: - Dates

    static func date(_ year: Int, _ month: Int, _ day: Int, hour: Int = 12) -> Date {
        Calendar.budget.date(from: DateComponents(year: year, month: month, day: day, hour: hour))!
    }

    private static func daysIn(_ month: Int) -> Int {
        Calendar.budget.range(of: .day, in: .month, for: date(2026, month, 1))!.count
    }
}
