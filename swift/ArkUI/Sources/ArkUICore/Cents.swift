// Money as an Ark app keeps it — whole cents in an `Int64` — and as people
// type it. Foundation alone; the views that show and take it are ArkUI's
// (`CentsText`, `AmountEntry`). The same reading as money's desktop
// (`money/iced/src/amount.rs`), so one typed amount is one amount everywhere.
import Foundation

public enum Cents {
    /// What somebody typed as an amount, in cents: `12`, `12.5`, `$1,200.00`,
    /// `-3.10`. Nothing typed is nothing; anything else is not an amount.
    public static func parse(_ typed: String) -> Int64? {
        let t = typed.filter { !["$", ",", " ", "\u{a0}"].contains($0) }
        if t.isEmpty { return 0 }
        var negative = false
        var body = Substring(t)
        if body.hasPrefix("-") { negative = true; body = body.dropFirst() }
        else if body.hasPrefix("+") { body = body.dropFirst() }
        let parts = body.split(separator: ".", maxSplits: 1, omittingEmptySubsequences: false)
        let whole = parts.first.map(String.init) ?? ""
        let frac = parts.count > 1 ? String(parts[1]) : ""
        if (whole.isEmpty && frac.isEmpty) || frac.count > 2 { return nil }
        if !(whole + frac).allSatisfy({ $0.isASCII && $0.isNumber }) { return nil }
        let w = whole.isEmpty ? 0 : Int64(whole)
        let f = Int64(frac.padding(toLength: 2, withPad: "0", startingAt: 0))
        guard let w, let f else { return nil }
        let (scaled, o1) = w.multipliedReportingOverflow(by: 100)
        let (cents, o2) = scaled.addingReportingOverflow(f)
        if o1 || o2 { return nil }
        return negative ? -cents : cents
    }

    /// Cents as a field shows them for editing: `1234.50`; nothing is empty.
    public static func editable(_ cents: Int64) -> String {
        if cents == 0 { return "" }
        let sign = cents < 0 ? "-" : ""
        let c = cents.magnitude
        return "\(sign)\(c / 100).\(String(format: "%02d", Int(c % 100)))"
    }

    /// Cents as a `Decimal` of whole units, for a currency format.
    public static func decimal(_ cents: Int64) -> Decimal { Decimal(cents) / 100 }

    /// Cents with a digit typed after them, the way a payment keypad takes
    /// an amount — `1`, `12`, `1.23` — none past a billion.
    public static func typing(_ cents: Int64, digit: Int) -> Int64 {
        let next = cents.magnitude * 10 + UInt64(max(0, min(9, digit)))
        return next >= 100_000_000_000 ? cents : Int64(next)
    }

    /// Cents with their last digit taken back.
    public static func erasing(_ cents: Int64) -> Int64 { Int64(cents.magnitude / 10) }

    /// `$1,234.56` in the en-US way, for where no locale's format is at
    /// hand (a test, a log line, an accessibility value read on Linux).
    public static func plain(_ cents: Int64) -> String {
        let sign = cents < 0 ? "-" : ""
        let c = cents.magnitude
        let dollars = String(c / 100)
        var grouped = ""
        for (i, d) in dollars.enumerated() {
            if i > 0 && (dollars.count - i) % 3 == 0 { grouped.append(",") }
            grouped.append(d)
        }
        return "\(sign)$\(grouped).\(String(format: "%02d", Int(c % 100)))"
    }
}
