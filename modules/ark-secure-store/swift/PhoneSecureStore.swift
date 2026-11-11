// ark-secure-store on the iPhone, the iPad and the Mac
// (modules/ark-secure-store/README.md): the `SecureStore` protocol UniFFI
// writes, over the keychain. Each secret is a generic password whose
// service is the app's bundle id and whose account is the key, readable
// after the device's first unlock and on this device only: never synced
// to iCloud Keychain, never in a backup another device restores.
import Foundation
import Security
// The app's bindings, where nix compiles this as a module of its own over
// them (flake.nix, `iosApp`); an app's own Xcode project compiles it beside
// them, and there is no such module. Public for the app it is linked into.
#if canImport(ArkBindings)
import ArkBindings
#endif

/// The keychain, under the app's bundle id.
public final class PhoneSecureStore: SecureStore, @unchecked Sendable {
    private let service: String
    #if os(macOS)
    /// The data protection keychain, as on the iPhone, while the app is
    /// signed with a team that gives it one; the login keychain, which has
    /// no such entitlement to ask for, when it is not (a local build).
    private var dataProtection = true
    private let lock = NSLock()
    #endif

    public init(service: String = Bundle.main.bundleIdentifier ?? "ark") {
        self.service = service
    }

    public func get(key: String) throws -> String? {
        var q = query(key)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        var status = SecItemCopyMatching(q as CFDictionary, &out)
        if fellBack(status) {
            q = query(key)
            q[kSecReturnData as String] = true
            q[kSecMatchLimit as String] = kSecMatchLimitOne
            status = SecItemCopyMatching(q as CFDictionary, &out)
        }
        switch status {
        case errSecSuccess:
            guard let data = out as? Data else { return nil }
            return String(data: data, encoding: .utf8)
        case errSecItemNotFound:
            return nil
        default:
            throw failed(status)
        }
    }

    public func put(key: String, value: String) throws {
        let data = Data(value.utf8)
        let update: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        var status = SecItemUpdate(query(key) as CFDictionary, update as CFDictionary)
        if fellBack(status) { status = SecItemUpdate(query(key) as CFDictionary, update as CFDictionary) }
        if status == errSecItemNotFound {
            var add = query(key)
            add.merge(update) { _, new in new }
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw failed(status) }
    }

    public func remove(key: String) throws {
        var status = SecItemDelete(query(key) as CFDictionary)
        if fellBack(status) { status = SecItemDelete(query(key) as CFDictionary) }
        guard status == errSecSuccess || status == errSecItemNotFound else { throw failed(status) }
    }

    /// The item `key` names: a generic password of this app's service.
    private func query(_ key: String) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        #if os(macOS)
        lock.lock()
        let dp = dataProtection
        lock.unlock()
        if dp { q[kSecUseDataProtectionKeychain as String] = true }
        #endif
        return q
    }

    /// Whether `status` says the data protection keychain isn't this app's
    /// (a Mac build signed with no team), so the call is made again on the
    /// login keychain; never on the iPhone.
    private func fellBack(_ status: OSStatus) -> Bool {
        #if os(macOS)
        guard status == errSecMissingEntitlement else { return false }
        lock.lock()
        defer { lock.unlock() }
        let was = dataProtection
        dataProtection = false
        return was
        #else
        return false
        #endif
    }

    /// Why, in the keychain's words; never the secret.
    private func failed(_ status: OSStatus) -> SecureStoreError {
        let said = SecCopyErrorMessageString(status, nil) as String? ?? "error \(status)"
        return .Failed(why: "the keychain: \(said)")
    }
}
