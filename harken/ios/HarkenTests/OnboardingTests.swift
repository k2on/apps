// The onboarding as this phone carries it: the machine itself is
// `ark_auth::onboarding`'s, its every rule held by its Rust tests; here is
// what is harken's — the server step's health and probing (ArkUI's), the
// code and the join link read off its scheme, and what is remembered on
// the shelf under the keys it always had.
@testable import Harken
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
        let store = MemoryStore()
        let k = Kept(shelf, secure: store)
        var pictured = Fx.login
        pictured.picture = "https://pictures.example.invalid/ana.png"
        k.remembered = Remembered(server: "https://h", login: pictured, demo: false)
        XCTAssertEqual(Kept(shelf, secure: store).remembered.login, pictured)
        XCTAssertEqual(shelf.get("ark.server"), "https://h")
        // The login in the secure store alone.
        XCTAssertNil(shelf.get("ark.login"))
        store.map["login"] = "{not json"
        XCTAssertNil(k.remembered.login)
        // One an older build kept in `UserDefaults` is moved once, and deleted there.
        shelf.put("ark.login", loginJson(login: pictured))
        XCTAssertEqual(Kept(shelf, secure: store).remembered.login, pictured)
        XCTAssertNil(shelf.get("ark.login"))
        XCTAssertEqual(store.map["login"], loginJson(login: pictured))
        XCTAssertEqual(Model.code(URL(string: "harken://auth?code=abc")!), "abc")
        XCTAssertNil(Model.code(URL(string: "harken://auth?code=")!))
        XCTAssertNil(Model.code(URL(string: "other://auth?code=abc")!))
        let link = joinLink(scheme: AppInfo.harken.scheme, server: "https://h", code: nil)
        XCTAssertEqual(joinOf(link: link), Join(server: "https://h", code: nil))
    }

    /// A face without a picture says the first and last words' initials, as
    /// the desktop's and Android's do. Falsified by taking the second word's: `JS`.
    func testAFaceSaysWhoByItsInitials() {
        XCTAssertEqual(Pictures.initials("Ada"), "A")
        XCTAssertEqual(Pictures.initials("Johann Sebastian Bach"), "JB")
        XCTAssertEqual(Pictures.initials("  ada   lovelace "), "AL")
        XCTAssertEqual(Pictures.initials("user-0123"), "U")
        XCTAssertEqual(Pictures.initials(""), "")
        XCTAssertEqual(Pictures.initials("\u{e9}mile (\u{2014})"), "\u{c9}")
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
