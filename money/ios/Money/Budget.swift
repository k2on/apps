// The budget's arithmetic as the screens need it, and nothing of Apple's
// but Foundation: the month's lines in their groups, what an available
// amount says (`Tone`), the cards' payment categories (`Cards`), the
// accounts in their sections, which way Move Money goes, the transaction
// form and its inputs, payees offered as one types, and days as the domain
// writes them. Every rule here is the desktop's (`money/iced`), so the
// phone and the window agree; all of it is held by `MoneyTests`, over the
// demo's household where it reads one.
import ArkUICore
import Foundation

// MARK: the rows, as SwiftUI lists them

extension AccountLine: Identifiable {}
extension GroupLine: Identifiable {}
extension TxnLine: Identifiable {}
extension BudgetLine: Identifiable { public var id: Id { categoryId } }
extension SpendingLine: Identifiable { public var id: Id { groupId } }
extension BankLine: Identifiable { public var id: String { name } }
extension PayeeLine: Identifiable { public var id: String { name } }
extension AskLine: Identifiable {}
extension LinkedLine: Identifiable { public var id: String { plaidAccountId } }

/// The budget's own ids: the same on every peer (`money_domain::budget::fixed`).
enum Fixed {
    /// Ready to Assign, the category money coming in is for: the first
    /// sixteen bytes of sha256("money ready to assign").
    static let readyToAssign = Id("332338b3e3e3aaa714a82657645453dc")
    /// The cards' group, "Credit Card Payments" (`money_domain::cards::card_group`):
    /// the first sixteen bytes of sha256("money credit card payments").
    static let cardGroup = Id("5aa01fc974fd8b643be86dcac41a33c7")
}

// MARK: a category's figure

/// What an available amount says: the colour of its pill. The desktop's `Tone`.
enum Tone: Equatable {
    /// Nothing, neither good nor bad.
    case zero
    /// Money with a job.
    case funded
    /// Short of what its target asks this month — or, a card's payment,
    /// of what the card owes.
    case short
    /// Below nothing: spent more cash than it had, which next month's
    /// Ready to Assign pays for.
    case over
    /// Below nothing, all of it on a card: new debt rather than cash gone.
    /// Amber, as YNAB draws it.
    case onCard

    static func of(available: Int64, target: Int64, assigned: Int64) -> Tone {
        if available < 0 { return .over }
        if assigned < target { return .short }
        return available == 0 ? .zero : .funded
    }

    /// A line's tone, the desktop's `tone`: overspending on a card apart
    /// from cash, and a card's payment short while it leaves debt uncovered.
    static func of(_ l: BudgetLine) -> Tone {
        if l.available < 0 && l.creditOverspent >= -l.available { return .onCard }
        if l.available >= 0 && l.card != nil && l.underfunded > 0 { return .short }
        return of(available: l.available, target: l.target, assigned: l.assigned)
    }
}

/// How far a category's month is toward its target, 0 to 1; none without one.
func funded(_ l: BudgetLine) -> Double? {
    guard l.target > 0 else { return nil }
    return min(max(Double(l.assigned) / Double(l.target), 0), 1)
}

/// What Ready to Assign says under itself.
func readyWords(_ s: Summary) -> String {
    if s.readyToAssign < 0 { return "You've assigned more than you have" }
    if s.readyToAssign == 0 { return "Every dollar has a job" }
    return "Give these dollars a job"
}

// MARK: the month, by group

/// One group of the budget and its categories in one month, with its totals.
struct GroupMonth: Identifiable, Equatable {
    let group: GroupLine
    let lines: [BudgetLine]
    var id: Id { group.id }
    var assigned: Int64 { lines.reduce(0) { $0 + $1.assigned } }
    var activity: Int64 { lines.reduce(0) { $0 + $1.activity } }
    var available: Int64 { lines.reduce(0) { $0 + $1.available } }
    /// The cards' own group, Credit Card Payments: named, filled and
    /// emptied as the cards are, never by hand.
    var isCards: Bool { Cards.isGroup(group.id) }
    /// What of the cards' debt its categories do not cover.
    var uncovered: Int64 { Cards.uncovered(lines) }
}

