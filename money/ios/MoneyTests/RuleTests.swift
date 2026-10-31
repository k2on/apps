// A category assigned by rule (`money_domain::rules`), as the phone draws
// it: read-only in a month its rule assigns — no keypad, no quick assign,
// no moving money to or from it — and its rule said in its place. Rows made
// by hand for the rule alone, then the demo's household with a rule set
// through the session.
@testable import Money
import Foundation
import XCTest

final class RuleTests: XCTestCase {
    /// Falsified by a ruled line offered to the keypad, or one by hand
    /// read as ruled.
    func testOnlyAMonthByHandIsEditable() {
        let hand = Fx.line("Gifts", available: 0)
        let ruled = Fx.line("Giving", available: 6_300, assigned: 6_300, rule: "10% of income matching ACME in Checking")
        XCTAssertTrue(Ruled.editable(hand))
        XCTAssertFalse(Ruled.editable(ruled))
        XCTAssertNil(Ruled.words(hand))
        XCTAssertEqual(Ruled.words(ruled), "Assigned by rule: 10% of income matching ACME in Checking")
        // A rule the domain could not say is still said to be one.
        XCTAssertEqual(Ruled.words(Fx.line("Tithe", available: 0, rule: " ")), "Assigned by rule")
    }

    /// Falsified by Move Money offering a ruled category as where money
    /// goes or comes from, or leaving out one assigned by hand.
    func testMoveMoneyPassesARuledCategoryBy() {
        let group = GroupLine(id: Fx.id(100), name: "Everyday", pos: 2, hidden: false)
        let lines = [Fx.line("Groceries", available: 100), Fx.line("Giving", available: 6_300, rule: "10% of income matching ACME in any account"),
                     Fx.line("Rent", available: 0)]
        let groups = [GroupMonth(group: group, lines: lines)]
        XCTAssertEqual(Moves.others(groups, besides: nil).map(\.name), ["Groceries", "Rent"])
        XCTAssertEqual(Moves.others(groups, besides: lines[0].categoryId).map(\.name), ["Rent"])
    }

    /// The demo with a rule set on Gifts from this month: 10% of what Acme
    /// paid into Checking (two paydays of $3,150 by the 26th) is its
    /// assignment, the line says it follows the rule and is read-only, Move
    /// Money no longer offers it, and assigning it by hand is refused in the
    /// domain's words. Falsified by `ruled` or `rule` not reaching the line,
    /// or the screens' rule reading anything but `ruled`.
    func testTheDemoWithARuleIsReadOnly() {
        let s = Seen()
        guard let gifts = s.line("Gifts"), let checking = s.account("Checking") else { return XCTFail("Gifts and Checking") }
        XCTAssertTrue(Ruled.editable(gifts), "the seed assigns Gifts by hand")
        s.write { try $0.setRule(SetRule(categoryId: gifts.categoryId, from: Fx.month, matching: "acme", accounts: [checking.id], times: 10, per: 100)) }
        XCTAssertEqual(s.notices, [])
        guard let ruled = s.line("Gifts") else { return XCTFail("Gifts after its rule") }
        XCTAssertTrue(ruled.ruled)
        XCTAssertFalse(Ruled.editable(ruled))
        XCTAssertEqual(ruled.assigned, 63_000)
        XCTAssertEqual(Ruled.words(ruled), "Assigned by rule: 10% of income matching acme in Checking")
        let groups = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        XCTAssertFalse(Moves.others(groups, besides: nil).contains { $0.categoryId == gifts.categoryId })
        // Every other category is still assigned by hand.
        XCTAssertEqual(s.snap.lines.filter { !Ruled.editable($0) }.map(\.name), ["Gifts"])

        s.write { try $0.assign(Assign(month: Fx.month, categoryId: gifts.categoryId, amount: 1_000)) }
        XCTAssertEqual(s.notices.count, 1)
        XCTAssertTrue(s.notices.first?.contains("this category's assignment follows its rule") ?? false)
        XCTAssertEqual(s.line("Gifts")?.assigned, 63_000)

        // Auto-Assign passes it by: resetting the month leaves the rule's amount.
        s.write { try $0.autoAssign(AutoAssign(month: Fx.month, how: .reset, only: [])) }
        XCTAssertEqual(s.line("Gifts")?.assigned, 63_000)
        XCTAssertEqual(s.line("Groceries")?.assigned, 0)

        // The month before the rule's first is assigned by hand, and stays so.
        s.bridge.show(month: moneyAddMonths(month: Fx.month, n: -1))
        s.bridge.settle()
        XCTAssertEqual(s.line("Gifts").map(Ruled.editable), true)
    }
}
