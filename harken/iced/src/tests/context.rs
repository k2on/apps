//! Which grid the keyboard is in, and where a menu goes — through this
//! window, over the demo's library. arkui holds the rules and tests them on
//! their own; these hold harken's composition of them: the panes under the
//! overlays, the device picker above them, the hover guard, the dwell
//! handshake through `update`, and a menu closing when its entry goes
//! somewhere.
use iced::Size;

use crate::places::{Focus, Pane, Source};
use crate::{Anchor, App, Message, NEW_PLAYLIST};
use arkui::fit::tail;
use arkui::panel::{EDGE, PADDING};
use arkui::picker::Picker;
use arkui::vim;

/// The demo's own boot: a seeded replica and nothing else.
fn app() -> App {
    let mut app = super::demo_app();
    app.pane = Pane::Tracks;
    app
}

fn row(app: &App, at: usize) -> ark_client::Id {
    app.peer.rows()[at].id
}

/// Every demo track's menu is as wide as its longest entry, so nothing in any
/// of them is cut — and at least one needed more than the minimum, or this
/// proves nothing about growing.
///
/// Falsified by pinning the menu at `MIN_WIDTH` after it opens (a menu that
/// cannot grow): it fails on the first entry that no longer fits.
#[test]
fn a_menu_is_as_wide_as_its_longest_entry() {
    let mut app = app();
    let ids: Vec<_> = app.peer.rows().iter().map(|i| i.id).collect();
    let mut widest: f32 = 0.0;
    for id in ids {
        let _ = app.update(Message::RowMenu(id, Anchor::Pointer));
        let menu = app.ctx.menu.as_ref().expect("the menu opened");
        widest = widest.max(menu.width);
        for entry in &menu.entries {
            assert_eq!(
                tail(&entry.label, menu.label_chars()),
                entry.label,
                "{:?} is cut in a menu {}px wide",
                entry.label,
                menu.width
            );
        }
    }
    assert!(
        widest > arkui::menu::MIN_WIDTH,
        "no demo track earned a menu wider than the minimum ({widest}px)"
    );
}

/// One keyboard, one cursor drawn: a menu takes the keys off the pane, the
/// submenu off the menu, the device picker off everything — and closing hands
/// them back one layer at a time.
///
/// Falsified by answering `Focus::Pane` in `focus` whenever the device picker
/// is up: the third assertion fails.
#[test]
fn a_context_window_takes_the_keyboard_from_the_grid_behind() {
    let mut app = app();
    assert_eq!(app.focus(), Focus::Pane(Pane::Tracks));
    assert!(app.has_keys(Pane::Tracks), "nothing is over the page");

    let _ = app.update(Message::RowMenu(row(&app, 3), Anchor::Pointer));
    assert_eq!(app.focus(), Focus::Menu, "the menu has it now");
    assert!(!app.has_keys(Pane::Tracks) && !app.has_keys(Pane::Sidebar), "…so no pane does");
    let _ = app.update(Message::OpenPicker);
    assert_eq!(app.focus(), Focus::Picker);
    let _ = app.update(Message::ClosePicker);
    assert_eq!(app.focus(), Focus::Menu, "back to the parent");
    let _ = app.update(Message::CloseMenu);
    assert_eq!(app.focus(), Focus::Pane(Pane::Tracks), "and back to the list");

    let _ = app.update(Message::OpenDevices);
    assert_eq!(app.focus(), Focus::Devices, "the device picker is above everything");
    let _ = app.act(vim::Action::Cancel);
    assert_eq!(app.focus(), Focus::Pane(Pane::Tracks));
}

/// The selection moves to what the menu is about. Falsified by dropping the
/// `position` block in the `RowMenu` handler: the cursor stays at 0.
#[test]
fn opening_a_row_menu_lands_the_cursor_on_that_row() {
    let mut app = app();
    app.pane = Pane::Sidebar;
    let id = row(&app, 5);
    let _ = app.update(Message::RowMenu(id, Anchor::Pointer));
    assert_eq!(app.at(Pane::Tracks), 5, "the cursor moved to the row");
    assert_eq!(app.pane, Pane::Tracks, "…and to the pane it is in");
    assert_eq!(app.menu_about, Some(id));
    let _ = app.update(Message::CloseMenu);
    assert_eq!(app.at(Pane::Tracks), 5, "closing it leaves the cursor there");
}

