// The first run as this phone carries it: the machine itself is
// `ark_auth::onboarding`'s, its every rule held by its Rust tests; here is
// what is money's — the code and the join link read off its scheme, what is
// remembered on the shelf under the keys it always had.
@testable import Money
import ArkUICore
import Foundation
import XCTest

final class OnboardingTests: XCTestCase {
    func testTheCodeComesBackOnMoneysScheme() {
        XCTAssertEqual(Probe.code(URL(string: "money://auth?code=abc")!, scheme: AppInfo.money.scheme), "abc")
        XCTAssertNil(Probe.code(URL(string: "harken://auth?code=abc")!, scheme: AppInfo.money.scheme))
        XCTAssertNil(Probe.code(URL(string: "money://auth")!, scheme: AppInfo.money.scheme))
        let link = joinLink(scheme: AppInfo.money.scheme, server: "https://money.koon.us", code: "c")
        XCTAssertEqual(joinOf(link: link), Join(server: "https://money.koon.us", code: "c"))
    }

    /// What is remembered is kept whole, under the keys it always had, and a
    /// login that no longer reads is none.
    func testWhatIsRememberedIsKept() {
        let shelf = MemoryShelf()
        let k = Kept(shelf)
        k.remembered = Remembered(server: "https://money.koon.us", login: Fx.login, demo: true)
        XCTAssertEqual(Kept(shelf).remembered, Remembered(server: "https://money.koon.us", login: Fx.login, demo: true))
        XCTAssertEqual(shelf.get("money.demo"), "1")
        shelf.put("ark.login", "{not json")
        XCTAssertNil(Kept(shelf).remembered.login)
    }

    /// `/healthz` decides: money's own domain line is a server this app can use.
    func testHealthIsReadAgainstMoneysDomain() {
        let ours = moneyDomain()
        let body = "ok\nconnections 1\ndomain \(ours) current\n"
        XCTAssertEqual(Health.decide("money.koon.us", status: 200, body: body, domain: ours), .serving(newer: false))
        XCTAssertEqual(Health.decide("x", status: 404, body: "", domain: ours), .refused("x answered, but it isn't an Ark server: it has no health page."))
    }
}
