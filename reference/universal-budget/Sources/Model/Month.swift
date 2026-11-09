import Foundation

/// A calendar month, the unit the budget is planned in.
///
/// Months are plain values so they can key dictionaries (assignments are
/// stored per month), be compared, and be passed in launch arguments
/// (`-month 2026-10`).
struct Month: Hashable, Comparable, Identifiable, Sendable {
    var year: Int
    var month: Int

    var id: String { String(format: "%04d-%02d", year, month) }

    init(year: Int, month: Int) {
        // Normalise so `Month(year: 2026, month: 13)` is January 2027.
        let zeroBased = year * 12 + (month - 1)
        self.year = zeroBased / 12
        self.month = zeroBased % 12 + 1
    }

    init(containing date: Date, calendar: Calendar = .budget) {
        let parts = calendar.dateComponents([.year, .month], from: date)
        self.init(year: parts.year ?? 2000, month: parts.month ?? 1)
    }

    /// Parses `yyyy-MM`, the form used by the `-month` launch argument.
    init?(argument: String) {
        let parts = argument.split(separator: "-").compactMap { Int($0) }
        guard parts.count == 2, (1...12).contains(parts[1]) else { return nil }
        self.init(year: parts[0], month: parts[1])
    }

    var next: Month { Month(year: year, month: month + 1) }
    var previous: Month { Month(year: year, month: month - 1) }

    func advanced(by months: Int) -> Month { Month(year: year, month: month + months) }

    /// The first instant of the month.
    var start: Date {
        Calendar.budget.date(from: DateComponents(year: year, month: month, day: 1))!
    }

    /// The first instant of the following month (exclusive end).
    var end: Date { next.start }

    var dateRange: Range<Date> { start..<end }

    func contains(_ date: Date) -> Bool { dateRange.contains(date) }

    static func < (lhs: Month, rhs: Month) -> Bool {
        (lhs.year, lhs.month) < (rhs.year, rhs.month)
    }
}

extension Calendar {
    /// The calendar used for month arithmetic. Gregorian, with the user's
    /// time zone, so a transaction dated on the 1st falls in that month
    /// wherever the person is.
    static let budget: Calendar = {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .current
        return calendar
    }()
}
