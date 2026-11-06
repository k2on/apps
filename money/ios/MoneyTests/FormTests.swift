// What the screens type and read with no session: amounts in cents, the
// transaction form's inputs, payees offered as one types, and days.
@testable import Money
import ArkUICore
import Foundation
import XCTest

final class FormTests: XCTestCase {
    func testTheLocaleIsReadAsNumberFormatterWritesMoney() {
        let us = LocaleNumbers.of(Locale(identifier: "en_US"))
        XCTAssertEqual(us, LocaleNumbers(currency: "USD", symbol: "$", symbolFirst: true, symbolSpaced: false,
                                         decimal: ".", grouping: ",", minus: "-", minusFirst: true))
        let de = LocaleNumbers.of(Locale(identifier: "de_DE"))
        XCTAssertEqual(de.currency, "EUR")
        XCTAssertFalse(de.symbolFirst)
        XCTAssertTrue(de.symbolSpaced)
        XCTAssertEqual(de.decimal, ",")
        XCTAssertEqual(de.grouping, ".")
    }

    func testAmountsAreKeyedAsApplePayTakesThem() {
        // The keypad: digits from the right, and back.
        var c: Int64 = 0
        for d in [1, 2, 3, 4] { c = Cents.typing(c, digit: d) }
        XCTAssertEqual(c, 1_234)
        XCTAssertEqual(Cents.erasing(c), 123)
    }

    /// The form's sign: an outflow is below nothing. A transfer has no category.
    func testTheFormSaysWhatTheDomainTakes() {
        var d = Draft(account: nil, date: "2026-10-26")
        XCTAssertEqual(d.problem, "Choose an account.")
        d.account = Fx.id(1)
        XCTAssertEqual(d.problem, "Enter an amount.")
        d.cents = 4_200
        XCTAssertNil(d.problem)
        XCTAssertEqual(d.adding?.amount, -4_200)
        d.inflow = true
        XCTAssertEqual(d.adding?.amount, 4_200)
        d.choose(category: Fx.id(7))
        d.choose(transfer: Fx.id(2))
        XCTAssertNil(d.adding?.categoryId)
        XCTAssertEqual(d.adding?.transferTo, Fx.id(2))
        XCTAssertNil(d.edit)
        // A payee chosen brings its category, unless one is chosen.
        var e = Draft(account: Fx.id(1), date: "2026-10-26")
        e.choose(PayeeLine(name: "Blue Bottle", count: 3, categoryId: Fx.id(9)))
        XCTAssertEqual(e.category, Fx.id(9))
        e.choose(PayeeLine(name: "Safeway", count: 9, categoryId: Fx.id(10)))
        XCTAssertEqual(e.category, Fx.id(9))
        XCTAssertEqual(e.payee, "Safeway")
        d.date = "2026-02-30"
        XCTAssertEqual(d.problem, "Choose a day.")
    }

    /// An imported line's form shows what the bank called it, whole, when
    /// that says more than the payee; one typed in, one the bank said
    /// nothing of, and one whose payee says the same in other case show none.
    func testTheFormShowsTheBanksDescription() {
        func line(_ payee: String, bank: String, imported: Bool = true) -> TxnLine {
            TxnLine(id: Fx.id(1), accountId: Fx.id(2), account: "Checking", date: "2026-10-26", payee: payee,
                    categoryId: nil, category: "Needs a category", group: "", memo: "", amount: -1_250,
                    cleared: .cleared, approved: false, transfer: false, imported: imported,
                    importedPayee: bank, createdMs: 0, userId: "alex", receipt: "", photos: 0)
        }
        let full = "CHIPOTLE 1234 SAN FRANCISCO CA"
        XCTAssertEqual(Registers.bankDescription(line("Chipotle", bank: full)), full)
        XCTAssertEqual(Draft(line("Chipotle", bank: "  \(full) ")).bank, full)
        XCTAssertNil(Registers.bankDescription(line("Chipotle", bank: full, imported: false)))
        XCTAssertNil(Registers.bankDescription(line("Chipotle", bank: " ")))
        XCTAssertNil(Registers.bankDescription(line("Chipotle", bank: "CHIPOTLE")))
        XCTAssertNil(Draft(account: Fx.id(2), date: "2026-10-26").bank)
    }

    func testPayeesAreOfferedAsTheyAreTyped() {
        let p = [PayeeLine(name: "Safeway", count: 9, categoryId: nil),
                 PayeeLine(name: "Blue Bottle", count: 3, categoryId: nil),
                 PayeeLine(name: "Bottle Shop", count: 5, categoryId: nil)]
        XCTAssertEqual(Payees.suggest("bo", p).map { $0.name }, ["Bottle Shop", "Blue Bottle"])
        XCTAssertEqual(Payees.suggest("", p, limit: 2).map { $0.name }, ["Safeway", "Bottle Shop"])
        XCTAssertEqual(Payees.suggest("safeway", p).map { $0.name }, [])
    }

    /// A day is noon of it in the phone's calendar, and back, whatever the zone.
    /// Falsified by noon UTC: Auckland read it as the 27th.
    func testDaysRoundTrip() {
        XCTAssertNil(Days.date("2026-13-01"))
        XCTAssertNil(Days.date("2026-02-30"))
        for zone in ["Pacific/Honolulu", "America/Chicago", "UTC", "Asia/Tokyo", "Pacific/Auckland"] {
            var cal = Calendar(identifier: .gregorian)
            cal.timeZone = TimeZone(identifier: zone)!
            guard let d = Days.date("2026-10-26", calendar: cal) else { return XCTFail(zone) }
            XCTAssertEqual(Days.day(d, calendar: cal), "2026-10-26", zone)
        }
        XCTAssertEqual(Months.of("2026-10-26"), "2026-10")
    }
}
