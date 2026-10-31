// The budget's rules as the screens draw them, over the demo's household:
// its month under its groups, what an amount's tone says, which way Move
// Money goes, and what assigning does to Ready to Assign.
@testable import Money
import Foundation
import XCTest

final class BudgetTests: XCTestCase {
    /// Falsified by the groups read without the month's lines, or a hidden
    /// category drawn: Dining Out was not under Everyday.
    func testTheMonthIsDrawnUnderItsGroups() {
        let s = Seen()
        XCTAssertTrue(s.snap.loaded)
        XCTAssertEqual(s.snap.month, Fx.month)
        let groups = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        XCTAssertEqual(groups.map { $0.group.name }, ["Bills", "Everyday", "Quality of Life", "Savings Goals"])
        let everyday = groups.first { $0.group.name == "Everyday" }
        XCTAssertEqual(everyday?.lines.map { $0.name }, ["Groceries", "Dining Out", "Gas & Transit", "Household"])
        XCTAssertEqual(everyday?.available, everyday?.lines.reduce(0) { $0 + $1.available })
        // Ready to Assign is the budget's own, never a line of it.
        XCTAssertFalse(s.snap.lines.contains { $0.categoryId == Fixed.readyToAssign })
    }

    /// The seed overspends Dining Out: its tone is over, and Move Money on
    /// it covers it — the money comes from the category chosen.
    func testAnOverspentCategoryIsCoveredFromAnother() {
        let s = Seen()
        guard let dining = s.line("Dining Out"), let groceries = s.line("Groceries") else { return XCTFail("the seed's categories") }
        XCTAssertTrue(dining.available < 0)
        XCTAssertEqual(Tone.of(dining), .over)
        let short = Int64(dining.available.magnitude)
        XCTAssertEqual(Moves.suggested(dining), short)
        let m = Moves.of(month: Fx.month, line: dining, other: groceries.categoryId, cents: short)
        XCTAssertEqual(m.from, groceries.categoryId)
        XCTAssertEqual(m.to, dining.categoryId)
        s.write { try $0.moveMoney(m) }
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
        s.write { try $0.assign(Assign(month: Fx.month, categoryId: gifts.categoryId, amount: gifts.assigned + 12_345)) }
        XCTAssertEqual(s.line("Gifts")?.assigned, gifts.assigned + 12_345)
        XCTAssertEqual(s.snap.summary?.readyToAssign, before.readyToAssign - 12_345)
        XCTAssertEqual(Tone.of(available: 1, target: 0, assigned: 0), .funded)
        XCTAssertEqual(Tone.of(available: 0, target: 100, assigned: 50), .short)
        XCTAssertEqual(Tone.of(available: 0, target: 0, assigned: 0), .zero)
    }

    /// Auto-Assign's Underfunded meets every target: none is short after it.
    func testAutoAssignMeetsEveryTarget() {
        let s = Seen()
        s.write { try $0.autoAssign(AutoAssign(month: Fx.month, how: .underfunded, only: [])) }
        XCTAssertEqual(s.snap.summary?.underfunded, 0)
        XCTAssertFalse(s.snap.lines.contains { $0.assigned < $0.target })
        for l in s.snap.lines where l.target > 0 { XCTAssertEqual(funded(l), 1) }
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
        XCTAssertEqual(moneyMonthName(month: last), "September 2026")
    }

    // MARK: Edit

    /// Dining Out dragged up under Bills, in front of its second category:
    /// it joins Bills there, and the budget comes back the way it was drawn
    /// ahead. Falsified by `before` read after the row was taken out, or the
    /// group read from where the row started.
    func testACategoryDraggedUnderAnotherGroupJoinsIt() {
        let s = Seen()
        let groups = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        let rows = Arranging.rows(groups)
        guard let bills = groups.first(where: { $0.group.name == "Bills" }), bills.lines.count > 1,
              let dining = s.line("Dining Out"),
              let from = rows.firstIndex(where: { $0.id == Arranged.category(dining).id }),
              let to = rows.firstIndex(where: { $0.id == Arranged.category(bills.lines[1]).id })
        else { return XFail() }
        let r = Arranging.drop(rows, from: from, to: to)
        XCTAssertEqual(r, .category(MoveCategory(id: dining.categoryId, groupId: bills.id, before: bills.lines[1].categoryId)))
        guard let r else { return }
        let drawn = Arranging.applied(r, to: groups)
        s.write { m in if case let .category(c) = r { try m.moveCategory(c) } }
        let now = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        XCTAssertEqual(Arranging.shape(now), Arranging.shape(drawn))
        XCTAssertEqual(now.first { $0.id == bills.id }?.lines.map { $0.name },
                       [bills.lines[0].name, "Dining Out"] + bills.lines.dropFirst().map { $0.name })
        XCTAssertFalse(now.first { $0.group.name == "Everyday" }?.lines.contains { $0.name == "Dining Out" } ?? true)
        XCTAssertEqual(s.line("Dining Out")?.groupId, bills.id)
        XCTAssertEqual(s.notices, [])
    }

