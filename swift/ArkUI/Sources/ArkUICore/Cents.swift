// Money as an Ark app keeps it — whole cents in an `Int64` — and how the
// place the person is in writes it. Foundation alone; the views that show and
// take it are ArkUI's (`CentsText`, `AmountEntry`).
//
// The writing and the reading are the app's library's (ark_format, every
// client's: the desktop prints what the phone prints). What the locale says
// is read here, once, from NumberFormatter (`LocaleNumbers`); the app hands
// it to its library and says how cents are written (`Cents.write`).
import Foundation

public enum Cents {
    /// How cents are written — `$1,234.56`, `+$25.00` when `signed` and
    /// above nothing — as the app's library writes them in the locale's
    /// `LocaleNumbers`. The app sets it once, before the first view draws;
    /// until then a figure is its cents, unwritten.
    public static var write: (_ cents: Int64, _ signed: Bool) -> String = { cents, _ in "\(cents)" }

    /// Cents with a digit typed after them, the way a payment keypad takes
    /// an amount — `1`, `12`, `1.23` — none past a billion.
    public static func typing(_ cents: Int64, digit: Int) -> Int64 {
        let next = cents.magnitude * 10 + UInt64(max(0, min(9, digit)))
        return next >= 100_000_000_000 ? cents : Int64(next)
    }

    /// Cents with their last digit taken back.
    public static func erasing(_ cents: Int64) -> Int64 { Int64(cents.magnitude / 10) }
}

/// How a place writes money, as NumberFormatter says it for a locale: the
/// fields of ark_format's `NumberStyle`, which the app's library formats
/// with. The currency is the locale's when it counts in hundredths and the
/// dollar otherwise, since the amounts are hundredths whatever the symbol.
public struct LocaleNumbers: Equatable, Sendable {
    /// The currency's ISO code: `USD`.
    public var currency: String
    /// What is written beside the figure: `$`, `€`.
    public var symbol: String
    /// The symbol before the figure (`$1.00`) or after it (`1,00 €`).
    public var symbolFirst: Bool
    /// A space between the symbol and the figure.
    public var symbolSpaced: Bool
    /// The decimal mark.
    public var decimal: String
    /// What groups thousands; nothing for none.
    public var grouping: String
    /// The minus sign.
    public var minus: String
    /// With the symbol first, the minus before it (`-$12.00`) rather than
    /// after it (`$-12.00`).
    public var minusFirst: Bool

    /// What NumberFormatter says of money in `locale`.
    public static func of(_ locale: Locale = .current) -> LocaleNumbers {
        let f = NumberFormatter()
        f.locale = locale
        f.numberStyle = .currency
        if f.maximumFractionDigits != 2 { f.currencyCode = "USD" }
        let symbol = f.currencySymbol ?? "$"
        let one = f.string(from: 1) ?? ""
        let at = one.range(of: symbol)
        let digit = one.firstIndex(where: { $0.isNumber })
        let first = at.map { r in digit.map { r.lowerBound < $0 } ?? true } ?? true
        let gap: Character? = at.flatMap { r in
            first ? (r.upperBound < one.endIndex ? one[r.upperBound] : nil)
                : (r.lowerBound > one.startIndex ? one[one.index(before: r.lowerBound)] : nil)
        }
        let minus = f.minusSign ?? "-"
        let less = f.string(from: -1) ?? ""
        var minusFirst = true
        if let m = less.range(of: minus), let s = less.range(of: symbol) { minusFirst = m.lowerBound < s.lowerBound }
        return LocaleNumbers(
            currency: f.currencyCode ?? "USD",
            symbol: symbol,
            symbolFirst: first,
            symbolSpaced: gap?.isWhitespace == true,
            decimal: f.currencyDecimalSeparator ?? ".",
            grouping: f.usesGroupingSeparator ? (f.currencyGroupingSeparator ?? ",") : "",
            minus: minus,
            minusFirst: minusFirst)
    }
}
