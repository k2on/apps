//! The accounts signed in on this device ([`DeviceAccounts`]): who a
//! first run could offer to continue as, by name and picture, before any
//! browser opens.
//!
//! A port, so that where they are kept can change without its callers
//! changing. The one implementation today is [`Secured::app_private`]:
//! the app's own secure store (`ark_module::secure_store`, its
//! [`Scope::Private`]), so it lists only the account this app itself
//! signed in. The shared scope, every Ark app's on the device, is the same
//! implementation over [`Scope::Shared`] once a platform has one
//! (ui/docs/app.md, "Signing In": Later, one sign-in per device).
//!
//! Nothing secret is in an entry: what would sign one in again is kept
//! apart, under its own key.

use std::sync::{Arc, Mutex};

use ark_module::secure_store::{self, Scope};
use ark_module::Modules;
use serde::{Deserialize, Serialize};

use crate::keep::ask;

/// One account signed in on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DeviceAccount {
    /// The provider's issuer that vouches for it — or, for an app whose
    /// server is the only one to speak to the provider, that server's
    /// address.
    pub provider: String,
    /// Its stable id at the provider (the `sub`): with `provider`, which
    /// account this is.
    pub subject: String,
    /// How it is shown: the name the provider gave. May be empty.
    #[serde(default)]
    pub name: String,
    /// Its picture's URL, as the login holds it; empty for none.
    #[serde(default)]
    pub picture: String,
    /// The app it was signed in through (`money`, `harken`).
    pub app: String,
}

impl DeviceAccount {
    fn same(&self, provider: &str, subject: &str) -> bool {
        self.provider == provider && self.subject == subject
    }
}

/// The accounts signed in on this device: listed, saved, forgotten.
pub trait DeviceAccounts: Send + Sync {
    /// Every account kept, the most recently saved first.
    fn list(&self) -> Vec<DeviceAccount>;
    /// `account` kept, in place of one with the same provider and subject.
    fn save(&self, account: DeviceAccount);
    /// The account `subject` at `provider` gone; nothing if it was not kept.
    fn forget(&self, provider: &str, subject: &str);
}

/// The list as kept: newest first, one per provider and subject.
fn saved(mut list: Vec<DeviceAccount>, account: DeviceAccount) -> Vec<DeviceAccount> {
    list.retain(|a| !a.same(&account.provider, &account.subject));
    list.insert(0, account);
    list
}

/// The accounts in a secure store, one entry under [`Secured::KEY`] in
/// one scope: the list as JSON. Best-effort, as a list of who to offer is:
/// a store that fails or is not there lists nobody and keeps nothing, and
/// the person signs in as before.
pub struct Secured {
    modules: Modules,
    scope: Scope,
}

impl Secured {
    /// Where the list is kept.
    pub const KEY: &'static str = "ark-auth.device-accounts";

    /// The app's own: its private scope, the secure store `modules` has.
    pub fn app_private(modules: Modules) -> Secured {
        Secured {
            modules,
            scope: Scope::Private,
        }
    }

    fn write(&self, list: &[DeviceAccount]) {
        let op = match list.is_empty() {
            true => secure_store::remove_in(self.scope, Self::KEY),
            false => secure_store::put_in(self.scope, Self::KEY, serde_json::to_string(list).unwrap_or_default()),
        };
        let _ = ask(&self.modules, op);
    }
}

impl DeviceAccounts for Secured {
    /// What cannot be read (a store written by something else) is no
    /// account at all, not a failure.
    fn list(&self) -> Vec<DeviceAccount> {
        let kept = ask(&self.modules, secure_store::get_in(self.scope, Self::KEY)).ok().flatten();
        kept.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save(&self, account: DeviceAccount) {
        self.write(&saved(self.list(), account));
    }

    fn forget(&self, provider: &str, subject: &str) {
        let mut list = self.list();
        list.retain(|a| !a.same(provider, subject));
        self.write(&list);
    }
}

/// The accounts, faked for tests: a list in memory.
#[derive(Clone, Default)]
pub struct Fake {
    accounts: Arc<Mutex<Vec<DeviceAccount>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }
}

impl DeviceAccounts for Fake {
    fn list(&self) -> Vec<DeviceAccount> {
        self.accounts.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn save(&self, account: DeviceAccount) {
        let mut a = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
        *a = saved(std::mem::take(&mut *a), account);
    }

    fn forget(&self, provider: &str, subject: &str) {
        self.accounts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|a| !a.same(provider, subject));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ann(app: &str, name: &str) -> DeviceAccount {
        DeviceAccount {
            provider: "https://idm.example.invalid/oauth2/openid/money".into(),
            subject: "ann-sub".into(),
            name: name.into(),
            picture: String::new(),
            app: app.into(),
        }
    }

    fn bob() -> DeviceAccount {
        DeviceAccount {
            subject: "bob-sub".into(),
            name: "Bob".into(),
            ..ann("money", "")
        }
    }

    /// Both implementations keep the same list: newest first, one per
    /// provider and subject (a second save of Ann replaces her), and a
    /// forgotten account gone, the rest kept.
    fn keeps_one_per_account(accounts: &dyn DeviceAccounts) {
        assert_eq!(accounts.list(), vec![]);
        accounts.save(ann("money", "Ann"));
        accounts.save(bob());
        accounts.save(ann("money", "Ann Example"));
        assert_eq!(accounts.list(), vec![ann("money", "Ann Example"), bob()]);
        accounts.forget(&bob().provider, "bob-sub");
        accounts.forget(&bob().provider, "nobody");
        assert_eq!(accounts.list(), vec![ann("money", "Ann Example")]);
    }

    #[test]
    fn the_fake_keeps_one_per_account() {
        keeps_one_per_account(&Fake::new());
    }

    /// The app's own store holds the list under one key, and none once the
    /// last is forgotten; what it cannot read lists nobody, and with no
    /// store at all there is nobody to offer. Falsified by `write` putting
    /// an empty list rather than removing the key: the key was left behind.
    #[test]
    fn the_apps_own_store_keeps_one_per_account() {
        use ark_module::secure_store::SecureStore;
        let store = ark_secure_store::Fake::new();
        let mut modules = Modules::new();
        store.register(&mut modules);
        let accounts = Secured::app_private(modules);
        keeps_one_per_account(&accounts);
        assert!(store.get(Secured::KEY.into()).unwrap().is_some());
        accounts.forget(&ann("", "").provider, "ann-sub");
        assert_eq!(store.get(Secured::KEY.into()).unwrap(), None);
        store.put(Secured::KEY.into(), "not a list".into()).unwrap();
        assert_eq!(accounts.list(), vec![]);

        let nowhere = Secured::app_private(Modules::new());
        nowhere.save(ann("money", "Ann"));
        assert_eq!(nowhere.list(), vec![]);
    }
}
