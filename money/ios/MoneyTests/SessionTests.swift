// The session over the demo's household, as the screens use it: the
// accounts in their sections, a register held and let go, the inbox
// approved, a transaction typed into the form and saved, the bank's syncs,
// and a refusal said in the domain's words.
@testable import Money
import Foundation
import XCTest

final class SessionTests: XCTestCase {
    /// Falsified by tracking accounts listed on budget: the mortgage was budgeted.
    func testTheAccountsAreShelved() {
        let s = Seen()
        let shelves = accountShelves(accounts: s.snap.accounts)
        XCTAssertEqual(shelves.map { $0.shelving }, [.budget, .tracking])
        let budget = shelves[0].accounts.map { $0.name }
        let tracking = shelves[1].accounts.map { $0.name }
        for n in ["Checking", "Savings", "Cash", "Chase Sapphire"] { XCTAssertTrue(budget.contains(n), n) }
        for n in ["Mortgage", "Retirement"] { XCTAssertTrue(tracking.contains(n), n) }
        XCTAssertEqual(netWorth(accounts: s.snap.accounts), s.snap.accounts.reduce(0) { $0 + $1.balance })
        XCTAssertEqual(s.snap.account(usualAccount(accounts: s.snap.accounts))?.onBudget, true)
        let card = s.account("Chase Sapphire")
        XCTAssertEqual(card?.subtitle().hasPrefix("Chase"), true)
    }

    /// A register is read when held and kept spliced while it is.
    func testARegisterIsHeldAndSpliced() {
        let s = Seen()
        guard let checking = s.account("Checking") else { return XCTFail("Checking") }
        XCTAssertNil(s.register(checking.id))
        s.hold(.register(account: checking.id))
        let rows = s.register(checking.id) ?? []
        XCTAssertFalse(rows.isEmpty)
        XCTAssertTrue(rows.allSatisfy { $0.accountId == checking.id })
        // Newest first, a day at a time.
        let days = Registers.days(rows)
        XCTAssertEqual(days.map { $0.day }, days.map { $0.day }.sorted(by: >))
        XCTAssertEqual(days.reduce(0) { $0 + $1.rows.count }, rows.count)

        var d = TxnForm(account: checking.id, accounts: s.snap.accounts, date: Fx.today)
        d.payee = "Corner Bakery"
        d.cents = 1_250
        d.choose(category: s.line("Dining Out")?.categoryId)
        guard let add = d.sent.toChange() else { return XCTFail("a new transaction's change") }
        XCTAssertEqual(d.sent.amount, -1_250)
        s.write(add)
        let after = s.register(checking.id) ?? []
        XCTAssertEqual(after.count, rows.count + 1)
        XCTAssertEqual(after.first?.payee, "Corner Bakery")
        XCTAssertEqual(after.first?.category, "Dining Out")

        s.release(.register(account: checking.id))
        XCTAssertNil(s.register(checking.id))
    }

    /// The inbox empties as it is looked at: what has a category approved,
    /// and what needs one categorized.
    func testTheInboxIsLookedAt() {
        let s = Seen()
        let waiting = s.snap.inbox
        XCTAssertFalse(waiting.isEmpty)
        let ready = waiting.filter { !$0.needsCategory() }.map { $0.id }
        let needs = waiting.filter { $0.needsCategory() }.map { $0.id }
        if !ready.isEmpty { s.write(.approve(value: Txns(ids: ready))) }
        if !needs.isEmpty, let c = s.line("Household")?.categoryId {
            s.write(.categorize(value: Categorize(ids: needs, categoryId: c)))
        }
        XCTAssertEqual(s.snap.inbox.count, 0)
        XCTAssertTrue(s.snap.accounts.allSatisfy { $0.unapproved == 0 })
    }

    /// The demo's bank: two syncs of news, then nothing — what pull to refresh says.
    func testPullingToRefreshSyncsTheDemosBank() {
        let s = Seen()
        let before = s.snap.inbox.count
        var said = [String]()
        for _ in 0..<3 {
            s.bridge.syncBank { said.append($0) }
            s.bridge.settle()
        }
        XCTAssertEqual(said, ["Chase: 2 new from the bank \u{2014} one still pending",
                              "Chase: 2 from the bank, 1 pending one posted",
                              "Chase: nothing new at the bank"])
        XCTAssertTrue(s.snap.inbox.count > before)
        XCTAssertFalse(s.snap.banks.isEmpty)
        // What the bank brought is opened with what the bank called it.
        guard let t = s.snap.inbox.first(where: { $0.imported }) else { return XCTFail("an imported line") }
        XCTAssertFalse(t.importedPayee.isEmpty)
        XCTAssertEqual(TxnForm(t).bank, t.bankSaid())
        XCTAssertNotNil(TxnForm(t).bank, "\(t.payee) / \(t.importedPayee)")
    }

    /// A refusal is the domain's sentence, said once.
    func testARefusalIsSaidInTheDomainsWords() {
        let s = Seen()
        s.write(.openAccount(value: OpenAccount(name: "  ", kind: .checking, onBudget: true, balance: 0, date: Fx.today)))
        XCTAssertEqual(s.notices.count, 1)
        XCTAssertEqual(s.notices.first.map { $0.hasPrefix("Couldn't do it: ") }, true)
        XCTAssertTrue(s.notices.first?.contains("an account needs a name") ?? false)
        s.write(.openAccount(value: OpenAccount(name: "Travel Fund", kind: .savings, onBudget: true, balance: 50_000, date: Fx.today)))
        XCTAssertEqual(s.account("Travel Fund")?.balance, 50_000)
        XCTAssertEqual(s.notices.count, 1)
        XCTAssertEqual(s.failures, [])
    }
}