enum Months {
    /// The month's lines under their groups, in the budget's order; a
    /// hidden group or category only when `hidden` is asked for. A group
    /// with nothing in it is still drawn: it is where a category is added.
    static func groups(_ groups: [GroupLine], _ lines: [BudgetLine], hidden: Bool) -> [GroupMonth] {
        var by = [Id: [BudgetLine]]()
        for l in lines where hidden || !l.hidden { by[l.groupId, default: []].append(l) }
        return groups.filter { hidden || !$0.hidden }.map { GroupMonth(group: $0, lines: by[$0.id] ?? []) }
    }

    /// The month a day is in: `2026-10` of `2026-10-26`.
    static func of(_ day: String) -> String { String(day.prefix(7)) }

    /// How many categories ask for attention this month, as a badge says it.
    static func attention(_ s: Summary?) -> Int { Int((s?.overspent ?? 0) + (s?.underfunded ?? 0)) }
}

// MARK: credit cards

/// What a line says of a card under its name: the desktop's `progress`.
enum CardWord: Equatable {
    /// A card's payment that leaves this much of its debt uncovered.
    case short(Int64)
    /// A card's payment that has all the card owes set aside.
    case covered
    /// A category that spent this much more on a card than it had: debt.
    case onCard(Int64)
}

/// The budget's cards as the screens draw them (`money_domain::cards`): a
/// card on the budget has a payment category in the cards' group, first in
/// the budget, which holds the money set aside for it. Spending on the
/// card from a category that has the money moves it there; spending more
/// than a category has is credit overspending — debt, which the payment
/// category shows as not covered until money is assigned to it.
enum Cards {
    static func isGroup(_ id: Id) -> Bool { id == Fixed.cardGroup }

    /// Whether a line is a card's payment category: the cards', named and
    /// kept as its card is, and no transaction's category.
    static func isPayment(_ l: BudgetLine) -> Bool { l.card != nil || isGroup(l.groupId) }

    static func word(_ l: BudgetLine) -> CardWord? {
        if isPayment(l) {
            if l.underfunded > 0 { return .short(l.underfunded) }
            return l.debt > 0 ? .covered : nil
        }
        return l.creditOverspent > 0 ? .onCard(l.creditOverspent) : nil
    }

    /// How much of what the card owes is set aside, 0 to 1; none when it owes nothing.
    static func covered(_ l: BudgetLine) -> Double? {
        guard isPayment(l), l.debt > 0 else { return nil }
        return min(max(Double(l.debt - l.underfunded) / Double(l.debt), 0), 1)
    }

    /// What assigning a card's payment category covers its debt with: what
    /// it was given and what it lacks; none when nothing is uncovered.
    static func toCover(_ l: BudgetLine) -> Int64? {
        guard isPayment(l), l.underfunded > 0 else { return nil }
        return l.assigned + l.underfunded
    }

    /// What of the cards' debt their payment categories do not cover.
    static func uncovered(_ lines: [BudgetLine]) -> Int64 { lines.filter(isPayment).reduce(0) { $0 + $1.underfunded } }

    /// The groups a transaction's category is chosen from: every one but
    /// the cards', whose categories are paid into by a transfer to the
    /// card, never chosen.
    static func choosable(_ groups: [GroupMonth]) -> [GroupMonth] {
        groups.filter { !$0.isCards }.map { GroupMonth(group: $0.group, lines: $0.lines.filter { !isPayment($0) }) }
    }
}

// MARK: the budget rearranged

/// A row of the budget as Edit lists it: one list, each group a row over
/// its categories, so a category is dragged out of one group into another
/// and a group past the others, as Reminders' lists are into and out of
/// their groups.
enum Arranged: Identifiable {
    case group(GroupLine)
    case category(BudgetLine)

