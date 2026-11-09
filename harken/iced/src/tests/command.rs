//! The command box (Ctrl+K): what it finds in the library for what is
//! typed, how its keys walk what it found, and what choosing does.
use arkui::command::Press;

use super::{alone, song};
use crate::places::Source;
use crate::{App, Message};

/// A window alone, with two albums by two people.
fn app() -> App {
    let mut p = alone("me");
    for (t, who, album) in [
        ("Suite 1", "Bach", "Suites"),
        ("Suite 2", "Bach", "Suites"),
        ("Summer", "Vivaldi", "Seasons"),
    ] {
        p.client
            .lock()
            .mutate("add_song", &song(t, who, album, &format!("music/{t}.mp3")))
            .unwrap();
    }
    p.refresh();
    App::with_peer(p, String::new(), None)
}

fn found(app: &App) -> Vec<(String, Vec<String>)> {
    app.command_groups()
        .into_iter()
        .filter(|g| !g.items.is_empty())
        .map(|g| (g.heading, g.items.into_iter().map(|i| i.label).collect()))
        .collect()
}

/// Ctrl+K opens the box, empty and finding nothing; what is typed finds
/// songs, albums and people whose names have it, those starting with it
/// first; Ctrl+K again closes it, and it fades out rather than vanishing.
///
/// Falsified by matching only the start of a name: "ite" found nothing,
/// where "Suite 1" and "Suite 2" have it.
#[test]
fn ctrl_k_opens_the_box_and_what_is_typed_finds_the_library() {
    let mut app = app();
    let _ = app.update(Message::CommandKey(Press::Toggle));
    assert!(app.command.is_some());
    assert!(found(&app).is_empty(), "nothing typed, nothing found");

    let _ = app.update(Message::CommandQuery("su".into()));
    assert_eq!(
        found(&app),
        [
            ("Songs".to_string(), vec!["Suite 1".to_string(), "Suite 2".into(), "Summer".into()]),
            ("Albums".into(), vec!["Suites".into()]),
        ]
    );
    let _ = app.update(Message::CommandQuery("ite".into()));
    assert_eq!(found(&app)[0].1, ["Suite 1", "Suite 2"]);
    let _ = app.update(Message::CommandQuery("vivaldi".into()));
    assert_eq!(
        found(&app).iter().map(|(h, _)| h.as_str()).collect::<Vec<_>>(),
        ["Songs", "Albums", "Artists"]
    );

    let _ = app.update(Message::CommandKey(Press::Toggle));
    assert!(app.command.is_none());
    assert!(app.command_leaving.is_some(), "drawn while it fades out");
}

/// What the window's subscription hears of `text` typed, a key at a time,
/// while no text box has the keyboard (`arkui::command::keys`): before
/// the focus Ctrl+K asked for has landed.
#[cfg(not(feature = "demo"))]
fn typed_before_the_focus(app: &mut App, text: &str) {
    for c in text.chars() {
        let key = iced::keyboard::Key::Character(c.to_lowercase().to_string().into());
        let event = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::empty(),
            text: Some(c.to_string().into()),
            repeat: false,
        });
        if let Some(press) = arkui::command::heard(&event, iced::event::Status::Ignored, app.command.is_some()) {
            let _ = app.update(Message::CommandKey(press));
        }
    }
}

/// Ctrl+K and "Elise" typed at once, as the window opens: before the box's
/// text box has the keys (its focus is a task, landing after the key that
/// opened it), and before the library — read on a thread
/// (`arkui::later`) — has come. The box stays up through the library's
/// arrival with what was typed in it, and finds "Für Elise" in the
/// library that came. What harken's desktop search shot does, on a
/// machine of any speed.
///
/// Falsified twice, each half undone alone: with the window that came
/// replacing this one whole (`*self = app`, as `Message::Opened` did), the
/// box was gone once the library arrived — CI's "\"Für Elise\" never
/// came", reproduced locally with the shots driver; and with
/// `arkui::command::keys` hearing no typing, the query stayed "" and the
/// box found nothing.
#[cfg(not(feature = "demo"))]
#[test]
fn what_is_typed_as_the_window_opens_finds_the_library_that_comes() {
    // The window `App::opening` makes: in memory, empty, saying so.
    let mut opening = crate::client::memory(crate::client::nobody(), None);
    opening.alone();
    let mut app = App::with_peer(super::hand(crate::peer::Peer::open(opening)), String::new(), None);
    app.held = true;

    let _ = app.update(Message::CommandKey(Press::Toggle));
    typed_before_the_focus(&mut app, "Elise");
    assert_eq!(app.command.as_ref().map(|c| c.query.as_str()), Some("Elise"));
    assert!(found(&app).is_empty(), "nothing in the library yet");

    // The replica, read: a library with Für Elise in it.
    let mut library = crate::client::memory(super::who("me"), None);
    library.alone();
    library.set_roles([crate::domain::LIBRARY]);
    library
        .mutate("add_song", &song("Für Elise", "Ludwig van Beethoven", "Bagatelles", "music/elise.mp3"))
        .unwrap();
    let _ = app.update(Message::Opened(None, arkui::later::Slot::new(Ok(library))));
    app.peer.refresh();

    assert_eq!(app.command.as_ref().map(|c| c.query.as_str()), Some("Elise"), "still up, still typed");
    assert_eq!(found(&app), [("Songs".to_string(), vec!["Für Elise".to_string()])]);
}

/// ↓ walks what it found and ⏎ chooses the row the keys are on: an album
/// opens its page, and the box closes. The page's keys are not the box's
/// while it is up.
///
/// Falsified by the box leaving the keys to the page: `j` typed in it moved
/// the page's cursor.
#[test]
fn the_keys_walk_what_it_found_and_choosing_an_album_opens_it() {
    let mut app = app();
    let _ = app.update(Message::CommandKey(Press::Toggle));
    let _ = app.update(Message::CommandQuery("suites".into()));
    let before = app.cursors;
    let _ = app.update(Message::Key(
        iced::keyboard::Key::Character("j".into()),
        iced::keyboard::Modifiers::empty(),
    ));
    assert_eq!(app.cursors, before, "the page's keys are not the box's");

    let _ = app.update(Message::CommandKey(Press::Down));
    let groups = app.command_groups();
    let chosen = app.command.as_ref().unwrap().chosen(&groups).unwrap();
    let _ = app.update(chosen);
    assert!(app.command.is_none());
    assert!(
        matches!(&app.peer.source, Source::Album(_, name) if name == "Suites"),
        "{:?}",
        app.peer.source
    );
}
