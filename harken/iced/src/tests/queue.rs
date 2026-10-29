//! The queue panel and the queue's edits, through this window: which rows
//! the panel draws for the session, what each key does on a row, which
//! transition each edit calls — and, a window alone, what each does to its
//! own queue. The session is set by hand, as the volume tests set it: these
//! are the window's rules over a session, and the transitions' own are the
//! domain's tests.
use arkui::vim;

use crate::domain::Id;

use super::{alone, device, song};
use crate::listening::{Active, Clock, Kind, Listening, Output, Track};
use crate::places::Focus;
use crate::{App, Message};

fn track(t: &str, n: u8) -> Track {
    Track {
        id: Id::new([n; 16]),
        title: t.into(),
        creator: "Bach".into(),
        album: String::new(),
        duration_ms: 300_000,
        file: format!("music/{t}.mp3"),
    }
}

/// A window part of a session of four tracks, on the second.
fn app() -> App {
    let mut app = App::with_peer(alone("me"), String::new(), None);
    app.listening.open("phone");
    app.session = Some(Listening {
        devices: vec![device("phone", "Phone", Kind::Phone, true)],
        scrub: Default::default(),
        session: Some(Active {
            queue: vec![track("a", 1), track("b", 2), track("c", 3), track("d", 4)],
            at: 1,
            clock: Clock::Paused { at_ms: 0 },
            output: Output::Device {
                id: "phone".into(),
                moving: false,
            },
            from: String::new(),
        }),
    });
    app
}

/// The panel draws the track playing, then what plays after it, in the order
/// it will, each with its place in the queue; what has played is not drawn.
///
/// Falsified by a `queue_rows` that started from the top of the queue: the
/// first row was `a`, which has played.
#[test]
fn the_queue_panel_draws_the_track_playing_then_up_next() {
    let app = app();
    let rows: Vec<(usize, String)> = app.queue_rows().into_iter().map(|(i, t)| (i, t.title)).collect();
    assert_eq!(rows, [(1, "b".to_string()), (2, "c".into()), (3, "d".into())]);
}

/// On a row of the panel `x` takes that track out, `J` and `K` move it down
/// and up among what plays after the track playing — never above it, never
/// past the end — `<Enter>` plays it, and `q` closes; and each edit is the
/// session's transition at the row's place in the queue.
///
/// Falsified by a `queue_key` that let `K` move the first of up next above
/// the track playing: `K` on `c` answered `QueueMove(2, 1)`.
#[test]
fn the_keys_on_a_queue_row_take_it_out_move_it_or_play_it() {
    let mut app = app();
    app.queue_panel = Some(1);
    assert!(matches!(app.queue_key('x'), Some(Message::QueueRemove(2))));
    assert!(matches!(app.queue_key('J'), Some(Message::QueueMove(2, 3))));
    assert!(app.queue_key('K').is_none(), "not above the track playing");
    app.queue_panel = Some(2);
    assert!(matches!(app.queue_key('K'), Some(Message::QueueMove(3, 2))));
    assert!(app.queue_key('J').is_none(), "not past the end");
    app.queue_panel = Some(0);
    assert!(
        app.queue_key('J').is_none() && app.queue_key('K').is_none(),
        "the track playing stays where it is"
    );
    assert!(matches!(app.queue_key('x'), Some(Message::QueueRemove(1))), "but can be taken out");
    assert!(matches!(app.queue_key('q'), Some(Message::CloseQueue)));
    use harken_player::Press;
    assert_eq!(App::queue_call(&Message::QueueRemove(2)), Some(Press::Remove { index: 2 }));
    assert_eq!(App::queue_call(&Message::QueueMove(3, 2)), Some(Press::Move { from: 3, to: 2 }));
    assert_eq!(App::queue_call(&Message::QueueJump(3)), Some(Press::Jump { index: 3 }));
    assert_eq!(App::queue_call(&Message::CloseQueue), None);
}

