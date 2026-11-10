// The budget as SwiftUI lists it, and nothing of Apple's but Foundation:
// the month's lines in their groups (Category Details' and the sheets'), which way
// Move Money goes, the transaction form's state, a register by day, and
// days as a picker holds them. Every rule is the core's (`money_core`:
// a line's `tone()`, `story`, `captions`, `meter()`, `isPayment()`; the
// accounts' `accountShelves`; the form's `Draft`; the asks' `follow`), so
// the phone, Android and the window say the same; the core's tests hold
// them (money/core/tests/words.rs).
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
extension AccountShelf: Identifiable { public var id: Shelving { shelving } }

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
    var isCards: Bool { group.isCards() }
    /// What of the cards' debt its categories do not cover (the core's).
    var uncovered: Int64 { Money.uncovered(lines: lines) }
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

    /// How many categories ask for attention this month, as a badge says it (the core's).
    static func attention(_ s: Summary?) -> Int { Int(s?.attention() ?? 0) }
}

// MARK: credit cards

enum Cards {
    /// The groups a transaction's category is chosen from: every one but
    /// the cards', whose categories are paid into by a transfer to the
    /// card, never chosen.
    static func choosable(_ groups: [GroupMonth]) -> [GroupMonth] {
        groups.filter { !$0.isCards }.map { GroupMonth(group: $0.group, lines: $0.lines.filter { !$0.isPayment() }) }
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


    /// The categories Move Money offers to move to or from, besides `id`:
    /// none whose month follows a rule, which the domain refuses either way.
    static func others(_ groups: [GroupMonth], besides id: Id?) -> [BudgetLine] {
        groups.flatMap(\.lines).filter { $0.categoryId != id && !$0.ruled }
    }
}

// MARK: assigned by rule

enum Ruled {
    /// The mark an amount its formula set wears (an SF Symbol).
    static let symbol = "function"
}

// MARK: the transaction form

/// A transaction as the form holds it while it is typed: the core's
/// `Draft` (its rules, `problem()`, and the change it is, `toChange()`),
/// and what the form shows beside it. The amount is typed unsigned, its
/// way (`inflow`) chosen apart.
@dynamicMemberLookup
struct TxnForm: Equatable {
    var draft: Draft
    /// How much, never below nothing; `inflow` says which way.
    var cents: Int64 = 0
    var inflow = false
    /// Who entered the one corrected.
    var by = ""
    /// What the bank called the one corrected, when it says more than its payee.
    var bank: String? = nil
    /// Whether the one corrected is a transfer already: it stays one, and
    /// only the account it is with is chosen again.
    var transfer = false

    /// A new one on `date`, in `account` or the usual one (the core's).
    init(account: Id?, accounts: [AccountLine], date: String) {
        draft = newDraft(accounts: accounts, account: account, date: date)
    }

    /// The form over a transaction already in a register.
    init(_ t: TxnLine) {
        draft = t.draft()
        cents = Int64(t.amount.magnitude)
        inflow = t.amount > 0
        by = t.userId
        bank = t.bankSaid()
        transfer = t.transfer
    }

    subscript<T>(dynamicMember path: WritableKeyPath<Draft, T>) -> T {
        get { draft[keyPath: path] }
        set { draft[keyPath: path] = newValue }
    }

    /// The draft as it is sent: its amount in the domain's sign.
    var sent: Draft {
        var d = draft
        d.amount = inflow ? cents : -cents
        return d
    }

    /// Why it cannot be saved yet, as the button's footnote says it (the core's).
    var problem: String? { sent.problem() }

    mutating func choose(_ p: PayeeLine) { draft = draft.withPayee(payee: p) }
    mutating func choose(category c: Id?) { draft = draft.withCategory(category: c) }
    mutating func choose(transfer a: Id) { draft = draft.withTransfer(account: a) }
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
}

// MARK: days

/// Days as the domain writes them (`2026-10-26`), and as a `Date` a picker
/// and a heading take: noon of that day in the phone's own calendar, which
/// a picker shows and gives back as the same day.
/// Money as the library writes it (ark_format, every client's), in what the
/// phone's locale says of money, read once from NumberFormatter.
enum Numbers {
    static let style: NumberStyle = {
        let n = LocaleNumbers.of()
        return NumberStyle(currency: n.currency, symbol: n.symbol, symbolFirst: n.symbolFirst, symbolSpaced: n.symbolSpaced,
                           decimal: n.decimal, grouping: n.grouping, minus: n.minus, minusFirst: n.minusFirst)
    }()

    /// Says how ArkUI's figures write cents: once, before the first view draws.
    static func install() {
        Cents.write = { cents, signed in formatAmount(cents: cents, withSign: signed, style: style) }
    }
}

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
