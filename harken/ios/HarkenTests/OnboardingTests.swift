// The onboarding's machine with no network and no browser: where a launch
// starts, the server step, and the sign-in that goes by itself — once a
// launch, so a refused or empty sign-in is said and never looped.
@testable import Harken
import Foundation
import XCTest

final class OnboardingTests: XCTestCase {
    private func machine(server: String? = nil, login: Login? = nil) -> Onboarding {
        Onboarding(server: server, login: login) { s, r in "\(s)/auth/login?redirect=\(r)" }
    }

    private let browse = Effect.browse("https://h/auth/login?redirect=harken://auth")

    func testALaunchStartsFromWhatWasRemembered() {
        var m = machine()
        XCTAssertEqual(m.begin(), [])
        XCTAssertEqual(m.step, .start)
        var r = machine(server: "https://h", login: Fx.login)
        XCTAssertEqual(r.begin(), [.openSession(server: "https://h", login: Fx.login)])
        XCTAssertEqual(r.step, .running(server: "https://h", login: Fx.login))
    }

    /// No sign-in button: a server with nobody signed in goes to sign in at
    /// launch. Falsified by starting the sign-in step idle: no browse.
    func testASignInGoesByItselfAtLaunch() {
        var m = machine(server: "https://h")
        XCTAssertEqual(m.begin(), [browse])
        XCTAssertEqual(m.step, .signIn(server: "https://h", phase: .browser, error: nil))
    }

    /// A sign-in that comes back empty is said, and not tried again until the
    /// app comes back to the front. Falsified by browsing again on `cameBack`.
    func testASignInThatComesBackEmptyIsSaidOnceAndNotLooped() {
        var m = machine(server: "https://h")
        _ = m.begin()
        XCTAssertEqual(m.next(.cameBack("closed")), [])
        XCTAssertEqual(m.step, .signIn(server: "https://h", phase: .idle, error: "closed"))
        // Nothing here starts it again: a code that is late, a check, a sync.
        XCTAssertEqual(m.next(.synced), [])
        XCTAssertEqual(m.next(.cameBack("again")), [])
        // The app away and back is the web's reload: once more.
        XCTAssertEqual(m.next(.foreground), [browse])
        XCTAssertEqual(m.next(.cameBack("closed")), [])
        XCTAssertEqual(m.step, .signIn(server: "https://h", phase: .idle, error: "closed"))
    }

    func testACodeIsExchangedThenTheAppOpens() {
        var m = machine(server: "https://h")
        _ = m.begin()
        XCTAssertEqual(m.next(.code("c0de")), [.exchange(server: "https://h", code: "c0de")])
        // A second code while one is traded is nothing.
        XCTAssertEqual(m.next(.code("again")), [])
        XCTAssertEqual(m.next(.signedIn(Fx.login)), [.rememberLogin(Fx.login), .openSession(server: "https://h", login: Fx.login)])
        XCTAssertEqual(m.step, .opening(server: "https://h", login: Fx.login))
        XCTAssertEqual(m.next(.synced), [])
        XCTAssertEqual(m.step, .running(server: "https://h", login: Fx.login))
    }

    /// A failed exchange is said and not retried by itself. Falsified by `go` on failure.
    func testAFailedExchangeIsSaidOnce() {
        var m = machine(server: "https://h")
        _ = m.begin()
        _ = m.next(.code("c"))
        XCTAssertEqual(m.next(.signInFailed("no")), [])
        XCTAssertEqual(m.step, .signIn(server: "https://h", phase: .idle, error: "no"))
    }

    /// Turned away goes back to sign in by itself — once a launch: a login
    /// turned away straight after it was made is not sent round again.
    /// Falsified by resetting `tried` on a denial.
    func testADenialSignsInAgainOnceALaunch() {
        var r = machine(server: "https://h", login: Fx.login)
        _ = r.begin()
        XCTAssertEqual(r.next(.denied("revoked")), [.closeSession, .forgetLogin, browse])
        guard case .signIn(_, .browser, let e?) = r.step else { return XCTFail("\(r.step)") }
        XCTAssertTrue(e.contains("revoked"))
        _ = r.next(.code("c"))
        _ = r.next(.signedIn(Fx.login))
        _ = r.next(.synced)
        XCTAssertEqual(r.next(.denied("again")), [.closeSession, .forgetLogin])
        guard case .signIn(_, .idle, _) = r.step else { return XCTFail("\(r.step)") }
    }

    /// Signing out waits on the page: it is not a sign-in to start again at once.
    func testSigningOutKeepsTheServerAndDoesNotSignInAgain() {
        var r = machine(server: "https://h", login: Fx.login)
        _ = r.begin()
        XCTAssertEqual(r.next(.signOut), [.closeSession, .forgetLogin])
        XCTAssertEqual(r.step, .signIn(server: "https://h", phase: .idle, error: nil))
        XCTAssertEqual(r.next(.foreground), [browse])
    }

    func testTheServerStep() {
        var m = machine()
        _ = m.next(.typed("  "))
        XCTAssertEqual(m.next(.check), [])
        XCTAssertEqual(m.step, .server(typed: "  ", checking: false, error: "Type the address of your server."))
        _ = m.next(.typed("h"))
        XCTAssertEqual(m.next(.check), [.findServer("h")])
        XCTAssertEqual(m.next(.check), [])
        XCTAssertEqual(m.next(.checked(.failed("down"))), [])
        XCTAssertEqual(m.step, .server(typed: "h", checking: false, error: "down"))
        _ = m.next(.check)
        XCTAssertEqual(m.next(.checked(.server("https://h", newer: false))), [.rememberServer("https://h"), browse])
        // Another server, from signing in: the field holds this one.
        _ = m.next(.cameBack("closed"))
        XCTAssertEqual(m.next(.changeServer), [])
        XCTAssertEqual(m.step, .server(typed: "h", checking: false, error: nil))
        // A new server is a new sign-in: it goes by itself though this launch has.
        _ = m.next(.check)
        XCTAssertEqual(m.next(.checked(.server("https://h", newer: false))), [.rememberServer("https://h"), browse])
    }

