//! The login kept as a shell keeps it: the sign-in machine's
//! `Effect::Remember` carried out through `ark_auth::keep::LoginKeep` over
//! ark-secure-store's fake, and the next launch started from what it kept.

use std::sync::{Arc, Mutex};

use ark_auth::keep::{Before, Kept, LoginKeep};
use ark_auth::onboarding::{login_json, AppInfo, Effect, Event, Login, Onboarding, Remembered};
use ark_module::Modules;
use ark_secure_store::Fake;

const H: &str = "https://h";

fn money() -> AppInfo {
    AppInfo {
        name: "Money".into(),
        scheme: "money".into(),
        demo: true,
        what: "your budget".into(),
    }
}

fn ana() -> Login {
    Login {
        token: "placeholder".into(),
        session: "s1".into(),
        user: "ana".into(),
        expires_ms: 9,
        ..Login::default()
    }
}

/// A phone's preferences, where the login used to be kept.
#[derive(Default)]
struct Prefs(Mutex<Option<String>>);

impl Before for Prefs {
    fn read(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
    fn delete(&self) {
        *self.0.lock().unwrap() = None;
    }
}

/// A shell: the machine's effects carried out, the login kept.
fn carry(keep: &LoginKeep, effects: Vec<Effect>) {
    for e in effects {
        if let Effect::Remember { remembered } = e {
            keep.keep(remembered.login.as_ref());
        }
    }
}

fn launch(keep: &LoginKeep) -> Onboarding {
    let m = Onboarding::new(money());
    let effects = m.next(Event::Launched {
        remembered: Remembered {
            server: Some(H.into()),
            login: keep.recall(),
            demo: false,
        },
        code: None,
        returned: false,
    });
    carry(keep, effects);
    m
}

fn modules(fake: &Fake) -> Modules {
    let mut m = Modules::new();
    fake.register(&mut m);
    m
}

#[test]
fn a_sign_in_is_kept_in_the_secure_store_and_a_sign_out_removes_it() {
    let fake = Fake::new();
    let keep = LoginKeep::new("login", modules(&fake), None);
    let m = launch(&keep);
    carry(&keep, m.next(Event::Code { code: "c".into() }));
    carry(&keep, m.next(Event::SignedIn { login: ana() }));
    assert_eq!(fake.kept("login"), Some(login_json(ana())));
    assert_eq!(keep.last(), Some(Kept::Secure));

    // The next launch is signed in from the store.
    let keep = LoginKeep::new("login", modules(&fake), None);
    let m = launch(&keep);
    assert_eq!(m.remembered().login, Some(ana()));

    carry(&keep, m.next(Event::SignOut));
    assert_eq!(fake.kept("login"), None);
    assert_eq!(LoginKeep::new("login", modules(&fake), None).recall(), None);
}

#[test]
fn a_login_kept_the_old_way_is_moved_at_the_first_launch() {
    let fake = Fake::new();
    let prefs = Arc::new(Prefs(Mutex::new(Some(login_json(ana())))));
    let keep = LoginKeep::new("login", modules(&fake), Some(prefs.clone()));
    let m = launch(&keep);
    assert_eq!(m.remembered().login, Some(ana()), "still signed in");
    assert_eq!(prefs.read(), None, "the old copy deleted");
    assert_eq!(fake.kept("login"), Some(login_json(ana())));
}

#[test]
fn a_store_that_fails_keeps_the_old_copy_and_signs_in_all_the_same() {
    let fake = Fake::new();
    fake.fail("the keyring is locked");
    let prefs = Arc::new(Prefs(Mutex::new(Some(login_json(ana())))));
    let keep = LoginKeep::new("login", modules(&fake), Some(prefs.clone()));
    let m = launch(&keep);
    assert_eq!(m.remembered().login, Some(ana()));
    assert!(prefs.read().is_some(), "moved at a later launch");
    assert_eq!(
        keep.last(),
        Some(Kept::Failed {
            why: "the keyring is locked".into()
        })
    );
}

#[test]
fn with_no_secure_store_a_sign_in_lasts_the_run() {
    let prefs = Arc::new(Prefs::default());
    let keep = LoginKeep::new("login", Modules::new(), Some(prefs.clone()));
    let m = launch(&keep);
    carry(&keep, m.next(Event::Code { code: "c".into() }));
    carry(&keep, m.next(Event::SignedIn { login: ana() }));
    assert_eq!(keep.last(), Some(Kept::ThisRun));
    assert_eq!(keep.recall(), Some(ana()));
    assert_eq!(prefs.read(), None, "never written in ordinary storage");
    // The next launch has nobody signed in, and goes to sign in.
    let keep = LoginKeep::new("login", Modules::new(), Some(prefs));
    assert_eq!(launch(&keep).remembered().login, None);
}
