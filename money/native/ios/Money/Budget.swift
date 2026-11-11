// The budget as SwiftUI lists it, and nothing of Apple's but Foundation:
// the month's lines in their groups (Insights'), money as the library
// writes it, and days as a picker holds them. Every rule is the core's (`money_core`:
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
