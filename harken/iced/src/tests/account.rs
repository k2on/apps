//! The account at the top right: what its menu offers in each state, that
//! its entries do what the top bar's buttons did, and that nobody is ever
//! left to press "sign in" — the window goes by itself, once a load. The
//! demo has none of it, and is alone.
#![cfg_attr(feature = "demo", allow(unused_imports, dead_code))]
use ark_auth::{Account, Login};
use ark_peer::Event;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use super::{alone, signed_in, signed_out, Queues};
use crate::auth::{arrival, Arrival};
use crate::places::Focus;
use crate::{App, Message, ACCOUNT_BOTTOM, PAGE_PADDING};

fn login(who: &str) -> Login {
    Login {
        token: format!("token-{who}"),
        session: format!("session-{who}"),
        user: Account {
            id: who.into(),
            name: "Alice Liddell".into(),
            ..Account::default()
        },
        expires_ms: 0,
    }
}

fn with_picture(who: &str) -> Login {
    let mut l = login(who);
    l.user.picture = "https://pictures.example.invalid/alice.png".into();
    l
}

fn labels(app: &App) -> Vec<String> {
    app.account()
        .map(|a| a.entries.iter().map(|e| e.label.clone()).collect())
        .unwrap_or_default()
}

fn key(app: &mut App, k: Key) {
    let _ = app.update(Message::Key(k, Modifiers::empty()));
}

/// What the menu offers: signed in, signing out; turned away, signing out
/// under a header that says so; alone, nothing but where things are kept;
/// nobody, no menu.
///
/// Falsified by `App::account` building the signed-in menu for a login whose
/// token is empty: the header says "Signed in as".
#[test]
#[cfg(not(feature = "demo"))]
fn the_menu_says_what_the_account_can_do() {
    let mut app = App::with_peer(signed_in("alice"), "http://127.0.0.1:9".into(), Some(login("alice")));
    let a = app.account().expect("a menu for somebody signed in");
    assert_eq!(a.header, "Signed in as Alice Liddell");
    assert_eq!(a.who.map(|w| w.initials()).as_deref(), Some("AL"));
    if let Some(l) = &mut app.login {
        l.token.clear();
    }
    assert_eq!(labels(&app), ["Sign out"]);
    let a = app.account().unwrap();
    assert!(a.header.contains("signed out by the server"), "{}", a.header);

    let nobody = App::with_peer(signed_out(), "http://127.0.0.1:9".into(), None);
    assert!(nobody.account().is_none(), "no menu, and no sign-in button, for nobody");

    let here = App::with_peer(alone("me"), String::new(), None);
    let a = here.account().expect("alone says so");
    assert!(a.entries.is_empty(), "nothing to sign out of");
    assert!(a.header.starts_with("Alone"), "{}", a.header);
    assert!(a.who.is_none(), "nobody's face");
}

/// The button is the person's picture, loaded through the server — one
/// address for the desktop and the page, with the picture's version in it
/// — and asked for once; with no picture there is nothing to ask for, and
/// the face is their initials.
///
/// Falsified by `App::account_picture_url` answering the provider's URL itself: it
/// does not start with the server's `/auth/picture`.
#[test]
#[cfg(not(feature = "demo"))]
fn the_button_is_the_persons_picture_through_the_server() {
    let app = App::with_peer(signed_in("alice"), "http://127.0.0.1:9".into(), Some(login("alice")));
    assert_eq!(app.account_picture_url(), None, "no picture, nothing to load");
    assert!(app.account_picture().is_none());

    let mut app = App::with_peer(signed_in("alice"), "http://127.0.0.1:9".into(), Some(with_picture("alice")));
    let url = app.account_picture_url().expect("a picture to load");
    assert!(url.starts_with("http://127.0.0.1:9/auth/picture?user=alice&v="), "{url}");
    let _ = app.update(Message::AccountMenu);
    assert!(app.covers.want(url).is_none(), "it was not asked for");
    assert!(app.account_picture().is_none(), "not arrived yet: the initials are drawn");
}

/// Choosing "Sign out" from the menu does what the button did: the session
/// is revoked through the domain and the login is gone. The menu hangs off
/// the button's bottom right, has the keys while it is up, and closes when
/// its entry has run.
///
/// Falsified by dropping `SignOut` from the messages that close a menu: the
/// menu is still up after `Sign out`.
#[test]
#[cfg(not(feature = "demo"))]
fn the_menu_signs_out() {
    let mut app = App::with_peer(signed_in("alice"), "http://127.0.0.1:9".into(), Some(login("alice")));
    let _ = app.update(Message::AccountMenu);
    let m = app.ctx.menu.as_ref().expect("the account menu opened");
    assert_eq!(
        m.origin.x + m.width,
        app.window.width - PAGE_PADDING,
        "its right edge is not the button's"
    );
    assert_eq!(m.origin.y, ACCOUNT_BOTTOM, "it does not open down from the button");
    assert_eq!(app.focus(), Focus::Menu);
    assert_eq!(app.menu_about, None, "it is about no row");

    // By key: `j` stays on `Sign out`, the one entry; `<Enter>`.
    key(&mut app, Key::Character("j".into()));
    key(&mut app, Key::Named(Named::Enter));
    assert!(app.login.is_none(), "still signed in");
    assert!(app.peer.client.lock().is_signed_out());
    let funcs: Vec<String> = app.peer.client.lock().log.pending.iter().map(|p| p.entry.func.clone()).collect();
    assert_eq!(funcs.last().map(String::as_str), Some("session.revoke"), "{funcs:?}");
    assert!(app.ctx.menu.is_none());
    let _ = app.update(Message::AccountMenu);
    assert!(app.ctx.menu.is_none(), "a menu for nobody");
}