    var id: String {
        switch self {
        case let .group(g): return "group " + g.id.hex
        case let .category(l): return "category " + l.categoryId.hex
        }
    }

    var isGroup: Bool { if case .group = self { return true } else { return false } }
}

/// What a row let go somewhere new in Edit asks of the budget.
enum Rearrange: Equatable {
    case category(MoveCategory)
    case group(MoveGroup)
}

enum Arranging {
    /// The groups on screen as Edit lists them, each over its categories.
    static func rows(_ groups: [GroupMonth]) -> [Arranged] {
        groups.flatMap { [Arranged.group($0.group)] + $0.lines.map(Arranged.category) }
    }

    /// The row at `from` let go in front of the row at `to`, both counted as
    /// `onMove` counts them (before the row was lifted; past the last is the
    /// end). A category goes into the group it is now under — let go above
    /// every group, the first it may join — in front of the category after
    /// it, or last; a group goes in front of the next group, or last, its
    /// categories with it, wherever among another's it was let go. None when
    /// that moves nothing, or when it would take a card's payment category
    /// out of the cards' group or put another among them (the domain's
    /// "a card's payment category stays with the cards").
    static func drop(_ rows: [Arranged], from: Int, to: Int) -> Rearrange? {
        guard rows.indices.contains(from) else { return nil }
        let row = rows[from]
        var after = Order.moved(rows, from: IndexSet(integer: from), to: to)
        guard var at = after.firstIndex(where: { $0.id == row.id }) else { return nil }
        switch row {
        case let .category(l):
            let cards = Cards.isPayment(l)
            if !after[..<at].contains(where: \.isGroup) {
                after.remove(at: at)
                guard let first = after.firstIndex(where: { r in
                    if case let .group(g) = r { return Cards.isGroup(g.id) == cards }
                    return false
                }) else { return nil }
                at = first + 1
                after.insert(row, at: at)
            }
            guard after.map(\.id) != rows.map(\.id), case let .group(g)? = after[..<at].last(where: \.isGroup),
                  Cards.isGroup(g.id) == cards
            else { return nil }
            var before: Id? = nil
            if at + 1 < after.count, case let .category(next) = after[at + 1] { before = next.categoryId }
            return .category(MoveCategory(id: l.categoryId, groupId: g.id, before: before))
        case let .group(g):
            let order = groupIds(after)
            guard order != groupIds(rows), let i = order.firstIndex(of: g.id) else { return nil }
            return .group(MoveGroup(id: g.id, before: i + 1 < order.count ? order[i + 1] : nil))
        }
    }

    /// The groups on screen as they will be once `r` is done — the domain's
    /// `move_category` and `move_group` over what is shown — drawn while the
    /// session does it, so a dropped row never jumps back on the way.
    static func applied(_ r: Rearrange, to groups: [GroupMonth]) -> [GroupMonth] {
        switch r {
        case let .category(m):
            guard let line = groups.flatMap(\.lines).first(where: { $0.categoryId == m.id }) else { return groups }
            return groups.map { g in
                var lines = g.lines.filter { $0.categoryId != m.id }
                if g.id == m.groupId {
                    let at = m.before.flatMap { b in lines.firstIndex { $0.categoryId == b } } ?? lines.count
                    lines.insert(line, at: at)
                }
                return GroupMonth(group: g.group, lines: lines)
            }
        case let .group(m):
            guard let g = groups.first(where: { $0.id == m.id }) else { return groups }
            var rest = groups.filter { $0.id != m.id }
            let at = m.before.flatMap { b in rest.firstIndex { $0.id == b } } ?? rest.count
            rest.insert(g, at: at)
            return rest
        }
    }

    /// The groups' order and each one's categories', as the rows name it:
    /// what says the session has caught up with what was drawn ahead.
    static func shape(_ groups: [GroupMonth]) -> [String] { rows(groups).map(\.id) }

    private static func groupIds(_ rows: [Arranged]) -> [Id] {
        rows.compactMap { if case let .group(g) = $0 { return g.id } else { return nil } }
    }
}