    func testAddresses() {
        func urls(_ t: String) -> [String] {
            if case let .candidates(u, _) = Address.parse(t) { return u }
            return []
        }
        func why(_ t: String) -> String {
            if case let .invalid(w) = Address.parse(t) { return w }
            return ""
        }
        XCTAssertEqual(urls("music.example.com"), ["https://music.example.com"])
        XCTAssertEqual(urls("  Music.Example.com/  "), ["https://music.example.com"])
        XCTAssertEqual(urls("music.example.com:8443"), ["https://music.example.com:8443"])
        XCTAssertEqual(urls("http://music.example.com"), ["http://music.example.com"])
        XCTAssertEqual(urls("https://music.example.com/harken/"), ["https://music.example.com/harken"])
        XCTAssertEqual(urls("192.168.1.20:8787"), ["https://192.168.1.20:8787", "http://192.168.1.20:8787"])
        XCTAssertEqual(urls("localhost:8787"), ["https://localhost:8787", "http://localhost:8787"])
        XCTAssertEqual(urls("nas"), ["https://nas", "http://nas"])
        XCTAssertEqual(urls("music.local"), ["https://music.local", "http://music.local"])
        XCTAssertEqual(urls("[::1]:8787"), ["https://[::1]:8787", "http://[::1]:8787"])
        XCTAssertEqual(urls("100.101.102.103"), ["https://100.101.102.103", "http://100.101.102.103"])
        XCTAssertEqual(urls("8.8.8.8"), ["https://8.8.8.8"])
        XCTAssertEqual(urls("172.32.0.1"), ["https://172.32.0.1"])
        XCTAssertEqual(why("   "), "Type the address of your server.")
        XCTAssertEqual(why("music example"), "An address has no spaces in it.")
        XCTAssertEqual(why("ftp://music.example.com"), "A server is reached over http or https, not ftp.")
        XCTAssertEqual(why("music.example.com:99999"), "The port has to be a number from 1 to 65535.")
        XCTAssertEqual(why("music.example.com:x"), "The port has to be a number from 1 to 65535.")
        XCTAssertTrue(why("user@music.example.com").hasPrefix("That isn't an address"))
        XCTAssertTrue(why("mu_sic.example.com").hasPrefix("That isn't an address"))
        XCTAssertTrue(why("https://").hasPrefix("That isn't an address"))
    }

    func testHealth() {
        let ours = "0a0b 0.1.0"
        func health(_ d: String) -> String { "ok\nconnections 1\nhead 3\nhorizon 0\ndomain \(d) current\n" }
        XCTAssertEqual(Health.decide("h", status: 200, body: health(ours), domain: ours), .serving(newer: false))
        XCTAssertEqual(Health.decide("h", status: 200, body: health("0a0b 0.2.0"), domain: ours), .serving(newer: false))
        XCTAssertEqual(Health.decide("h", status: 200, body: health("0c0d 0.2.0"), domain: ours), .serving(newer: true))
        XCTAssertEqual(Health.decide("h", status: 200, body: "ok\nconnections 0\n", domain: ours), .refused("h is an Ark server with no app on it yet."))
        XCTAssertEqual(Health.decide("h", status: 200, body: "", domain: ours), .refused("h answered, but not the way an Ark server does."))
        XCTAssertEqual(Health.decide("h", status: 404, body: "", domain: ours), .refused("h answered, but it isn't an Ark server: it has no health page."))
    }

    /// https first, then http, and the first answer decides.
    func testProbing() {
        var asked = [String]()
        let found = wait {
            await Probe.find("nas:8787", domain: "0a0b 0.1.0") { url in
                asked.append(url)
                return url.hasPrefix("https") ? .unreachable("nothing answered there") : .answered(status: 200, body: "ok\ndomain 0a0b 0.1.0 current\n")
            }
        }
        XCTAssertEqual(asked, ["https://nas:8787/healthz", "http://nas:8787/healthz"])
        XCTAssertEqual(found, .server("http://nas:8787", newer: false))
        let none = wait { await Probe.find("nas", domain: "x") { _ in .unreachable("nothing answered there") } }
        XCTAssertEqual(none, .failed("Couldn't reach nas over https or http: nothing answered there."))
    }

    func testTheLoginIsKeptWholeAndAForgottenOneIsNone() {
        let shelf = MemoryShelf()
        let r = Remembered(shelf)
        r.login = Fx.login
        XCTAssertEqual(r.login, Fx.login)
        shelf.put("ark.login", "{not json")
        XCTAssertNil(r.login)
        XCTAssertEqual(Model.code(URL(string: "harken://auth?code=abc")!), "abc")
        XCTAssertNil(Model.code(URL(string: "harken://auth?code=")!))
        XCTAssertNil(Model.code(URL(string: "other://auth?code=abc")!))
    }
}

/// An async answer, waited for: the tests' clock is the thread.
func wait<T>(_ f: @escaping () async -> T) -> T {
    let done = DispatchSemaphore(value: 0)
    let box = Box<T>()
    Task.detached {
        box.value = await f()
        done.signal()
    }
    done.wait()
    return box.value!
}

final class Box<T>: @unchecked Sendable { var value: T? = nil }