/// A submenu opens by being pointed at, through this window's clock: a
/// crossing is not a rest, a rest opens it *shown* (the entry keeps the keys),
/// `<Esc>`-like closing stays closed while the cursor rests there, and
/// leaving and coming back offers it again.
///
/// Numbers, not `DWELL`. Falsified by removing the `ctx.tick()` call from
/// `App::tick`: "resting on it opens it" fails.
#[test]
fn a_submenu_opens_by_being_pointed_at_and_not_by_being_passed_over() {
    let mut app = app();
    let _ = app.update(Message::RowMenu(row(&app, 2), Anchor::Pointer));
    assert!(app.ctx.menu.as_ref().unwrap().entries[1].opens_submenu(), "entry 1 is Add to playlist");
    let _ = app.update(Message::MenuAt(1));
    let _ = app.update(Message::Tick);
    assert!(app.ctx.picker.is_none(), "a sweep across is not a rest on");
    for _ in 0..20 {
        let _ = app.update(Message::Tick);
    }
    assert!(app.ctx.picker.is_some(), "…and resting on it opens it");
    assert_eq!(app.focus(), Focus::Menu, "shown, not entered: the entry still has the keys");
    assert_eq!(app.ctx.picker.as_ref().unwrap().choices.len(), 3, "the demo's three playlists");

    let _ = app.update(Message::ClosePicker);
    for _ in 0..20 {
        let _ = app.update(Message::Tick);
    }
    assert!(app.ctx.picker.is_none(), "closed stays closed while you are on it");

    let _ = app.update(Message::MenuAt(0));
    let _ = app.update(Message::MenuAt(1));
    for _ in 0..20 {
        let _ = app.update(Message::Tick);
    }
    assert!(app.ctx.picker.is_some(), "and it is offered a second time");
    let _ = app.update(Message::PickerAt(0));
    assert_eq!(app.focus(), Focus::Picker, "the pointer reaching a row hands the keys over");
}

/// `l` steps into a submenu and `h` back out, through this window's `act`;
/// `l` on an entry with no submenu is the nothing `l` in a list is.
///
/// Falsified by routing `Focus::Menu` to the panes' travel in `App::travel`:
/// `l` opens nothing.
#[test]
fn l_and_h_walk_into_a_submenu_and_back_out() {
    let mut app = app();
    let _ = app.update(Message::RowMenu(row(&app, 2), Anchor::Dots));
    let _ = app.update(Message::MenuAt(1));
    let _ = app.act(vim::Action::Move(vim::Motion::Right(1)));
    assert!(app.ctx.picker.is_some(), "`l` opened it");
    assert_eq!(app.focus(), Focus::Picker, "…and stepped in");
    let _ = app.act(vim::Action::Move(vim::Motion::Left(1)));
    assert_eq!(app.focus(), Focus::Menu, "`h` stepped back out");
    assert!(app.ctx.picker.is_some(), "…and left the panel up");

    let mut bare = self::app();
    let _ = bare.update(Message::RowMenu(row(&bare, 2), Anchor::Dots));
    let _ = bare.update(Message::MenuAt(0));
    let _ = bare.act(vim::Action::Move(vim::Motion::Right(1)));
    assert!(bare.ctx.picker.is_none(), "`Play` has nowhere to go");
}

/// The page behind a context window does not follow the pointer: a hover
/// falls through every backdrop, so the window refuses it.
///
/// Falsified by dropping the `focus()` guard on `HoverAt`.
#[test]
fn the_page_behind_a_context_window_does_not_follow_the_pointer() {
    let mut app = app();
    let _ = app.update(Message::RowMenu(row(&app, 2), Anchor::Pointer));
    let _ = app.update(Message::HoverAt(9));
    assert_eq!(app.at(Pane::Tracks), 2, "the menu is still about row 2");
    let _ = app.update(Message::OpenDevices);
    let _ = app.update(Message::CloseMenu);
    let _ = app.update(Message::HoverAt(9));
    assert_eq!(app.at(Pane::Tracks), 2, "…nor behind the device picker");
    let _ = app.update(Message::CloseDevices);
    let _ = app.update(Message::HoverAt(9));
    assert_eq!(app.at(Pane::Tracks), 9, "with nothing over it, hovering is what it was");
}

