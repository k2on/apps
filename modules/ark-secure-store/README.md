# Secure Store

`ark-secure-store` (`modules/ark-secure-store`): a device module, one package with its crate,
its Swift and its Kotlin (ui/docs/app.md, The Device). An app opts in by
listing `secure-store` in its `modules.toml` and depending on `ark-secure-store`.

Secrets kept by the system: a login's token. A read answers `Text` (none
when nothing is kept under the key); a write or a removal `Done`.

## API

```rust
ark_secure_store::{get, put, remove} // -> SecureStoreOp, output Option<String>: what is kept under the key after
trait SecureStore { fn get(&self, key) -> Option<String>; fn put(&self, key, value); fn remove(&self, key); }
ark_secure_store::Fake // a map
```

## Per Platform

No native implementation yet: the keychain (iPhone, Mac,
`kSecClassGenericPassword`), the Android keystore with
`EncryptedSharedPreferences`. The desktop and the browser answer
`Unavailable` until there is a keyring (Secret Service, the macOS
keychain) or a browser's credential store to use.

## Permissions

None.