/// The row menu offers "Play next" and "Add to queue", and a window alone
/// does both to its own queue: next after the track playing, the other at
/// the end. `q` opens the queue and takes the keyboard — the cursor walks it
/// with `j`, `<Esc>` closes it.
///
/// Falsified by a row menu without the two entries ("Play next" missing),
/// and by `enqueue` putting "next" at the end alone: `[a, b, c, a, c]`.
#[test]
fn q_opens_the_queue_and_the_row_menu_puts_a_track_next_or_last() {
    // A window alone, with a library of three.
    let mut p = alone("me");
    for t in ["a", "b", "c"] {
        p.client
            .lock()
            .mutate("add_song", &song(t, "Bach", "Suites", &format!("music/{t}.mp3")))
            .unwrap();
    }
    p.refresh();
    let mut app = App::with_peer(p, String::new(), None);
    let ids: Vec<_> = app.peer.rows().iter().map(|i| (i.title.clone(), i.id)).collect();
    let id = |t: &str| ids.iter().find(|(n, _)| n == t).unwrap().1;
    let (_, entries) = app.row_entries(id("a")).unwrap();
    let labels: Vec<String> = entries.iter().map(|e| e.label.clone()).collect();
    assert!(
        labels.contains(&"Play next".to_string()) && labels.contains(&"Add to queue".to_string()),
        "{labels:?}"
    );
    let _ = app.update(Message::PlayItem(id("a")));
    let _ = app.update(Message::AddToQueue(id("a")));
    let _ = app.update(Message::PlayNext(id("c")));
    let titles: Vec<&str> = app.queue.iter().map(|t| t.title.as_str()).collect();
    let mut want: Vec<&str> = ids.iter().map(|(t, _)| t.as_str()).collect();
    let at = want.iter().position(|t| *t == "a").unwrap();
    want.insert(at + 1, "c");
    want.push("a");
    assert_eq!(titles, want);

    let _ = app.act(vim::Action::Key('q'));
    assert_eq!((app.queue_panel, app.focus()), (Some(0), Focus::Queue));
    assert_eq!(app.queue_rows()[0].1.title, "a", "the track playing first");
    let _ = app.act(vim::Action::Move(vim::Motion::Down(1)));
    assert_eq!(app.queue_panel, Some(1));
    let _ = app.act(vim::Action::Cancel);
    assert_eq!(app.queue_panel, None);
}

/// A row of what plays next dragged down past the next one lands after it:
/// the list is drawn in that order at once, ahead of the session saying so,
/// and the cursor goes with the row. A press let go where it was is a
/// click, which plays the row and moves nothing.
///
/// Falsified by a drop that waited for the session: up next was still drawn
/// `[c, d]`, so the row jumped back before it moved. (`step`, not `update`:
/// the session here is set by hand, and `update` reconciles it with the
/// room, which has none.)
#[test]
fn a_row_dragged_past_the_next_lands_after_it() {
    let mut app = app();
    let _ = app.step(Message::OpenQueue);
    let titles = |app: &App| app.upcoming_drawn().into_iter().map(|(_, t)| t.title).collect::<Vec<_>>();
    assert_eq!(titles(&app), ["c", "d"]);

    app.cursor = iced::Point::new(500.0, 100.0);
    let _ = app.step(Message::QueueGrab(0));
    let _ = app.step(Message::QueueDrag(iced::Point::new(500.0, 100.0 + 1.6 * arkui::panel::ENTRY)));
    assert!(app.queue_drag.as_ref().is_some_and(|d| d.dragging() && d.to() == 1));
    let _ = app.step(Message::QueueDrop);
    assert!(app.queue_drag.is_none());
    assert_eq!(titles(&app), ["d", "c"], "drawn moved before the session answers");
    assert_eq!(app.queue_panel, Some(2), "the cursor on the row moved");

    // A press that hardly moves is a click: nothing settles.
    app.queue_settle = None;
    app.cursor = iced::Point::new(500.0, 100.0);
    let _ = app.step(Message::QueueGrab(1));
    let _ = app.step(Message::QueueDrag(iced::Point::new(500.0, 101.0)));
    let _ = app.step(Message::QueueDrop);
    assert!(app.queue_drag.is_none() && app.queue_settle.is_none());
}

/// Closed, the queue lets go of the keyboard at once but its sheet stays
/// drawn, sliding out; opened again on its way out, it slides back in from
/// where it got to.
///
/// Falsified by a close that dropped the sheet: there was nothing left to
/// slide out.
#[test]
fn the_queue_sheet_slides_out_when_closed() {
    let mut app = app();
    let _ = app.step(Message::OpenQueue);
    assert!(app.queue_sheet.is_some_and(|s| s.is_shown()));
    let _ = app.step(Message::CloseQueue);
    assert_eq!(app.queue_panel, None);
    assert_ne!(app.focus(), Focus::Queue);
    assert!(app.queue_sheet.is_some_and(|s| !s.is_shown()), "still drawn, on its way out");
    let _ = app.step(Message::OpenQueue);
    assert!(app.queue_sheet.is_some_and(|s| s.is_shown()));
}