// MARK: moving money

enum Moves {
    /// Money moved between `line` and `other` in `month` (either none for
    /// Ready to Assign): out of `line` to `other`, unless `line` is
    /// overspent — then `other` covers it, and the money comes *from* it.
    /// The desktop's `Choosing::Move`.
    static func of(month: String, line: BudgetLine?, other: Id?, cents: Int64) -> MoveMoney {
        let from = line?.categoryId
        if let l = line, l.available < 0 { return MoveMoney(month: month, from: other, to: from, amount: cents) }
        return MoveMoney(month: month, from: from, to: other, amount: cents)
    }

    /// What the move sheet offers first: an overspent category's shortfall,
    /// or all a category has.
    static func suggested(_ line: BudgetLine?) -> Int64 { line.map { Int64($0.available.magnitude) } ?? 0 }

    /// What assigning `line` to its target takes this month.
    static func toTarget(_ l: BudgetLine) -> Int64 { max(l.target - l.assigned, 0) }
}

// MARK: the accounts

/// The sidebar's sections, as the Accounts tab lists them.
enum Shelving: String, CaseIterable {
    case budget = "Budget"
    case tracking = "Tracking"
    case closed = "Closed"
}

struct AccountShelf: Identifiable, Equatable {
    let kind: Shelving
    let rows: [AccountLine]
    var id: String { kind.rawValue }
    var total: Int64 { rows.reduce(0) { $0 + $1.balance } }
}

enum Accounts {
    /// The kinds an account can be, in the order a form offers them, with
    /// what to call each (`money_domain::accounts::KINDS`).
    static let kinds: [(kind: String, name: String)] = [
        ("checking", "Checking"), ("savings", "Savings"), ("cash", "Cash"),
        ("credit", "Credit Card"), ("loan", "Loan or Mortgage"), ("investment", "Investment"),
    ]

    static func name(ofKind k: String) -> String { kinds.first { $0.kind == k }?.name ?? k.capitalized }

    /// Whether an account of `kind` is in the budget unless somebody says otherwise.
    static func onBudget(byDefault kind: String) -> Bool { !["loan", "investment"].contains(kind) }

    /// Whether the balance of an account of `kind` is what is owed on it.
    static func owes(_ kind: String) -> Bool { ["credit", "loan"].contains(kind) }

    /// The SF Symbol an account of `kind` is drawn with.
    static func symbol(_ kind: String) -> String {
        switch kind {
        case "checking": return "building.columns.fill"
        case "savings": return "dollarsign.circle.fill"
        case "cash": return "banknote.fill"
        case "credit": return "creditcard.fill"
        case "loan": return "house.fill"
        case "investment": return "chart.line.uptrend.xyaxis"
        default: return "building.columns.fill"
        }
    }

    /// Open accounts on budget, then tracking ones, then the closed; a
    /// section with nothing in it is left out.
    static func shelves(_ rows: [AccountLine]) -> [AccountShelf] {
        let ordered = rows.sorted { ($0.pos, $0.name) < ($1.pos, $1.name) }
        return Shelving.allCases.map { s in
            AccountShelf(kind: s, rows: ordered.filter { a in
                switch s {
                case .budget: return !a.closed && a.onBudget
                case .tracking: return !a.closed && !a.onBudget
                case .closed: return a.closed
                }
            })
        }.filter { !$0.rows.isEmpty }
    }

    /// Everything the household has less everything it owes: the open accounts' balances.
    static func netWorth(_ rows: [AccountLine]) -> Int64 { rows.filter { !$0.closed }.reduce(0) { $0 + $1.balance } }

    /// The account a new transaction goes to when none is chosen: the first open one on budget.
    static func usual(_ rows: [AccountLine]) -> AccountLine? { shelves(rows).first { $0.kind == .budget }?.rows.first }

    /// What the account's line says under its name: the bank and its last digits, or its kind.
    static func subtitle(_ a: AccountLine) -> String {
        if !a.bank.isEmpty { return a.mask.isEmpty ? a.bank.capitalized : "\(a.bank.capitalized) ••\(a.mask)" }
        return name(ofKind: a.kind)
    }
}

