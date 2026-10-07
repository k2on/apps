//! `E`: the explorer over this device's replica (`docs/plan-guards.md` D4).
//! Opened and closed by the keyboard, every key its own while it is open,
//! drawn over what the replica holds — and read-only here, since harken
//! exposes no CRUD and a client has no raw write.

use ark_client::Options;
use iced::keyboard::{key::Named, Key, Modifiers};

use crate::{App, Message};

fn press(app: &mut App, c: &str) {
    let _ = app.update(Message::Key(Key::Character(c.into()), Modifiers::empty()));
}

fn press_named(app: &mut App, n: Named) {
    let _ = app.update(Message::Key(Key::Named(n), Modifiers::empty()));
}

/// `E` opens it over the replica's own tables and gives it the keyboard;
/// an edit is refused — nothing exposed, no raw writer — and nothing is
/// authored; the raw switch stays off; `<Esc>` goes back, then out.
/// Falsified by leaving the keys with the window while it is open: `j`
/// moved the window's cursor and not the explorer's.
#[test]
fn e_opens_the_explorer_read_only() {
    let mut app = App::with_peer(super::peer(Options::alone("alice")), String::new(), None);
    app.peer.ensure_playlist();
    let pending = app.peer.client.pending_len();
    press(&mut app, "E");
    assert!(app.explore.is_some(), "E opens the explorer");
    assert!(app.view_explorer().is_some());
    let tables = ark_explorer::Explorer::tables(&ark_explorer::Source {
        store: app.peer.client.store(),
        schema: app.peer.client.schema(),
        module: app.peer.client.domain().module(),
        log: &ark_explorer::NoLog,
        ctx: app.peer.client.ctx().clone(),
    });
    assert!(tables.iter().any(|t| t == "playlist"), "{tables:?}");
    let at = tables.iter().position(|t| t == "playlist").unwrap();
    for _ in 0..at {
        press(&mut app, "j");
    }
    assert_eq!(app.explore.as_ref().unwrap().ui.tables_at, at, "the keys are the explorer's");
    press_named(&mut app, Named::Enter);
    assert_eq!(app.explore.as_ref().unwrap().ui.screen, ark_explorer::Screen::Table("playlist".into()));
    // The window made its default playlist; its name is the second column.
    press(&mut app, "l");
    press(&mut app, "e");
    let _ = app.update(Message::Explore(ark_explorer::Msg::EditText("Renamed".into())));
    let _ = app.update(Message::Explore(ark_explorer::Msg::Commit));
    let x = app.explore.as_ref().unwrap();
    assert!(x.ui.note.contains("not the authority"), "{}", x.ui.note);
    assert_eq!(app.peer.client.pending_len(), pending, "nothing was authored");
    press(&mut app, "R");
    assert!(!app.explore.as_ref().unwrap().ui.raw, "no raw switch on a client");
    let _ = app.update(Message::Explore(ark_explorer::Msg::CancelEdit));
    press_named(&mut app, Named::Escape);
    assert!(app.explore.is_some(), "<Esc> on a table goes back to the tables");
    press_named(&mut app, Named::Escape);
    assert!(app.explore.is_none(), "and from the top, out");
}
