// The first run as this phone carries it: the machine itself is
// `ark_auth::onboarding`'s, its every rule held by its Rust tests; here is
// what is money's — the code and the join link read off its scheme, what is
// remembered on the shelf under the keys it always had, and the login in
// the secure store.
@testable import Money
import ArkUICore
import Foundation
import XCTest

/// The secure store as a map, for a test.
final class MemoryStore: SecureStore, @unchecked Sendable {
    var map = [String: String]()
    func get(key: String) throws -> String? { map[key] }
    func put(key: String, value: String) throws { map[key] = value }
    func remove(key: String) throws { map[key] = nil }
}

final class OnboardingTests: XCTestCase {
    func testTheCodeComesBackOnMoneysScheme() {
        XCTAssertEqual(Probe.code(URL(string: "money://auth?code=abc")!, scheme: AppInfo.money.scheme), "abc")
        XCTAssertNil(Probe.code(URL(string: "harken://auth?code=abc")!, scheme: AppInfo.money.scheme))
        XCTAssertNil(Probe.code(URL(string: "money://auth")!, scheme: AppInfo.money.scheme))
        let link = joinLink(scheme: AppInfo.money.scheme, server: "https://money.koon.us", code: "c")
        XCTAssertEqual(joinOf(link: link), Join(server: "https://money.koon.us", code: "c"))
    }

    /// What is remembered is kept whole: the server and the demo under the
    /// keys they always had, the login in the secure store alone; a login
    /// that no longer reads is none, and signing out removes it.
    func testWhatIsRememberedIsKept() {
        let shelf = MemoryShelf()
        let store = MemoryStore()
        let k = Kept(shelf, secure: store)
        k.remembered = Remembered(server: "https://money.koon.us", login: Fx.login, demo: true)
        XCTAssertEqual(Kept(shelf, secure: store).remembered, Remembered(server: "https://money.koon.us", login: Fx.login, demo: true))
        XCTAssertEqual(shelf.get("money.demo"), "1")
        XCTAssertNil(shelf.get("ark.login"))
        XCTAssertEqual(Array(store.map.keys), ["login"])
        store.map["login"] = "{not json"
        XCTAssertNil(Kept(shelf, secure: store).remembered.login)
        k.remembered = Remembered(server: "https://money.koon.us", login: nil, demo: false)
        XCTAssertTrue(store.map.isEmpty)
    }

    /// A login an older build kept in `UserDefaults` is moved to the secure
    /// store at the first launch, and deleted there.
    func testAnOldLoginIsMovedOnce() {
        let shelf = MemoryShelf()
        let store = MemoryStore()
        shelf.put("ark.server", "https://money.koon.us")
        shelf.put("ark.login", loginJson(login: Fx.login))
        XCTAssertEqual(Kept(shelf, secure: store).remembered.login, Fx.login)
        XCTAssertNil(shelf.get("ark.login"))
        XCTAssertEqual(shelf.get("ark.server"), "https://money.koon.us")
        XCTAssertEqual(Kept(shelf, secure: store).remembered.login, Fx.login)
    }

    /// `/healthz` decides: money's own domain line is a server this app can use.
    func testHealthIsReadAgainstMoneysDomain() {
        let ours = moneyDomain()
        let body = "ok\nconnections 1\ndomain \(ours) current\n"
        XCTAssertEqual(Health.decide("money.koon.us", status: 200, body: body, domain: ours), .serving(newer: false))
        XCTAssertEqual(Health.decide("x", status: 404, body: "", domain: ours), .refused("x answered, but it isn't an Ark server: it has no health page."))
    }
}
