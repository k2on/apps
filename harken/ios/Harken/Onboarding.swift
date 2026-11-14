// The onboarding is the sign-in machine every Ark app shares
// (`ark_auth::onboarding`, through the bridge): `Onboarding`, its `Step`,
// `Event` and `Effect`, and the sentences it says (`Said`). Its rules — no
// sign-in button: a server with nobody signed in goes to sign in by itself,
// at most once a launch, and again when the app comes back to the front; a
// denial signs in again once; a join link is asked about first — are
// written and tested once, in Rust. The server's address, its `/healthz`
// and the shelf are ArkUI's (`Probe`, `Health`, `Shelf`). What is harken's
// here is who it is to the machine, and what this phone remembers.
import ArkUICore
import Foundation

extension AppInfo {
    /// harken, to the sign-in machine: no demo on the phones.
    static let harken = AppInfo(name: "Harken", scheme: "harken", demo: false, what: "your library")
}

/// What this phone remembers, on a shelf (`UserDefaults`), under the keys it
/// always had: the server; and what is nobody else's business: the last few
/// searches, and whether diagnostics have been unlocked. The login is in the
/// secure store (ark-secure-store: the keychain), through the bridge's
/// `LoginKeep` (`ark_auth::keep`), which moves a login kept on the shelf
/// under `ark.login` there once and deletes it. With no `secure` store, the
/// login lasts this `Kept`.
final class Kept {
    private let shelf: Shelf
    private let login: LoginKeep
    init(_ shelf: Shelf, secure: SecureStore?) {
        self.shelf = shelf
        login = LoginKeep(store: secure, shelf: OldKeys(shelf), oldKey: "ark.login")
    }

    var remembered: Remembered {
        get { Remembered(server: shelf.get("ark.server"), login: login.recall(), demo: false) }
        set {
            shelf.put("ark.server", newValue.server)
            login.keep(login: newValue.login)
        }
    }

    var searches: [String] {
        get { (shelf.get("harken.searches") ?? "").split(separator: "\n").map(String.init).filter { !$0.isEmpty } }
        set { shelf.put("harken.searches", newValue.joined(separator: "\n")) }
    }

    var diagnostics: Bool {
        get { shelf.get("harken.diagnostics") == "1" }
        set { shelf.put("harken.diagnostics", newValue ? "1" : nil) }
    }
}

/// A shelf as where a login used to be kept: read once, then removed.
final class OldKeys: OldShelf, @unchecked Sendable {
    private let shelf: Shelf
    init(_ shelf: Shelf) { self.shelf = shelf }
    func get(key: String) -> String? { shelf.get(key) }
    func remove(key: String) { shelf.put(key, nil) }
}

extension Found {
    /// What ArkUI's probe found, as the machine takes it.
    init(_ f: ArkUICore.Found) {
        switch f {
        case let .server(url, newer): self = .server(url: url, newer: newer)
        case let .failed(why): self = .failed(why: why)
        }
    }
}

/// "Add a Device": a one-time code made at `server` as `login`, off the main
/// thread — the server is asked — as the join link another phone scans or
/// opens, and its QR code (the bridge's `deviceLink`); or why not.
func addDevice(server: String, login: Login?) async -> DeviceLinkShown {
    guard let token = login?.token, !token.isEmpty else { return DeviceLinkShown(link: nil, size: 0, dark: [], error: "Sign in first.") }
    return await Task.detached {
        do {
            let d = try deviceLink(scheme: AppInfo.harken.scheme, server: server, token: token)
            return DeviceLinkShown(link: d.link, size: Int(d.qr.size), dark: d.qr.dark, error: nil)
        } catch {
            return DeviceLinkShown(link: nil, size: 0, dark: [], error: "Couldn't make a code: \(why(error)).")
        }
    }.value
}
