//! Everything a click opens on a page, reached by the keys (`nav`): over the
//! demo's Four Seasons, whose page heads each concerto with its composer
//! and its title, names the orchestra under every movement, and has a Play
//! button and the people on it.
use iced::keyboard::{Key, Modifiers};

use crate::nav::Mark;
use crate::places::{Pane, Source};
use crate::{App, Message};

fn press(app: &mut App, c: &str) {
    let _ = app.update(Message::Key(Key::Character(c.into()), Modifiers::empty()));
}

fn album(name: &str) -> App {
    let mut app = super::demo_app();
    let _ = app.update(Message::Select(Source::Album(name.into())));
    app.pane = Pane::Tracks;
    app
}

/// `k` from the first movement climbs to the work's heading, then the
/// Play button and the orchestra in the header; `l` from a movement reaches
/// the orchestra under it; `<Enter>` on the work's title opens the work.
/// Falsified by giving the page only its main list (`layout` answering
/// `Page::single`): `k` from the first movement goes nowhere.
#[test]
fn every_link_on_an_album_is_a_key_away() {
    let mut app = album("The Four Seasons");
    let l = app.layout();
    let work = l.cell(Mark::Work(0)).expect("the first concerto's title is a cell");
    let composer = l.cell(Mark::Composer(0)).expect("…and its composer");
    let play = l.cell(Mark::Play).expect("the Play button");
    assert!(l.cell(Mark::Person(0)).is_some(), "the people on it");

    assert_eq!(app.at(Pane::Tracks), 0);
    press(&mut app, "k");
    assert_eq!(app.at(Pane::Tracks), composer, "up from the first movement: its heading");
    press(&mut app, "l");
    assert_eq!(app.at(Pane::Tracks), work);
    press(&mut app, "k");
    assert_eq!(app.at(Pane::Tracks), play, "then the Play button");

    press(&mut app, "G");
    assert_eq!(app.at(Pane::Tracks), app.peer.rows().len() - 1, "G is still the last movement");
    press(&mut app, "g");
    press(&mut app, "g");
    press(&mut app, "l");
    assert_eq!(Some(app.at(Pane::Tracks)), l.cell(Mark::Performer(0, 0)), "l reaches who played it");

    let _ = app.update(Message::HoverAt(work));
    app.keyed = None;
    let _ = app.update(Message::HoverAt(work));
    let _ = app.update(Message::Key(Key::Named(iced::keyboard::key::Named::Enter), Modifiers::empty()));
    assert!(
        matches!(app.peer.source, Source::Work(..)),
        "<Enter> on the title opens the work: {:?}",
        app.peer.source
    );
}

/// A row sliding under a pointer that has not moved is not a hover: the
/// keys keep the cursor. Falsified by dropping the `keyed` guard in
/// `HoverAt`: the hover pulled the cursor back to row 0.
#[test]
fn a_still_pointer_does_not_take_the_cursor_from_the_keys() {
    let mut app = album("The Four Seasons");
    press(&mut app, "j");
    press(&mut app, "j");
    let _ = app.update(Message::HoverAt(0));
    assert_eq!(app.at(Pane::Tracks), 2, "the keys still have it");
    let _ = app.update(Message::Hover(iced::Point::new(5.0, 5.0)));
    let _ = app.update(Message::HoverAt(0));
    assert_eq!(app.at(Pane::Tracks), 0, "a pointer that moved does");
}

/// `^O` goes back to the page before and `^I` forward again.
#[test]
fn ctrl_o_and_ctrl_i_go_back_and_forward() {
    let mut app = album("The Four Seasons");
    let _ = app.update(Message::Select(Source::Albums));
    let _ = app.update(Message::Key(Key::Character("o".into()), Modifiers::CTRL));
    assert_eq!(app.peer.source, Source::Album("The Four Seasons".into()));
    let _ = app.update(Message::Key(Key::Character("i".into()), Modifiers::CTRL));
    assert_eq!(app.peer.source, Source::Albums);
}

/// A work's performance set apart, and its Play, are cells: `<Enter>` on
/// the Play queues the performance, as its click does. Falsified by
/// marking only `Featured`: the Play had no cell.
#[test]
fn a_featured_play_is_a_key_away() {
    let mut app = super::demo_app();
    let id = crate::domain::host::work_key("Johann Sebastian Bach", "BWV 988", "Goldberg Variations");
    let _ = app.update(Message::Select(Source::Work(id, "Goldberg Variations".into())));
    app.pane = Pane::Tracks;
    let l = app.layout();
    assert!(l.cell(Mark::Featured).is_some(), "the performance set apart");
    let play = l.cell(Mark::FeaturedPlay).expect("…and its Play");
    let _ = app.update(Message::HoverAt(play));
    app.keyed = None;
    let _ = app.update(Message::HoverAt(play));
    let _ = app.update(Message::Key(Key::Named(iced::keyboard::key::Named::Enter), Modifiers::empty()));
    assert!(!app.queue.is_empty(), "<Enter> on the Play queues it");
}