// MARK: the transaction form

/// A transaction as the form holds it while it is typed: a new one, or one being corrected.
struct Draft: Equatable {
    /// The one corrected; none for a new one.
    var editing: Id? = nil
    var account: Id? = nil
    var date: String
    var payee = ""
    var category: Id? = nil
    /// The other account, for a transfer: the payee is then that account.
    var transferTo: Id? = nil
    var memo = ""
    /// How much, never below nothing; `inflow` says which way.
    var cents: Int64 = 0
    var inflow = false
    var cleared = false
    /// Who entered the one corrected.
    var by = ""
    /// What the bank called the one corrected, when it says more than its payee.
    var bank: String? = nil
    /// Whether the one corrected is a transfer already: it stays one, and
    /// only the account it is with is chosen again.
    var transfer = false

    init(account: Id?, date: String) {
        self.account = account
        self.date = date
    }

    /// The form over a transaction already in the register; `accounts` say
    /// which a transfer is with.
    init(_ t: TxnLine, accounts: [AccountLine] = []) {
        editing = t.id
        account = t.accountId
        date = t.date
        payee = t.payee
        category = t.categoryId
        memo = t.memo
        cents = Int64(t.amount.magnitude)
        inflow = t.amount > 0
        cleared = t.cleared != .uncleared
        by = t.userId
        bank = Registers.bankDescription(t)
        transfer = t.transfer
        if t.transfer { transferTo = Registers.transferAccount(t, accounts) }
    }

    /// The amount in the domain's sign: spending below nothing.
    var amount: Int64 { inflow ? cents : -cents }

    /// Why it cannot be saved yet, as the button's footnote says it; none when it can.
    var problem: String? {
        if account == nil { return "Choose an account." }
        if cents == 0 { return "Enter an amount." }
        if Days.date(date) == nil { return "Choose a day." }
        return nil
    }

    /// A payee chosen from what the field offered: its last category comes with it, unless one is chosen.
    mutating func choose(_ p: PayeeLine) {
        payee = p.name
        if category == nil && transferTo == nil { category = p.categoryId }
    }

    /// The category chosen, or a transfer: one excludes the other.
    mutating func choose(category c: Id?) { category = c; transferTo = nil }
    mutating func choose(transfer a: Id) { transferTo = a; category = nil }

    var adding: AddTransaction? {
        guard editing == nil, let a = account else { return nil }
        return AddTransaction(accountId: a, date: date, payee: payee.trimmingCharacters(in: .whitespaces),
                              categoryId: transferTo == nil ? category : nil, memo: memo, amount: amount,
                              cleared: cleared, transferTo: transferTo)
    }

    var edit: EditTransaction? {
        guard let id = editing else { return nil }
        // A transfer chosen for one that was none makes it one, paired with
        // the matching transaction there or given its other half (the
        // domain's `transfers::make`); the same account again changes nothing.
        return EditTransaction(id: id, date: date, payee: payee.trimmingCharacters(in: .whitespaces), categoryId: category,
                               memo: memo, amount: amount, transferTo: transferTo)
    }
}

enum Payees {
    /// Who the payee field offers while `typed` is typed: names that start
    /// with it first, then ones that contain it, the most paid first; none
    /// once the name is typed whole.
    static func suggest(_ typed: String, _ payees: [PayeeLine], limit: Int = 4) -> [PayeeLine] {
        let t = typed.trimmingCharacters(in: .whitespaces).lowercased()
        if t.isEmpty { return Array(payees.sorted { $0.count > $1.count }.prefix(limit)) }
        if payees.contains(where: { $0.name.lowercased() == t }) { return [] }
        let starts = payees.filter { $0.name.lowercased().hasPrefix(t) }
        let within = payees.filter { !$0.name.lowercased().hasPrefix(t) && $0.name.lowercased().contains(t) }
        return Array((starts.sorted { $0.count > $1.count } + within.sorted { $0.count > $1.count }).prefix(limit))
    }
}

