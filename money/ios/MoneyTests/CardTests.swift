// Credit cards as the screens draw them (`money_domain::cards`): a card's
// payment category, what it says and the tap that covers it, credit
// overspending told apart from cash, the payments kept out of a
// transaction's categories and in the cards' group, and a transaction
// already there made a transfer to the card from its form.
@testable import Money
import Foundation
import XCTest

final class CardTests: XCTestCase {
    /// Red is cash overspent, which next month pays for; amber is the same
    /// on a card, which is debt — only when all of it was on the card. A
    /// card's payment is short while it leaves debt uncovered, and funded
    /// once it covers it. Falsified by the desktop's `Tone::of` alone: the
    /// card's overspending read red.
    func testCreditOverspendingIsToldFromCash() {
        let cash = Fx.line("Groceries", available: -500)
        let card = Fx.line("Dining Out", available: -500, creditOverspent: 500)
        let both = Fx.line("Household", available: -500, creditOverspent: 200)
        XCTAssertEqual(Tone.of(cash), .over)
        XCTAssertEqual(Tone.of(card), .onCard)
        XCTAssertEqual(Tone.of(both), .over, "some cash overspent is cash's red")
        XCTAssertEqual(Cards.word(cash), nil)
        XCTAssertEqual(Cards.word(card), .onCard(500))
        XCTAssertEqual(Cards.word(both), .onCard(200))

        let visa = Fx.id(77)
        let short = Fx.line("Visa", available: 3_000, card: visa, debt: 10_000, underfunded: 7_000)
        let covered = Fx.line("Visa", available: 10_000, card: visa, debt: 10_000)
        let paidOff = Fx.line("Visa", available: 0, card: visa)
        XCTAssertEqual(Tone.of(short), .short)
        XCTAssertEqual(Tone.of(covered), .funded)
        XCTAssertEqual(Tone.of(paidOff), .zero)
        XCTAssertEqual(Cards.word(short), .short(7_000))
        XCTAssertEqual(Cards.word(covered), .covered)
        XCTAssertEqual(Cards.word(paidOff), nil)
        XCTAssertEqual(Cards.covered(short), 0.3)
        XCTAssertEqual(Cards.covered(covered), 1)
        XCTAssertNil(Cards.covered(paidOff))
        XCTAssertNil(Cards.covered(cash))
        // One tap assigns what it lacks on top of what it was given.
        XCTAssertEqual(Cards.toCover(Fx.line("Visa", available: 3_000, assigned: 2_000, card: visa, debt: 10_000, underfunded: 7_000)), 9_000)
        XCTAssertNil(Cards.toCover(covered))
        XCTAssertNil(Cards.toCover(cash))
        XCTAssertEqual(Cards.uncovered([short, covered, cash, card]), 7_000)
    }

    /// The demo's card: its payment category first, the card's own, owing
    /// what the card's balance says, with the month's last days not yet
    /// filed left uncovered; one tap covers them out of Ready to Assign.
    func testTheCardsPaymentIsCoveredAtATap() {
        let s = Seen()
        guard let card = s.account("Chase Sapphire"), let pay = s.line("Chase Sapphire"), let ready = s.snap.summary?.readyToAssign
        else { return XCTFail("the demo's card and its payment") }
        XCTAssertTrue(Cards.isPayment(pay))
        XCTAssertEqual(pay.groupId, Fixed.cardGroup)
        XCTAssertEqual(pay.card, card.id)
        XCTAssertEqual(pay.debt, -card.balance)
        XCTAssertTrue(pay.available > 0 && pay.underfunded > 0, "\(pay)")
        XCTAssertEqual(pay.available + pay.underfunded, pay.debt)
        XCTAssertEqual(Tone.of(pay), .short)
        XCTAssertEqual(Cards.word(pay), .short(pay.underfunded))
        let groups = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        XCTAssertEqual(groups.first?.uncovered, pay.underfunded)
        XCTAssertEqual(groups.dropFirst().map(\.uncovered), groups.dropFirst().map { _ in 0 })

        guard let cover = Cards.toCover(pay) else { return XCTFail("nothing to cover") }
        s.write { try $0.assign(Assign(month: Fx.month, categoryId: pay.categoryId, amount: cover)) }
        guard let after = s.line("Chase Sapphire") else { return XCTFail("the payment, after") }
        XCTAssertEqual(after.underfunded, 0)
        XCTAssertEqual(after.available, after.debt)
        XCTAssertEqual(Cards.word(after), .covered)
        XCTAssertEqual(Cards.covered(after), 1)
        XCTAssertEqual(Tone.of(after), .funded)
        XCTAssertNil(Cards.toCover(after))
        XCTAssertEqual(s.snap.summary?.readyToAssign, ready - pay.underfunded)
        XCTAssertEqual(s.notices, [])
    }

    /// A transaction's category is never a card's payment: the list it is
    /// chosen from has every group but the cards', each whole.
    func testAPaymentIsNoTransactionsCategory() {
        let s = Seen()
        let groups = Months.groups(s.snap.groups, s.snap.lines, hidden: false)
        let choosable = Cards.choosable(groups)
        XCTAssertEqual(choosable.map { $0.group.name }, ["Bills", "Everyday", "Quality of Life", "Savings Goals"])
        XCTAssertFalse(choosable.flatMap(\.lines).contains(where: Cards.isPayment))
        XCTAssertEqual(choosable.flatMap(\.lines).count, groups.flatMap(\.lines).count - 1)
    }