/// `<Esc>` takes the menu and its submenu together, from either side of the
/// keyboard; `a`'s picker alone is only itself.
///
/// Falsified by making the Cancel arm of `act` call `ctx.close_picker()`: the
/// menu is still up after the first `<Esc>`.
#[test]
fn escape_closes_a_menu_and_its_submenu_together() {
    for entered in [false, true] {
        let mut app = app();
        let _ = app.update(Message::RowMenu(row(&app, 2), Anchor::Pointer));
        let _ = app.update(Message::MenuAt(1));
        let _ = app.update(Message::MenuActivate);
        assert!(app.ctx.picker.is_some());
        if !entered {
            app.ctx.picker.as_mut().unwrap().keys = false;
        }
        let _ = app.act(vim::Action::Cancel);
        assert!(app.ctx.picker.is_none() && app.ctx.menu.is_none(), "both went (entered: {entered})");
        assert_eq!(app.focus(), Focus::Pane(Pane::Tracks));
    }
    let mut app = app();
    let _ = app.update(Message::OpenPicker);
    assert!(app.ctx.picker.is_some() && app.ctx.menu.is_none());
    let _ = app.act(vim::Action::Cancel);
    assert!(app.ctx.picker.is_none());
    assert_eq!(app.focus(), Focus::Pane(Pane::Tracks));
}

/// An entry that goes somewhere closes the menu that offered it — and the
/// submenu hanging off it — rather than leaving it up over the page it went
/// to. Play is the same.
///
/// Falsified by deleting the `close_menu` before the match in `step`: the
/// menu is still up on the album page.
#[test]
fn going_somewhere_from_a_menu_closes_it() {
    let mut app = app();
    let id = row(&app, 2);
    let _ = app.update(Message::RowMenu(id, Anchor::Pointer));
    let goto = app
        .ctx
        .menu
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .position(|e| e.label.starts_with("Go to"))
        .expect("a Go to entry");
    let _ = app.update(Message::MenuAt(1));
    let _ = app.update(Message::MenuActivate);
    assert!(app.ctx.picker.is_some(), "the submenu is up");
    let _ = app.update(Message::MenuAt(goto));
    let _ = app.update(Message::MenuActivate);
    assert!(app.ctx.menu.is_none() && app.ctx.picker.is_none(), "the menu and its submenu went");
    assert!(
        matches!(app.peer.source, Source::Album(_) | Source::Artist(_)),
        "…and the page went where it said"
    );

    let _ = app.update(Message::Select(Source::Library));
    let _ = app.update(Message::RowMenu(row(&app, 2), Anchor::Pointer));
    let _ = app.update(Message::MenuActivate);
    assert!(app.ctx.menu.is_none(), "Play is an entry that goes somewhere too");
}

/// A submenu opens beside its parent, never over it, and laps it by exactly
/// one padding — so the menu's lit row touches its edge. Said as a literal
/// too: against the constant the lap holds nothing.
///
/// Falsified by handing `ctx.open_picker` a window twice as wide as the real
/// one: the submenu stops flipping and runs off the right-hand edge.
#[test]
fn a_submenu_opens_beside_its_parent_and_never_over_it() {
    let mut app = app();
    let id = row(&app, 2);
    for width in (320..=3000).step_by(7) {
        app.window = Size::new(width as f32, 720.0);
        let _ = app.update(Message::CloseMenu);
        let _ = app.update(Message::RowMenu(id, Anchor::Dots));
        let _ = app.update(Message::MenuAt(1));
        let _ = app.update(Message::MenuActivate);
        let menu = app.ctx.menu.as_ref().unwrap();
        let panel = app.ctx.picker.as_ref().unwrap();
        let at = panel.origin.unwrap();
        assert!(at.x >= EDGE, "at {width}px it starts at {}", at.x);
        if width >= 600 {
            assert!(at.x + panel.width <= width as f32 - EDGE, "at {width}px it runs off the right-hand edge");
            let right = at.x > menu.origin.x;
            let lit = if right {
                menu.origin.x + menu.width - PADDING
            } else {
                menu.origin.x + PADDING
            };
            let edge = if right { at.x } else { at.x + panel.width };
            assert_eq!(edge, lit, "at {width}px the submenu's edge does not meet the menu's highlight");
            let lap = if right {
                menu.origin.x + menu.width - at.x
            } else {
                at.x + panel.width - menu.origin.x
            };
            assert_eq!(lap, 4.0, "at {width}px it laps the menu by {lap}");
        }
    }
}

