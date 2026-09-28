//! Where things go, which nothing on screen can check for you.
use iced::{Point, Size};

use crate::{media_url, App, Message, BAR_HEIGHT, DEVICES_WIDTH, PAGE_PADDING, SHELF};
use arkui::menu::{Anchor, Menu};
use arkui::panel::EDGE;

/// The join, and the things that make it more than a `format!`.
///
/// Falsified by returning `file` unchanged: the first assertion fails.
#[test]
fn a_relative_file_is_joined_to_the_media_route() {
    assert_eq!(
        media_url("https://harken.example.com", "music/Bach/air.mp3"),
        "https://harken.example.com/media/music/Bach/air.mp3",
        "the scanner's path is what /media/ serves, so this is the join"
    );
    // A whole URL is already an answer: the demo's library is Wikimedia links.
    let wiki = "https://upload.wikimedia.org/x.mp3";
    assert_eq!(media_url("https://harken.example.com", wiki), wiki);
    // Nothing to stream stays nothing, rather than a URL that 404s.
    assert_eq!(media_url("https://harken.example.com", ""), "");
    assert_eq!(
        media_url("https://harken.example.com/", "music/a.mp3"),
        "https://harken.example.com/media/music/a.mp3"
    );
}

/// Real libraries are full of these, and `#` is the destructive one: a raw
/// one makes the request stop mid-filename and 404.
#[test]
fn the_awkward_characters_in_a_filename_are_escaped() {
    assert_eq!(
        media_url("https://h.example", "music/Ravel/Boléro #1 & 2.mp3"),
        "https://h.example/media/music/Ravel/Bol%C3%A9ro%20%231%20%26%202.mp3"
    );
    assert_eq!(media_url("", "a/b/c.mp3").matches('/').count(), 4, "/media + three segments");
}

/// This window's cards fit their row at every width, and a card that would
/// fit is drawn — against *this* window's pane, which is what the keyboard's
/// grid is built from too.
///
/// The page is 265px narrower than the window: 16 of padding either side,
/// the 200px sidebar, and the 1px rule with 16 either side of it — measured
/// off a screenshot, where the first card starts at x = 265 (16 of the
/// shelf's own padding past the page's edge at 249). A literal, because
/// written as `pane_width()` the test agreed with the bug it was for: the
/// pane once left out all of it but the sidebar, and a seventh card at
/// 1280px was squeezed to fit.
///
/// Falsified by `pane_width` returning the window less the sidebar alone: it
/// fails at 522px, where 2 cards need 280px in 215.
#[test]
fn the_cards_fit_the_page_beside_the_sidebar() {
    let mut app = App::with_peer(super::peer(ark_client::Options::alone("me")), String::new(), None);
    for width in 320..=4000 {
        app.window = Size::new(width as f32, 600.0);
        let n = app.columns() as f32;
        let room = width as f32 - 265.0 - 32.0 - arkui::SCROLLBAR;
        let needed = n * SHELF.card + (n - 1.0) * SHELF.gap;
        assert!(n == 1.0 || needed <= room, "at {width}px {n} cards need {needed}px in {room}px");
        assert!((n + 1.0) * SHELF.card + n * SHELF.gap > room, "at {width}px another card fits");
    }
}

/// The two AppKit rules, through this window's anchor: a right click opens on
/// the pointer, the ⋯ on the ⋯ — and the ⋯ column ends where the list does.
///
/// Falsified by making `dots_right` forget the scrollbar: the last assertion
/// fails by ten pixels.
#[test]
fn a_right_click_opens_on_the_pointer_and_the_dots_open_on_the_dots() {
    let window = Size::new(1280.0, 720.0);
    let wide = arkui::menu::MIN_WIDTH;
    let at = |x: f32, anchor| Menu::<Message>::origin_for(Point::new(x, 300.0), window, 4, anchor, wide);
    assert_eq!(at(220.0, Anchor::Pointer), Point::new(220.0, 300.0));
    assert_eq!(at(600.0, Anchor::Pointer), Point::new(600.0, 300.0));
    let dots = Anchor::RightEdge(App::dots_right(window));
    assert_eq!(at(220.0, dots), at(1240.0, dots), "one button, one place");
    assert_eq!(at(220.0, dots).y, 300.0, "…and how far down is still the row's");
    assert_eq!(at(220.0, dots).x + wide, 1280.0 - 16.0 - 10.0, "the menu ends where the dots do");
}

/// The device picker hangs off the speaker button — the right-hand end of
/// the bar, sitting on the bar's top edge — and nothing about the pointer can
/// move it: `devices_origin` takes no pointer.
///
/// **Said twice, the second time as literals**, because against the
/// constants both sides move together. 628 and 423 are this panel at 860×600
/// with two devices and the stop row, by hand: 860 − 16 − 216, and
/// 600 − 16 − 48 − (8 + 24 + 27 × 3). Falsified by leaving `BAR_HEIGHT` out
/// of `devices_origin`: the literals fail.
#[test]
fn the_device_picker_hangs_off_the_speaker_button() {
    let window = Size::new(860.0, 600.0);
    let at = App::devices_origin(window, 3);
    assert_eq!(at.x + DEVICES_WIDTH, window.width - PAGE_PADDING);
    assert_eq!(at.y + App::devices_height(3), window.height - PAGE_PADDING - BAR_HEIGHT);
    assert_eq!((at.x, at.y), (628.0, 423.0));
}

/// …and it stays on the glass with a house full of speakers on a window too
/// small for them. Falsified by placing it without `hang_above`'s clamp.
#[test]
fn the_device_picker_never_hangs_off_the_glass() {
    for rows in 1..=30 {
        for width in [120.0, 240.0, 860.0, 4000.0] {
            let at = App::devices_origin(Size::new(width, 600.0), rows);
            assert!(
                at.x >= EDGE && at.y >= EDGE,
                "{rows} devices on a {width}px window put the picker at {at:?}"
            );
        }
    }
}