/// Nobody is asked to sign in: a window with a server and no login goes, and
/// a server turning the login away sends it again — **once a load**. A
/// sign-in that fails, or a page that comes back without a code, says why in
/// the status line and does not go again, or the window would go round and
/// round; a reload starts afresh (`auth::came_back` takes its mark).
///
/// Falsified by `sign_in_unasked` ignoring `sign_in_tried`: after the failed
/// sign-in the note is replaced by "signing in — …": `it went again`.
#[test]
#[cfg(not(feature = "demo"))]
fn a_sign_in_goes_by_itself_once_a_load() {
    assert_eq!(arrival(Some("c".into()), true), Arrival::Exchange("c".into()));
    assert_eq!(arrival(None, true), Arrival::CameBack);
    assert_eq!(arrival(None, false), Arrival::Go);

    // Turned away by the server: straight back to sign in.
    let mut app = App::with_peer(signed_in("alice"), "http://127.0.0.1:9".into(), Some(login("alice")));
    let made: std::sync::Arc<std::sync::Mutex<Vec<Queues>>> = Default::default();
    app.peer.client.lock().connect_with("ws://queues", Queues::dialer(made.clone()));
    let _ = app.update(Message::Due);
    let q = made.lock().unwrap()[0].clone();
    q.push(Event::Open);
    q.push(Event::Closed("denied: expired".into()));
    let _ = app.update(Message::Wake);
    assert!(app.login.as_ref().is_some_and(|l| l.token.is_empty()), "the login was not turned away");
    assert!(app.sign_in_tried, "it did not go to sign in");
    assert!(app.note.starts_with("signing in"), "{}", app.note);

    // …which fails: said once, and not tried again.
    let _ = app.update(Message::SignedIn(Err("the provider refused".into())));
    let said = "could not sign in: the provider refused \u{2014} reload to try again";
    assert_eq!(app.note, said);
    let _ = app.sign_in_unasked();
    assert_eq!(app.note, said, "it went again");

    // A page back from signing in with nothing stays, and says so.
    let mut page = App::with_peer(signed_out(), "http://127.0.0.1:9".into(), None);
    let _ = page.arrive_signing_in(arrival(None, true));
    assert!(page.note.contains("without a login"), "{}", page.note);
    let _ = page.arrive_signing_in(arrival(None, false));
    assert!(page.note.contains("without a login"), "it went again: {}", page.note);

    // A fresh load with nobody goes.
    let mut fresh = App::with_peer(signed_out(), "http://127.0.0.1:9".into(), None);
    let _ = fresh.arrive_signing_in(arrival(None, false));
    assert!(fresh.sign_in_tried && fresh.note.starts_with("signing in"), "{}", fresh.note);
}

/// The demo is alone, and says it is a demo: a header, and nothing to sign
/// in to or out of.
///
/// Falsified by dropping the demo's arm from `App::account`: the demo's
/// replica is its own authority, so it reads `Alone — …`.
#[test]
#[cfg(feature = "demo")]
fn the_demo_is_alone() {
    let app = super::demo_app();
    let a = app.account().expect("the demo has an account button");
    assert!(a.entries.is_empty(), "{:?}", a.entries);
    assert!(a.header.starts_with("A demo"), "{}", a.header);
}

/// Settings, from the account menu: its page has a route of its own, and
/// its Resync asks the server for a full look — its music walked again and
/// its catalogue synced — which only the library's role may ask: without
/// it, Settings says why. Falsified by `Resync` authoring nothing: no
/// edit was pending; and by the menu's entry opening another page.
#[test]
#[cfg(not(feature = "demo"))]
fn settings_resyncs_the_library_for_the_library_alone() {
    use crate::places::{Place, Source};
    use arkui::route::Route;
    assert_eq!(Place::parse(&Place::Settings.fragment()), Place::Settings, "#settings is a page");
    let mut app = App::with_peer(signed_in("alice"), "http://127.0.0.1:9".into(), Some(login("alice")));
    let entry = app.account().unwrap().entries.remove(0);
    let _ = app.update(entry.message().clone());
    assert_eq!(app.peer.source, Source::Settings, "the menu's first entry opens Settings");
    let _ = app.update(Message::Resync);
    assert_eq!(app.edits.last().map(|e| e.what.as_str()), Some("resync the library"));
    assert!(app.resync.as_deref().unwrap_or("").starts_with("Asked"), "{:?}", app.resync);

    // Somebody without the library's role.
    let plain = super::hand(crate::peer::Peer::open(crate::client::memory(super::who("bob"), Some("dev"))));
    let mut app = App::with_peer(plain, "http://127.0.0.1:9".into(), Some(login("bob")));
    let _ = app.update(Message::Select(Source::Settings));
    let _ = app.update(Message::Resync);
    assert!(app.edits.is_empty(), "nothing asked");
    assert!(
        app.resync.as_deref().unwrap_or("").contains("could not resync the library"),
        "{:?}",
        app.resync
    );
}
