# Secure Store

`ark-secure-store` (`modules/ark-secure-store`): a device module, one package with its crate,
its Swift and its Kotlin (ui/docs/app.md, The Device). An app opts in by
depending on `ark-secure-store` (ui/docs/app.md, How an App Opts In).

Secrets kept by the system: a login's token. A read answers the secret
(none when nothing is kept under the key); a write the value; a removal
none. Every app's login goes through it (`ark_auth::keep`, below); what is
not a secret — the server's address, the demo, the pages seen — stays in
the app's ordinary storage.

## API

```rust
ark_app::modules::secure_store::{get, put, remove}  // -> SecureStoreOp, output Option<String>: what is kept under the key after
ark_app::modules::secure_store::{get_in, put_in, remove_in}(Scope, ..) // the same, in a scope
trait SecureStore { fn get(&self, key) -> Answer<Option<String>>; fn put(&self, key, value) -> Answer<()>; fn remove(&self, key) -> Answer<()>; }
ark_secure_store::Fake                 // a map; .kept(key), .fail(why)
ark_secure_store::phone::SecureStore   // the foreign trait (`ffi`), its errors SecureStoreError::{Unavailable, Failed}
ark_secure_store::SecretService        // the Linux desktop's (`desktop`)
```

The port is the runtime's (`ark_module::secure_store`), as every module's
is; this crate implements it. An operation prints its key, never its
secret.

### Scopes

Each operation is in a `Scope`:

- `Scope::Private` (what `get`, `put` and `remove` ask): this app's alone.
  It is what every platform below implements.
- `Scope::Shared`: readable by every Ark app on the device, for a
  device-wide sign-in. Not built yet: the port answers it `Unavailable` on
  every platform, before any store is asked. When it is, a shared store is
  registered beside the private one, and it will need:
  - **iPhone, iPad, Mac:** a keychain access group every Ark app's
    entitlements list (`keychain-access-groups`, the team's prefix), the
    items put with `kSecAttrAccessGroup`; so a team-signed build.
  - **Android:** an `AccountManager` account type of the apps' own (an
    authenticator service), or a content provider guarded by a
    `signature`-level permission, so only apps signed with the same key
    read it — which needs a private release key first: today's key is
    committed and public, so a signature permission would protect nothing.
  - **Linux desktop:** one Secret Service item every app reads: attributes
    of no single app (`application` = `ark`).
  - **Browser:** none; a page's session is its own server's cookie.

### The Login: `ark_auth::keep`

A shell keeps its login with `ark_auth::keep::LoginKeep` (the phones,
through the bridge's `LoginKeep`, over their `PhoneSecureStore`) or
`ServerLogins` (a desktop, one per server, `ark_peer::Logins::secured`):
`recall` at a launch, `keep` for each `Effect::Remember`. The login is
`ark_auth::Login`'s JSON under the key `login` (a desktop's
`login <server>`).

- **Moved once.** A login an older build kept in ordinary storage — the
  phones' `UserDefaults` or `SharedPreferences` (`ark.login`,
  `money.login`), the desktop's `~/.config/<app>/logins/` — is put in the
  secure store at the first launch that finds it, and the old copy deleted
  once it is (the desktop's whole logins directory: its log would keep a
  forgotten row). A store that fails just then leaves it for the next launch.
- **No secure store** (`Unavailable`): the login lasts the run, never
  written to ordinary storage, and the old copy is deleted all the same.
  The next launch has nobody signed in and goes to sign in by itself, as
  any such launch does (`ark_auth::onboarding`).

## Per Platform

| Platform | Implementation | What it does |
|---|---|---|
| iPhone, iPad | `swift/PhoneSecureStore.swift` | the keychain: a `kSecClassGenericPassword` whose service is the bundle id and whose account is the key, `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` (not synced, not restored onto another device). On the simulator alone, a build not signed at all (nix's: the screenshots' and the tests'), which has no keychain (`errSecMissingEntitlement`), keeps its secrets in a file in the app's own Application Support instead; a device build has no such fallback compiled in |
| Mac | `swift/PhoneSecureStore.swift` | the same, in the data protection keychain; a build signed with no team, which has none, falls back to the login keychain |
| Android | `kotlin/dev/arkui/modules/PhoneSecureStore.kt` | an AES-256-GCM key made in the Android keystore (`ark-secure-store`), each secret's IV and ciphertext in the private preferences file `ark-secure-store`, the key's name its associated data; the file kept out of backups and device-to-device transfers (module.toml's `backup-exclude`, which ui/prebuild.py writes into the data-extraction rules). A ciphertext the keystore can no longer open (its key lost) reads as none and is deleted |
| Desktop (Linux) | `SecretService` (feature `desktop`) | the Secret Service on the session bus (GNOME Keyring, KWallet, KeePassXC), over zbus: an item of the default collection, attributes `application` and `key`, a locked collection unlocked through the service's prompt. No session bus, or no Secret Service on it: `Unavailable` |
| Desktop (other) | none | `Unavailable` |
| Browser | none | `Unavailable`. money's page signs in with an `HttpOnly` cookie, which no script reads, and keeps no token |

Android uses the keystore and a `Cipher` directly, not Jetpack Security's
`EncryptedSharedPreferences`: that is deprecated, and would be a Maven
dependency (and the offline Gradle lock moved) for what is one key and one
preferences file here.

## Permissions

None. (The Mac's data protection keychain wants a team-signed build; the
desktop's prompt is the keyring's own.)
