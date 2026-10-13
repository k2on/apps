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

fn point(app: &mut App, x: f32, y: f32) {
    let _ = app.update(Message::Pointer(iced::Point::new(x, y)));
}

fn click(app: &mut App, x: f32, y: f32) {
    point(app, x, y);
    let _ = app.update(Message::Pressed);
    let _ = app.update(Message::Released);
}

/// `E` docks the explorer beside the app rather than over it: the app keeps
/// its page, its width less the panel's; the keys go to the explorer, and
/// `` ` `` or a click moves them to the app and back — a click into the
/// app ending an edit in progress there; `E` from either half closes it.
/// Falsified by giving the app the whole window while the panel is open:
/// the app's width was the window's, so its geometry would have laid cards
/// under the panel.
#[test]
fn e_docks_the_explorer_beside_a_working_app() {
    let mut app = App::with_peer(super::peer(Options::alone("alice")), String::new(), None);
    app.peer.ensure_playlist();
    let _ = app.update(Message::Resized(iced::Size::new(1280.0, 800.0)));
    assert_eq!(app.window.width, 1280.0);
    press(&mut app, "E");
    let panel = app.dock.width_in(app.screen);
    assert!(
        panel >= crate::explore::DOCK_MIN && app.window.width >= crate::explore::APP_MIN,
        "{panel}"
    );
    assert_eq!(
        app.window.width,
        1280.0 - panel - crate::explore::DOCK_RULE,
        "the app is what the panel leaves"
    );
    assert!(app.explorer_has_keys());
    press(&mut app, "j");
    assert_eq!(app.explore.as_ref().unwrap().ui.tables_at, 1, "the explorer has the keys");
    // `` ` ``: the app has them, and works — `j` moves its own cursor.
    press(&mut app, "`");
    assert!(!app.explorer_has_keys());
    assert!(
        !app.explore.as_ref().unwrap().ui.has_keys,
        "and the explorer draws its cursor as one without them"
    );
    press(&mut app, "j");
    press(&mut app, "?");
    assert!(app.help, "the app's own keys reach it");
    press(&mut app, "?");
    assert_eq!(app.explore.as_ref().unwrap().ui.tables_at, 1, "and the explorer's cursor did not move");
    press(&mut app, "`");
    assert!(app.explorer_has_keys());
    // A click in the app takes the keys there, and ends an edit in progress
    // the way a click away from a cell does.
    let _ = app.update(Message::Explore(ark_explorer::Msg::Open("playlist".into())));
    let _ = app.update(Message::Explore(ark_explorer::Msg::Cell(0, 1)));
    let _ = app.update(Message::Explore(ark_explorer::Msg::Edit));
    assert!(app.explore.as_ref().unwrap().ui.editing.is_some());
    click(&mut app, 200.0, 300.0);
    assert!(!app.explorer_has_keys(), "a click in the app gives it the keys");
    assert!(app.explore.as_ref().unwrap().ui.editing.is_none(), "and ends the edit");
    click(&mut app, 1200.0, 300.0);
    assert!(app.explorer_has_keys(), "a click in the panel takes them back");
    // The app's own messages work beside it.
    let _ = app.update(Message::Select(crate::places::Source::Albums));
    assert!(app.explore.is_some(), "the explorer stays open while the app is used");
    // `E` from the app's half closes it, and the app has the window back.
    press(&mut app, "`");
    press(&mut app, "E");
    assert!(app.explore.is_none());
    assert_eq!(app.window.width, 1280.0);
}

/// The panel's edge drags to resize it, within its bounds and the app's, and
/// the width is kept for the session: closed and opened again, the panel is
/// as wide as it was left. Its close button closes it. Falsified by
/// forgetting the width on close: it came back at its first share of the
/// window.
#[test]
fn the_docked_explorer_resizes_and_remembers_its_width() {
    let mut app = App::with_peer(super::peer(Options::alone("alice")), String::new(), None);
    let _ = app.update(Message::Resized(iced::Size::new(1280.0, 800.0)));
    press(&mut app, "E");
    let edge = app.dock.edge(app.screen);
    point(&mut app, edge + 2.0, 400.0);
    let _ = app.update(Message::Pressed);
    assert!(app.dock.dragging, "a press on the edge starts a drag");
    point(&mut app, 700.0, 400.0);
    let _ = app.update(Message::Released);
    let width = app.dock.width_in(app.screen);
    assert_eq!(width, 1280.0 - 700.0 - crate::explore::DOCK_RULE);
    assert_eq!(app.window.width, 700.0);
    // Past the app's least width, it stops.
    let edge = app.dock.edge(app.screen);
    point(&mut app, edge, 400.0);
    let _ = app.update(Message::Pressed);
    point(&mut app, 10.0, 400.0);
    let _ = app.update(Message::Released);
    assert_eq!(app.window.width, crate::explore::APP_MIN);
    let edge = app.dock.edge(app.screen);
    point(&mut app, edge, 400.0);
    let _ = app.update(Message::Pressed);
    point(&mut app, 700.0, 400.0);
    let _ = app.update(Message::Released);
    let _ = app.update(Message::CloseExplorer);
    assert!(app.explore.is_none(), "the close button closes it");
    press(&mut app, "E");
    assert_eq!(app.dock.width_in(app.screen), width, "as wide as it was left");
}
