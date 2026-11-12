// The budget's rows as SwiftUI lists them, and money as the library writes
// it, with nothing of Apple's but Foundation. Every rule is the core's
// (`money_core`), so the phone, Android and the window say the same; the
// core's tests hold them (money/core/tests/words.rs).
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