// MARK: registers

/// One day of a register, as its list's section heads it.
struct Day: Identifiable, Equatable {
    let day: String
    let rows: [TxnLine]
    var id: String { day }
}

enum Registers {
    /// The register's rows a day at a time, newest first (the view's own order).
    static func days(_ rows: [TxnLine]) -> [Day] {
        var out = [Day]()
        for r in rows {
            if out.last?.day == r.date { out[out.count - 1] = Day(day: r.date, rows: out[out.count - 1].rows + [r]) }
            else { out.append(Day(day: r.date, rows: [r])) }
        }
        return out
    }

    /// The rows a search finds: its words in the payee, the category, the memo or the amount.
    static func search(_ rows: [TxnLine], _ needle: String) -> [TxnLine] {
        let n = needle.trimmingCharacters(in: .whitespaces).lowercased()
        if n.isEmpty { return rows }
        return rows.filter { r in
            r.payee.lowercased().contains(n) || r.category.lowercased().contains(n) || r.memo.lowercased().contains(n)
                || Cents.plain(r.amount).contains(n) || r.account.lowercased().contains(n)
        }
    }

    /// Whether a line has a category it can be approved with.
    static func needsCategory(_ t: TxnLine) -> Bool { t.category == "Needs a category" }

    /// What the bank called an imported line, as its form shows it under the
    /// payee: none for one typed in, one the bank said nothing of, or one
    /// whose payee already says it (`CHIPOTLE` for Chipotle).
    static func bankDescription(_ t: TxnLine) -> String? {
        let d = t.importedPayee.trimmingCharacters(in: .whitespacesAndNewlines)
        let payee = t.payee.trimmingCharacters(in: .whitespacesAndNewlines)
        guard t.imported, !d.isEmpty, d.caseInsensitiveCompare(payee) != .orderedSame else { return nil }
        return d
    }

    /// The account a transfer's line is with, as its payee names it
    /// (`Transfer : Savings`, the domain's `txn_line`): an open one before a
    /// closed one of the same name; none for a line that is no transfer.
    static func transferAccount(_ t: TxnLine, _ accounts: [AccountLine]) -> Id? {
        let prefix = "Transfer : "
        guard t.transfer, t.payee.hasPrefix(prefix) else { return nil }
        let name = String(t.payee.dropFirst(prefix.count))
        let named = accounts.filter { $0.name == name && $0.id != t.accountId }
        return (named.first { !$0.closed } ?? named.first)?.id
    }

    /// The cleared, uncleared and working balances of an account's register.
    static func balances(_ a: AccountLine) -> (cleared: Int64, uncleared: Int64, working: Int64) { (a.cleared, a.uncleared, a.balance) }
}

// MARK: days

/// Days as the domain writes them (`2026-10-26`), and as a `Date` a picker
/// and a heading take: noon of that day in the phone's own calendar, which
/// a picker shows and gives back as the same day.
enum Days {
    static func date(_ day: String, calendar cal: Calendar = .current) -> Date? {
        let p = day.split(separator: "-").map { Int($0) }
        guard p.count == 3, let y = p[0], let m = p[1], let d = p[2], (1...12).contains(m), (1...31).contains(d) else { return nil }
        let c = DateComponents(year: y, month: m, day: d, hour: 12)
        guard let date = cal.date(from: c), cal.component(.day, from: date) == d else { return nil }
        return date
    }

    /// The day a picker's `Date` is, in the phone's own calendar.
    static func day(_ date: Date, calendar: Calendar = .current) -> String {
        let c = calendar.dateComponents([.year, .month, .day], from: date)
        return String(format: "%04d-%02d-%02d", c.year ?? 1970, c.month ?? 1, c.day ?? 1)
    }

    /// The first day of a month (`2026-10`) and the last.
    static func first(_ month: String) -> Date? { date(month + "-01") }
}
