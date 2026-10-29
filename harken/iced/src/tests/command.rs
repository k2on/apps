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