/// …and it slides up to fit rather than flipping over its parent, level
/// with its entry by the rows when there is room. Falsified by handing
/// `ctx.open_picker` a window ten times as tall: it never slides, and the
/// last row is off the bottom.
#[test]
fn a_submenu_slides_to_fit_rather_than_flipping_over_its_parent() {
    let mut app = app();
    let id = row(&app, 2);
    app.window = Size::new(1280.0, 900.0);
    app.cursor.y = 120.0;
    let _ = app.update(Message::RowMenu(id, Anchor::Dots));
    let _ = app.update(Message::MenuAt(1));
    let _ = app.update(Message::MenuActivate);
    let top = app.ctx.menu.as_ref().unwrap().entry_top();
    assert_eq!(
        app.ctx.picker.as_ref().unwrap().origin.unwrap().y + PADDING,
        top,
        "the first row is level with the entry"
    );

    let height = app.ctx.picker.as_ref().unwrap().submenu_height();
    for h in 200..=900 {
        app.window = Size::new(1280.0, h as f32);
        let _ = app.update(Message::CloseMenu);
        let _ = app.update(Message::RowMenu(id, Anchor::Dots));
        let _ = app.update(Message::MenuAt(1));
        let _ = app.update(Message::MenuActivate);
        let y = app.ctx.picker.as_ref().unwrap().origin.unwrap().y;
        assert!(y >= EDGE, "at {h}px tall it starts at {y}");
        assert!(y + PADDING <= app.ctx.menu.as_ref().unwrap().entry_top(), "it slid *down* at {h}px tall");
        if height + EDGE * 2.0 <= h as f32 {
            assert!(y + height <= h as f32 - EDGE, "at {h}px tall the last row is off the bottom");
        }
    }
}

/// The submenu is as wide as the demo's longest playlist name and no wider,
/// and the panel is drawn at the width it was measured for. Falsified by
/// opening the picker at a fixed `MAX_WIDTH`: the last assertion fails.
#[test]
fn a_submenu_is_as_wide_as_its_longest_name() {
    let mut app = app();
    let _ = app.update(Message::RowMenu(row(&app, 2), Anchor::Dots));
    let _ = app.update(Message::MenuAt(1));
    let _ = app.update(Message::MenuActivate);
    let picker = app.ctx.picker.as_ref().unwrap();
    let narrow = Picker::width_for(&picker.choices, Some(NEW_PLAYLIST));
    assert!(narrow < arkui::picker::MAX_WIDTH, "Favorites and Piano do not need {narrow}px");
    assert!(narrow >= arkui::picker::MIN_WIDTH);
    assert_eq!(picker.width, narrow, "drawn at the width it was measured for");
}

/// `<Enter>` on a card opens it and on a row plays it, with no mode to be in;
/// and `h` at a grid's left edge hands the cursor to the sidebar.
///
/// Falsified by making `card_under_cursor` answer `None` on `Albums`: the
/// first assertion fails and `<Enter>` starts a track.
#[test]
fn enter_opens_a_card_and_h_leaves_a_grid() {
    let mut app = app();
    let _ = app.update(Message::Select(Source::Albums));
    let first = app.peer.albums[0].name.clone();
    let _ = app.act(vim::Action::Activate);
    assert_eq!(app.peer.source, Source::Album(first), "a card is a place");
    let _ = app.update(Message::Select(Source::Albums));
    let _ = app.act(vim::Action::Move(vim::Motion::Right(1)));
    assert_eq!(app.at(Pane::Tracks), 1, "`l` walks along a shelf");
    let _ = app.act(vim::Action::Move(vim::Motion::Left(1)));
    let _ = app.act(vim::Action::Move(vim::Motion::Left(1)));
    assert_eq!(app.pane, Pane::Sidebar, "and `h` off its left edge is the sidebar");
}
