// What the tests stand on: the demo's household for a fixed day — the
// demo window's to the cent, the same every time — and a session over it
// that tells what it publishes at once, on its own queue, so a test reads
// it after `settle()`.
@testable import Money
import Foundation

enum Fx {
    static let today = "2026-10-26"
    static let month = "2026-10"

    static func demo() -> Peer { try! moneyDemo(today: today) }

    static func id(_ n: Int) -> Id { Id(String(format: "%032x", n)) }

    static let login = Login(token: "t", session: "s1", user: "alex", name: "Alex", email: "alex@example.com", roles: [], expiresMs: 42, picture: "")
}

/// A session over the demo, and everything it said.
final class Seen {
    var snap = Snapshot()
    var notices = [String]()
    var failures = [String]()
    var bridge: Bridge!

    init(month: String = Fx.month) {
        bridge = Bridge(.demo(today: Fx.today), month: month, deliver: { $0() },
                        publish: { [unowned self] s in self.snap = s },
                        notice: { [unowned self] n in self.notices.append(n) },
                        denied: { _ in },
                        failed: { [unowned self] w in self.failures.append(w) })
        bridge.settle()
    }

    /// A line of the month, by its category's name.
    func line(_ name: String) -> BudgetLine? { snap.lines.first { $0.name == name } }
    func account(_ name: String) -> AccountLine? { snap.accounts.first { $0.name == name } }

    /// `f` done on the session, and everything it moved published.
    func write(_ f: @escaping (Money) throws -> Void) {
        bridge.write("do it", f)
        bridge.settle()
    }
}

/// Rows made by hand, for the rules that need no session.
extension Fx {
    /// A month's line: a category of the household's, or — given `card` — a
    /// card's payment category in the cards' group.
    static func line(_ name: String, available: Int64, target: Int64 = 0, assigned: Int64 = 0, card: Id? = nil,
                     debt: Int64 = 0, underfunded: Int64 = 0, creditOverspent: Int64 = 0) -> BudgetLine {
        BudgetLine(categoryId: id(name.count), groupId: card == nil ? id(100) : Fixed.cardGroup, group: card == nil ? "Everyday" : "Credit Card Payments",
                   groupPos: card == nil ? 2 : 1, name: name, pos: 1, hidden: false, target: target, assigned: assigned, activity: 0,
                   available: available, carried: 0, assignedLastMonth: 0, spentLastMonth: 0, averageSpent: 0,
                   card: card, debt: debt, underfunded: underfunded, creditOverspent: creditOverspent)
    }

    static func account(_ n: Int, _ name: String, kind: String = "checking", closed: Bool = false, bank: String = "", pos: Int64 = 0) -> AccountLine {
        AccountLine(id: id(n), name: name, kind: kind, onBudget: true, closed: closed, pos: pos, balance: 0, cleared: 0,
                    uncleared: 0, bank: bank, mask: "", bankBalance: nil, unapproved: 0)
    }

    static func ask(_ n: Int, _ what: String = "connect", state: AskState, url: String = "", message: String = "",
                    institution: String = "", asked: Int64 = 0, answered: Int64 = 0, expires: Int64 = 0) -> AskLine {
        AskLine(id: id(n), what: what, bank: institution.isEmpty ? "" : institution.lowercased(), institution: institution,
                state: state, url: url, message: message, expiresMs: expires, askedMs: asked, answeredMs: answered)
    }

    static func bank(_ name: String, institution: String = "", synced: Int64 = 0, error: String = "", needsLogin: Bool = false,
                     waiting: Int64 = 0) -> BankLine {
        BankLine(name: name, institution: institution, syncedMs: synced, added: 0, updated: 0, deleted: 0, locked: 0, accounts: 1,
                 errorCode: needsLogin ? "ITEM_LOGIN_REQUIRED" : "", error: error, failedMs: error.isEmpty ? 0 : 1, needsLogin: needsLogin,
                 waiting: waiting)
    }

    static func linked(_ plaid: String, bank: String = "wells", name: String, mask: String = "", kind: String = "depository",
                       subtype: String = "", waiting: Bool = false, ignored: Bool = false, opensAs: String = "checking",
                       feeds: AccountLine? = nil) -> LinkedLine {
        LinkedLine(plaidAccountId: plaid, bank: bank, accountId: feeds?.id, account: feeds?.name ?? "", name: name, mask: mask,
                   kind: kind, balance: nil, ignored: ignored, subtype: subtype, waiting: waiting, opensAs: opensAs)
    }
}
