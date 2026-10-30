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
