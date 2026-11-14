//! Everything a click opens on a page, reached by the keys (`nav`): over the
//! demo's Four Seasons, whose page heads each concerto with its composer
//! and its title, names the orchestra under every movement, and has a Play
//! button and the people on it.
use iced::keyboard::{Key, Modifiers};

use crate::nav::Mark;
use crate::routes::{Pane, Route};
use crate::{App, Message};

fn press(app: &mut App, c: &str) {
    let _ = app.update(Message::Key(Key::Character(c.into()), Modifiers::empty()));
}

fn album(name: &str) -> App {
    let mut app = super::demo_app();
    let id = app.peer.album_id(name);
    let _ = app.update(Message::Select(Route::Album { key: id, title: name.into() }));
    app.pane = Pane::Tracks;
    app
}

/// The keys walk the rows and the items, and not the names and buttons
/// around them: from the first movement `k` and `l` go nowhere (the work's
/// heading, the Play button and the orchestra under it are the pointer's),
/// and `j` from the last movement reaches the people on the record, whose
/// `<Enter>` opens their page. Falsified by `Mark::reachable` answering
/// true for every mark: `k` climbed to the work's heading.
#[test]
fn the_keys_walk_rows_and_items_not_links() {
    let mut app = album("The Four Seasons");
    let l = app.layout();
    assert!(l.cell(Mark::Work(0)).is_some(), "the concerto's title still opens it");
    assert!(l.cell(Mark::Play).is_some(), "…and the Play button plays");
    let face = l.cell(Mark::Person(0)).expect("the people on it");

    assert_eq!(app.at(Pane::Tracks), 0);
    press(&mut app, "k");
    assert_eq!(app.at(Pane::Tracks), 0, "nothing above the first movement is walked onto");
    press(&mut app, "l");
    assert_eq!(app.at(Pane::Tracks), 0, "nor who played it, beside it");

    press(&mut app, "G");
    assert_eq!(app.at(Pane::Tracks), app.peer.rows().len() - 1, "G is still the last movement");
    press(&mut app, "j");
    assert_eq!(app.at(Pane::Tracks), face, "then the faces under the list");
    let _ = app.update(Message::Key(Key::Named(iced::keyboard::key::Named::Enter), Modifiers::empty()));
    assert!(
        matches!(app.peer.source, Route::Artist { .. }),
        "<Enter> on a face opens them: {:?}",
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
    let _ = app.update(Message::Select(Route::Albums));
    let _ = app.update(Message::Key(Key::Character("o".into()), Modifiers::CTRL));
    assert_eq!(app.peer.source.title(), "The Four Seasons");
    let _ = app.update(Message::Key(Key::Character("i".into()), Modifiers::CTRL));
    assert_eq!(app.peer.source, Route::Albums);
}

/// A work's performance set apart, and its Play, are cells: `<Enter>` on
/// the Play queues the performance, as its click does. Falsified by
/// marking only `Featured`: the Play had no cell.
#[test]
fn a_featured_play_is_a_key_away() {
    let mut app = super::demo_app();
    let id = crate::domain::host::work_key("Johann Sebastian Bach", "BWV 988", "Goldberg Variations");
    let _ = app.update(Message::Select(Route::Work {
        key: id,
        title: "Goldberg Variations".into(),
    }));
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

/// A prefix half typed opens the which-key panel once its delay goes by,
/// listing what follows it; finishing it closes the panel, and `zz` leaves
/// the cursor on its row. `?` opens the panel for every key, and `<Esc>`
/// closes it. Falsified by `?` still toggling the old keymap page: nothing
/// was showing.
#[test]
fn a_prefix_shows_what_can_follow_it() {
    let mut app = album("The Four Seasons");
    press(&mut app, "j");
    press(&mut app, "z");
    assert_eq!(app.which.showing(), None, "not before the delay");
    let key = app.which.waiting().expect("a timer for the panel");
    let _ = app.update(Message::WhichDue(key));
    assert_eq!(app.which.showing(), Some("z"));
    let entries = arkui::which::entries(&[arkui::vim::GRAMMAR, crate::KEYS], &[arkui::vim::GROUPS], "z");
    assert!(entries.iter().any(|(k, what)| k == "z" && what.contains("middle")), "{entries:?}");
    press(&mut app, "z");
    assert_eq!(app.which.showing(), None, "zz done: gone");
    assert_eq!(app.at(Pane::Tracks), 1, "the cursor stays on its row");

    press(&mut app, "?");
    assert_eq!(app.which.showing(), Some(""), "every key");
    let _ = app.update(Message::Key(Key::Named(iced::keyboard::key::Named::Escape), Modifiers::empty()));
    assert_eq!(app.which.showing(), None);
}
