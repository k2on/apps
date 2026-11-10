// The budget's rules as the screens draw them, over the demo's household:
// its month under its groups — the cards' payments first — what an
// amount's tone says, which way Move Money goes, and what assigning does
// to Ready to Assign.
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

    /// The seed overspends Dining Out, all of it on the card: its tone is
    /// the card's amber, not cash's red — it is debt — and Move Money on it
    /// covers it all the same, the money coming from the category chosen.
    func testAnOverspentCategoryIsCoveredFromAnother() {
        let s = Seen()
        guard let dining = s.line("Dining Out"), let groceries = s.line("Groceries") else { return XCTFail("the seed's categories") }
        XCTAssertTrue(dining.available < 0)
        XCTAssertEqual(dining.creditOverspent, -dining.available, "the seed's dining out goes on the card")
        XCTAssertEqual(dining.tone(), .onCard)
        let short = Int64(dining.available.magnitude)
        XCTAssertEqual(Moves.suggested(dining), short)
        let m = Moves.of(month: Fx.month, line: dining, other: groceries.categoryId, cents: short)
        XCTAssertEqual(m.from, groceries.categoryId)
        XCTAssertEqual(m.to, dining.categoryId)
        s.write(.moveMoney(value: m))
        XCTAssertEqual(s.line("Dining Out")?.available, 0)
        XCTAssertEqual(s.line("Groceries")?.available, groceries.available - short)
        XCTAssertEqual(s.notices, [])
    }

    /// A category with money moves it out; none is Ready to Assign.
    func testMoneyMovesOutOfAFundedCategory() {
        let s = Seen()
        guard let rent = s.line("Rent") else { return XCTFail("Rent") }
        let m = Moves.of(month: Fx.month, line: rent, other: nil, cents: 100)
        XCTAssertEqual(m.from, rent.categoryId)
        XCTAssertNil(m.to)
        let fromReady = Moves.of(month: Fx.month, line: nil, other: rent.categoryId, cents: 100)
        XCTAssertNil(fromReady.from)
        XCTAssertEqual(fromReady.to, rent.categoryId)
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
