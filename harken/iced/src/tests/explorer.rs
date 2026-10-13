//! `E`: the explorer over this device's replica (`docs/plan-guards.md` D4).
//! Opened and closed by the keyboard, every key its own while it is open,
//! drawn over what the replica holds — and read-only here, since harken
//! exposes no CRUD and a client has no raw write.

use ark_client::ark::store::Store;
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

/// Every query and mutation is callable from the explorer: a mutation from
/// its form is authored through the client as the signed-in person, exactly
/// as a button authors it — the row is alice's — and a query answers from
/// the replica with the row in it; an id field is chosen from the
/// replica's rows. Falsified by authoring through the drawing writer (which
/// writes nothing): the call was refused with "drawing writes nothing" and
/// no playlist appeared.
#[test]
fn a_function_from_the_explorer_is_authored_as_the_signed_in_person() {
    let mut app = App::with_peer(super::peer(Options::alone("alice")), String::new(), None);
    press(&mut app, "E");
    press(&mut app, "f");
    let module = app.peer.client.domain().module().clone();
    let all = ark_explorer::form::callables(&module);
    let at = |name: &str| all.iter().position(|c| c.name == name).unwrap();
    let explore = |app: &mut App, m: ark_explorer::Msg| {
        let _ = app.update(Message::Explore(m));
    };
    explore(&mut app, ark_explorer::Msg::Function(at("create_playlist")));
    explore(&mut app, ark_explorer::Msg::FormText(vec![0], "From the explorer".into()));
    explore(&mut app, ark_explorer::Msg::Call);
    let made = app
        .peer
        .client
        .store()
        .scan("playlist")
        .into_iter()
        .find(|r| r.get("name") == Some(&ark_client::Value::text("From the explorer")))
        .expect("the playlist was authored");
    assert_eq!(made.get("user_id"), Some(&ark_client::Value::text("alice")));
    let note = &app.explore.as_ref().unwrap().ui.note;
    assert!(note.starts_with("create_playlist: "), "{note}");
    assert!(!note.contains("refused"), "{note}");
    // The query sees it.
    explore(&mut app, ark_explorer::Msg::Function(at("playlists")));
    explore(&mut app, ark_explorer::Msg::Call);
    let ui = &app.explore.as_ref().unwrap().ui;
    let g = ui.answer_grid().expect("an answer");
    let name = g.columns.iter().position(|c| c == "name").unwrap();
    assert!(
        g.rows.iter().any(|r| r[name] == ark_client::Value::text("From the explorer")),
        "{:?}",
        g.rows
    );
    // An id is chosen from the replica's playlists.
    explore(&mut app, ark_explorer::Msg::Function(at("add_all_to_playlist")));
    explore(&mut app, ark_explorer::Msg::Pick(vec![0]));
    explore(&mut app, ark_explorer::Msg::PickFilter("from the".into()));
    explore(&mut app, ark_explorer::Msg::Picked(0));
    let ui = &mut app.explore.as_mut().unwrap().ui;
    let (args, bad) = ui.form().unwrap().read();
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(args["playlist_id"], made.get("id").cloned().unwrap());
}