    /// Let go past the last row, a category goes last in the last group; let
    /// go above every group, first in the first.
    func testACategoryAtEitherEndGoesToTheNearestGroup() {
        let s = Seen()
        let groups = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        let rows = Arranging.rows(groups)
        guard let first = groups.first, let last = groups.last, let groceries = s.line("Groceries"),
              let from = rows.firstIndex(where: { $0.id == Arranged.category(groceries).id })
        else { return XFail() }
        XCTAssertEqual(Arranging.drop(rows, from: from, to: rows.count),
                       .category(MoveCategory(id: groceries.categoryId, groupId: last.id, before: nil)))
        XCTAssertEqual(Arranging.drop(rows, from: from, to: 0),
                       .category(MoveCategory(id: groceries.categoryId, groupId: first.id, before: first.lines.first?.categoryId)))
    }

    /// A group dragged to the top goes first; into the middle of another's
    /// categories, after that one; past the end, last. Its categories go
    /// with it, as drawn ahead.
    func testAGroupDraggedTakesItsCategories() {
        let s = Seen()
        let groups = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        let rows = Arranging.rows(groups)
        let names = groups.map { $0.group.name }
        guard names.count == 4,
              let savings = rows.firstIndex(where: { $0.id == Arranged.group(groups[3].group).id }),
              let bills = rows.firstIndex(where: { $0.id == Arranged.group(groups[0].group).id }),
              let everyday = groups[1].lines.last.flatMap({ l in rows.firstIndex { $0.id == Arranged.category(l).id } })
        else { return XFail() }
        XCTAssertEqual(Arranging.drop(rows, from: savings, to: 0), .group(MoveGroup(id: groups[3].id, before: groups[0].id)))
        XCTAssertEqual(Arranging.drop(rows, from: bills, to: rows.count), .group(MoveGroup(id: groups[0].id, before: nil)))
        // In among Everyday's categories: after Everyday, in front of Quality of Life.
        let r = Arranging.drop(rows, from: bills, to: everyday)
        XCTAssertEqual(r, .group(MoveGroup(id: groups[0].id, before: groups[2].id)))
        guard let r else { return }
        let drawn = Arranging.applied(r, to: groups)
        s.write { m in if case let .group(g) = r { try m.moveGroup(g) } }
        let now = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        XCTAssertEqual(now.map { $0.group.name }, [names[1], names[0], names[2], names[3]])
        XCTAssertEqual(Arranging.shape(now), Arranging.shape(drawn))
        XCTAssertEqual(now[1].lines.map { $0.name }, groups[0].lines.map { $0.name })
    }

    /// A row let go where it was, or a group among its own categories, or
    /// the first category above every group, asks nothing.
    func testADropThatMovesNothingAsksNothing() {
        let s = Seen()
        let groups = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        let rows = Arranging.rows(groups)
        guard groups[0].lines.count > 1 else { return XFail() }
        for i in rows.indices {
            XCTAssertNil(Arranging.drop(rows, from: i, to: i))
            XCTAssertNil(Arranging.drop(rows, from: i, to: i + 1))
        }
        XCTAssertNil(Arranging.drop(rows, from: 0, to: 2))
        XCTAssertNil(Arranging.drop(rows, from: 1, to: 0))
        XCTAssertNil(Arranging.drop(rows, from: rows.count, to: 0))
    }

    private func XFail(file: StaticString = #filePath, line: UInt = #line) { XCTFail("the seed's groups and categories", file: file, line: line) }
}
