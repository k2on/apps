// Money as every Ark app writes it on the iPhone, the iPad and the Mac: by
// the library every client formats with (ark_format, in each app's library),
// in what the device's locale says of money, read once from NumberFormatter
// (ArkUICore's `LocaleNumbers`). ArkUI's figures write cents through
// `Cents.write`, said here once, before the first view draws.
import ArkUICore
import Foundation
#if canImport(ArkBindings)
import ArkBindings
#endif

public enum ArkNumbers {
    /// The device's number style, as the library takes it.
    public static let style: NumberStyle = {
        let n = LocaleNumbers.of()
        return NumberStyle(currency: n.currency, symbol: n.symbol, symbolFirst: n.symbolFirst, symbolSpaced: n.symbolSpaced,
                           decimal: n.decimal, grouping: n.grouping, minus: n.minus, minusFirst: n.minusFirst)
    }()

    /// Says how ArkUI's figures write cents: once, as the app is made.
    public static func install() {
        Cents.write = { cents, signed in formatAmount(cents: cents, withSign: signed, style: style) }
    }
}
