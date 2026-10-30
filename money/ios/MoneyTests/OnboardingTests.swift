// The first run's machine with no network and no browser: where a launch
// starts, the household's server already in the field, the demo, and the
// sign-in that goes by itself — once a launch.
@testable import Money
import ArkUICore
import Foundation
import XCTest

final class OnboardingTests: XCTestCase {
    private func machine(server: String? = nil, login: Login? = nil, demo: Bool = false) -> Onboarding {
        Onboarding(server: server, login: login, demo: demo) { s, r in "\(s)/auth/login?redirect=\(r)" }
    }

    func testALaunchStartsFromWhatWasRemembered() {
        var m = machine()
        XCTAssertEqual(m.begin(), [])
        XCTAssertEqual(m.step, .server(typed: "money.koon.us", checking: false, error: nil))
        var r = machine(server: "https://money.koon.us", login: Fx.login)
        XCTAssertEqual(r.begin(), [.openSession(server: "https://money.koon.us", login: Fx.login)])
        var d = machine(server: "https://money.koon.us", demo: true)
        XCTAssertEqual(d.begin(), [.openDemo])
        XCTAssertEqual(d.step, .demo)
    }

    func testTheDemoNeedsNoServer() {
        var m = machine()
        XCTAssertEqual(m.next(.tryDemo), [.rememberDemo(true), .openDemo])
        XCTAssertEqual(m.step, .demo)
        // Leaving it is the first page again, the demo forgotten.
        XCTAssertEqual(m.next(.signOut), [.closeSession, .rememberDemo(false)])
        XCTAssertEqual(m.step, .start)
    }

    func testAServerFoundGoesToSignInByItselfOnce() {
        var m = machine()
        XCTAssertEqual(m.next(.check), [.findServer("money.koon.us")])
        let browse = Effect.browse("https://money.koon.us/auth/login?redirect=money://auth")
        XCTAssertEqual(m.next(.checked(.server("https://money.koon.us", newer: false))), [.rememberServer("https://money.koon.us"), browse])
        XCTAssertEqual(m.next(.cameBack("closed")), [])
        XCTAssertEqual(m.step, .signIn(server: "https://money.koon.us", phase: .idle, error: "closed"))
        XCTAssertEqual(m.next(.foreground), [browse])
        XCTAssertEqual(m.next(.code("c0de")), [.exchange(server: "https://money.koon.us", code: "c0de")])
        XCTAssertEqual(m.next(.signedIn(Fx.login)), [.rememberLogin(Fx.login), .openSession(server: "https://money.koon.us", login: Fx.login)])
        XCTAssertEqual(m.next(.synced), [])
        XCTAssertEqual(m.step, .running(server: "https://money.koon.us", login: Fx.login))
        XCTAssertEqual(m.next(.signOut), [.closeSession, .rememberLogin(nil)])
    }

    func testTheCodeComesBackOnMoneysScheme() {
        XCTAssertEqual(Probe.code(URL(string: "money://auth?code=abc")!, scheme: Onboarding.scheme), "abc")
        XCTAssertNil(Probe.code(URL(string: "harken://auth?code=abc")!, scheme: Onboarding.scheme))
        XCTAssertNil(Probe.code(URL(string: "money://auth")!, scheme: Onboarding.scheme))
        let shelf = MemoryShelf()
        let r = Remembered(shelf)
        r.login = Fx.login
        r.demo = true
        XCTAssertEqual(Remembered(shelf).login, Fx.login)
        XCTAssertTrue(Remembered(shelf).demo)
    }

    /// `/healthz` decides: money's own domain line is a server this app can use.
    func testHealthIsReadAgainstMoneysDomain() {
        let ours = moneyDomain()
        let body = "ok\nconnections 1\ndomain \(ours) current\n"
        XCTAssertEqual(Health.decide("money.koon.us", status: 200, body: body, domain: ours), .serving(newer: false))
        XCTAssertEqual(Health.decide("x", status: 404, body: "", domain: ours), .refused("x answered, but it isn't an Ark server: it has no health page."))
    }
}
