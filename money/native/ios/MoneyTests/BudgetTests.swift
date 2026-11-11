// The budget's rules as the screens draw them, over the demo's household:
// its month under its groups — the cards' payments first — and what
// assigning does to Ready to Assign.
@testable import Money
import Foundation
import XCTest

final class BudgetTests: XCTestCase {
    /// Falsified by the groups read without the month's lines, or a hidden
    /// category drawn: Dining Out was not under Everyday. The cards' group
    /// is the budget's own and comes first, as YNAB draws it, holding the
    /// demo's one card's payment category.
    func testTheMonthIsDrawnUnderItsGroups() {
        let s = Seen()
        XCTAssertTrue(s.snap.loaded)
        XCTAssertEqual(s.snap.month, Fx.month)
        let groups = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        XCTAssertEqual(groups.map { $0.group.name }, ["Credit Card Payments", "Bills", "Everyday", "Quality of Life", "Savings Goals"])
        XCTAssertEqual(groups.map(\.isCards), [true, false, false, false, false])
        XCTAssertEqual(groups.first?.id, cardGroupId())
        XCTAssertEqual(groups.first?.lines.map { $0.name }, ["Chase Sapphire"])
        let everyday = groups.first { $0.group.name == "Everyday" }
        XCTAssertEqual(everyday?.lines.map { $0.name }, ["Groceries", "Dining Out", "Gas & Transit", "Household"])
        XCTAssertEqual(everyday?.available, everyday?.lines.reduce(0) { $0 + $1.available })
        // Ready to Assign is the budget's own, never a line of it.
        XCTAssertFalse(s.snap.lines.contains { $0.categoryId == readyToAssignId() })
    }

    /// Assigning takes from Ready to Assign, to the cent. Falsified by
    /// assign adding to what the category had rather than replacing it.
    func testAssigningTakesFromReadyToAssign() {
        let s = Seen()
        guard let before = s.snap.summary, let gifts = s.line("Gifts") else { return XCTFail("the summary and Gifts") }
        s.write(.assign(value: Assign(month: Fx.month, categoryId: gifts.categoryId, amount: gifts.assigned + 12_345)))
        XCTAssertEqual(s.line("Gifts")?.assigned, gifts.assigned + 12_345)
        XCTAssertEqual(s.snap.summary?.readyToAssign, before.readyToAssign - 12_345)
    }

    /// Auto-Assign's Underfunded meets every target: none is short after it.
    func testAutoAssignMeetsEveryTarget() {
        let s = Seen()
        s.write(.autoAssign(value: AutoAssign(month: Fx.month, how: .underfunded, only: [])))
        XCTAssertEqual(s.snap.summary?.underfunded, 0)
        XCTAssertFalse(s.snap.lines.contains { $0.assigned < $0.target })
        for l in s.snap.lines where l.target > 0 { XCTAssertEqual(l.meter()?.fraction(), 1) }
    }

    /// The pager: the month on screen moves, and its lines and summary with it.
    func testTheMonthPages() {
        let s = Seen()
        let last = moneyAddMonths(month: Fx.month, n: -1)
        s.bridge.show(month: last)
        s.bridge.settle()
        XCTAssertEqual(s.snap.month, last)
        XCTAssertEqual(s.snap.summary?.month, last)
        XCTAssertFalse(s.snap.lines.isEmpty)
        XCTAssertEqual(formatMonth(month: last), "September 2026")
    }
}
