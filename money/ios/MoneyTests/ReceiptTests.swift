// What the receipt screens decide with no session: where a transaction's
// receipt stands, which unmatched receipts are offered first, and the
// receipt form's amounts and what it gives the domain.
@testable import Money
import ArkUICore
import Foundation
import XCTest

final class ReceiptTests: XCTestCase {
    private func receipt(_ id: String, total: Int64, date: String = "2026-10-25", txn: Id? = nil, charges: Int64 = 0) -> ReceiptLine {
        ReceiptLine(id: id, store: "paper", date: date, place: "ALDI", total: total, items: 2, filed: 0, txnId: txn,
                    payee: "", account: "", charges: charges, charged: 0)
    }

    private func photo(_ n: Int, txn: Id?, receipt: String = "", state: String = "reading", why: String = "") -> PhotoLine {
        PhotoLine(id: Fx.id(n), file: FileRef(hash: String(repeating: "a", count: 64), size: 3, media: Receipts.jpeg),
                  txnId: txn, receiptId: receipt, takenMs: 0, state: state, why: why)
    }

    func testATransactionsReceiptIsReadOrBeingRead() {
        let t = Fx.id(1)
        XCTAssertEqual(Receipts.state(txn: t, receipts: [], photos: []), .none)
        XCTAssertEqual(Receipts.state(txn: t, receipts: [], photos: [photo(9, txn: t)]), .reading(why: ""))
        XCTAssertEqual(Receipts.state(txn: t, receipts: [], photos: [photo(9, txn: t, state: "failed", why: "not here yet")]),
                       .reading(why: "not here yet"))
        XCTAssertEqual(Receipts.state(txn: t, receipts: [], photos: [photo(9, txn: Fx.id(2))]), .none, "another's photo")
        let r = receipt("paper:x", total: -1_000, txn: t)
        XCTAssertEqual(Receipts.state(txn: t, receipts: [r], photos: [photo(9, txn: t, receipt: r.id, state: "sure")]),
                       .receipt(r, check: false))
        XCTAssertEqual(Receipts.state(txn: t, receipts: [r], photos: [photo(9, txn: t, receipt: r.id, state: "check")]),
                       .receipt(r, check: true))
    }

    func testTheNearestUnmatchedReceiptsAreOfferedFirst() {
        let all = [
            receipt("a", total: -1_200, date: "2026-10-20"),
            receipt("b", total: -1_000, date: "2026-10-30"),
            receipt("c", total: -1_000, date: "2026-10-26"),
            receipt("d", total: -1_000, txn: Fx.id(3)),
            receipt("e", total: -1_000, charges: 2),
        ]
        XCTAssertEqual(Receipts.unmatched(all, amount: -1_000, day: "2026-10-25").map(\.id), ["c", "b", "a"])
    }

    func testAReceiptIsTypedAsItsPaperReads() {
        XCTAssertEqual(ReceiptDraft.typed(429), "4.29")
        XCTAssertEqual(ReceiptDraft.typed(-50), "-0.50")
        XCTAssertEqual(ReceiptDraft.cents("4.29"), 429)
        XCTAssertNil(ReceiptDraft.cents(""))
        var d = ReceiptDraft(txn: Fx.id(1), place: "ALDI", date: "2026-10-25")
        XCTAssertEqual(d.problem, "Add an item")
        d.items = [PaperItem(name: "EGGS", amount: "4.29"), PaperItem(name: "", amount: "1"), PaperItem()]
        XCTAssertEqual(d.problem, "Every item needs a name")
        d.items[1].name = "COUPON"
        d.items[1].amount = "-0.50"
        XCTAssertNil(d.problem)
        XCTAssertEqual(d.total, 379)
        let w = d.write
        XCTAssertEqual(w.items.map(\.amount), [-429, 50], "spent negative, a discount positive")
        XCTAssertEqual(w.receiptId, "")
        XCTAssertEqual(w.txnId, Fx.id(1))
        XCTAssertNil(w.total)
    }

    func testAReceiptReadIsPutRightFromItsItems() {
        let r = receipt("paper:x", total: -1_000, txn: Fx.id(1))
        let item = ItemLine(receiptId: r.id, line: 1, store: "paper", date: r.date, place: "ALDI", name: "EGGS", sku: "",
                            quantity: 2000, unit: "ea", amount: -1_000, unitPrice: -500, categoryId: nil, category: "")
        let d = ReceiptDraft(r, items: [item], txn: nil)
        XCTAssertEqual(d.receiptId, "paper:x")
        XCTAssertEqual(d.txn, Fx.id(1))
        XCTAssertEqual(d.items.map(\.amount), ["10.00"])
        XCTAssertEqual(d.items.map(\.quantity), [2000])
    }
}
