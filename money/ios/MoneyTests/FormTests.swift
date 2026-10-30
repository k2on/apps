// What the screens type and read with no session: amounts in cents, the
// transaction form's inputs, payees offered as one types, and days.
@testable import Money
import ArkUICore
import Foundation
import XCTest

final class FormTests: XCTestCase {
    func testAmountsAreReadAsTheDesktopReadsThem() {
        XCTAssertEqual(Cents.parse("12"), 1_200)
        XCTAssertEqual(Cents.parse("12.5"), 1_250)
        XCTAssertEqual(Cents.parse("$1,200.00"), 120_000)
        XCTAssertEqual(Cents.parse("-3.10"), -310)
        XCTAssertEqual(Cents.parse(""), 0)
        XCTAssertNil(Cents.parse("1.234"))
        XCTAssertNil(Cents.parse("twelve"))
        XCTAssertEqual(Cents.editable(123_450), "1234.50")
        XCTAssertEqual(Cents.editable(0), "")
        XCTAssertEqual(Cents.plain(-123_456_789), "-$1,234,567.89")
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

    func testPayeesAreOfferedAsTheyAreTyped() {
        let p = [PayeeLine(name: "Safeway", count: 9, categoryId: nil),
                 PayeeLine(name: "Blue Bottle", count: 3, categoryId: nil),
                 PayeeLine(name: "Bottle Shop", count: 5, categoryId: nil)]
        XCTAssertEqual(Payees.suggest("bo", p).map { $0.name }, ["Bottle Shop", "Blue Bottle"])
        XCTAssertEqual(Payees.suggest("", p, limit: 2).map { $0.name }, ["Safeway", "Bottle Shop"])
        XCTAssertEqual(Payees.suggest("safeway", p).map { $0.name }, [])
    }

    /// A day is noon UTC, the same calendar day in every zone a person lives in.
    func testDaysRoundTrip() {
        XCTAssertNil(Days.date("2026-13-01"))
        guard let d = Days.date("2026-10-26") else { return XCTFail("a day") }
        for zone in ["Pacific/Honolulu", "America/Chicago", "UTC", "Asia/Tokyo", "Pacific/Auckland"] {
            var cal = Calendar(identifier: .gregorian)
            cal.timeZone = TimeZone(identifier: zone)!
            XCTAssertEqual(Days.day(d, calendar: cal), "2026-10-26", zone)
        }
        XCTAssertEqual(Months.of("2026-10-26"), "2026-10")
    }
}