    /// The domain keeps a card's payment with the cards, and every other
    /// category out of their group: Edit asks neither, and the row goes
    /// back where it was.
    func testAPaymentStaysWithTheCards() {
        let s = Seen()
        let rows = Arranging.rows(Months.groups(s.snap.groups, s.snap.lines, hidden: false))
        guard let pay = s.line("Chase Sapphire"), let rent = s.line("Rent"), let groceries = s.line("Groceries"),
              let p = rows.firstIndex(where: { $0.id == Arranged.category(pay).id }),
              let r = rows.firstIndex(where: { $0.id == Arranged.category(rent).id }),
              let g = rows.firstIndex(where: { $0.id == Arranged.category(groceries).id })
        else { return XCTFail("the seed's rows") }
        XCTAssertNil(Arranging.drop(rows, from: p, to: r + 1), "a payment under Bills")
        XCTAssertNil(Arranging.drop(rows, from: p, to: rows.count), "a payment at the end")
        XCTAssertNil(Arranging.drop(rows, from: g, to: p), "a category above the payment, in the cards' group")
        XCTAssertNil(Arranging.drop(rows, from: g, to: p + 1), "a category under the payment, in the cards' group")
    }

    /// A payment typed by hand as spending, made a transfer to the card from
    /// its form: the card's payment category spends it, and the form over it
    /// then reads it as the transfer it is. Falsified by `EditTransaction`
    /// sent without `transferTo`: it stayed spending.
    func testAnExistingTransactionIsMadeATransfer() {
        let s = Seen()
        guard let checking = s.account("Checking"), let card = s.account("Chase Sapphire"), let pay = s.line("Chase Sapphire")
        else { return XCTFail("the demo's accounts") }
        s.bridge.hold(register: checking.id)
        s.bridge.settle()
        var d = Draft(account: checking.id, date: Fx.today)
        d.payee = "Chase"
        d.memo = "card payment"
        d.cents = 5_000
        guard let add = d.adding else { return XCTFail("the payment's input") }
        s.write { try $0.addTransaction(add) }
        guard let typed = s.snap.registers[Registers.key(checking.id)]?.first(where: { $0.memo == "card payment" })
        else { return XCTFail("the payment typed") }
        XCTAssertFalse(typed.transfer)

        var e = Draft(typed, accounts: s.snap.accounts)
        XCTAssertFalse(e.transfer)
        XCTAssertNil(e.transferTo)
        e.choose(transfer: card.id)
        XCTAssertEqual(e.edit?.transferTo, card.id)
        XCTAssertNil(e.edit?.categoryId)
        guard let edit = e.edit else { return XCTFail("the edit") }
        s.write { try $0.editTransaction(edit) }
        guard let made = s.snap.registers[Registers.key(checking.id)]?.first(where: { $0.id == typed.id })
        else { return XCTFail("the payment, after") }
        XCTAssertTrue(made.transfer)
        XCTAssertEqual(made.payee, "Transfer : Chase Sapphire")
        XCTAssertEqual(s.account("Chase Sapphire")?.balance, card.balance + 5_000)
        XCTAssertEqual(s.line("Chase Sapphire")?.activity, pay.activity - 5_000)
        XCTAssertEqual(s.notices, [])

        // Opened again, the form knows the transfer and its account; saved
        // as it is, it asks for the same transfer, which changes nothing.
        let again = Draft(made, accounts: s.snap.accounts)
        XCTAssertTrue(again.transfer)
        XCTAssertEqual(again.transferTo, card.id)
        XCTAssertEqual(again.edit?.transferTo, card.id)
        s.bridge.release(register: checking.id)
        s.bridge.settle()
    }

    /// A transfer's account is read from its payee: an open one before a
    /// closed one of the same name, never the line's own account.
    func testATransfersAccountIsReadFromItsPayee() {
        func line(_ payee: String, transfer: Bool = true) -> TxnLine {
            TxnLine(id: Fx.id(1), accountId: Fx.id(2), account: "Checking", date: Fx.today, payee: payee,
                    categoryId: nil, category: "Transfer", group: "", memo: "", amount: -5_000,
                    cleared: .uncleared, approved: true, transfer: transfer, imported: false,
                    importedPayee: "", createdMs: 0, userId: "alex")
        }
        let accounts = [Fx.account(2, "Checking"), Fx.account(3, "Visa", kind: "credit", closed: true),
                        Fx.account(4, "Visa", kind: "credit"), Fx.account(5, "Savings", kind: "savings")]
        XCTAssertEqual(Registers.transferAccount(line("Transfer : Visa"), accounts), Fx.id(4))
        XCTAssertEqual(Registers.transferAccount(line("Transfer : Savings"), accounts), Fx.id(5))
        XCTAssertNil(Registers.transferAccount(line("Transfer : Checking"), accounts))
        XCTAssertNil(Registers.transferAccount(line("Transfer : Visa", transfer: false), accounts))
        XCTAssertNil(Registers.transferAccount(line("Visa"), accounts))
        XCTAssertNil(Draft(line("Visa", transfer: false), accounts: accounts).edit?.transferTo)
    }
}
