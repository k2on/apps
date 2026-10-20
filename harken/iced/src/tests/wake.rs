//! The window runs no loop (`docs/plan-reactive.md`): these run the very
//! subscription iced would run — every recipe `App::subscription` returns,
//! streamed with no input events — and count what reaches `update`. At rest
//! that is nothing at all; a frame on the socket is a wake; a fading
//! highlight and a submenu's dwell run a clock, and stop it when they stop.

use std::time::{Duration, Instant};

use harken_domain5::playlists::CreatePlaylist;
use iced::advanced::subscription::{into_recipes, EventStream};
use iced::futures::executor::block_on;
use iced::futures::future::{self, Either};
use iced::futures::stream::{self, StreamExt};

use super::Queues;
use super::{alone, signed_in};
use crate::client::{monotonic_ms, Event};
use crate::{App, Message};

/// Every message the window's subscriptions produce in `d` of real time.
fn heard_within(app: &App, d: Duration) -> Vec<Message> {
    let streams: Vec<_> = into_recipes(app.subscription())
        .into_iter()
        .map(|r| {
            let quiet: EventStream = stream::pending().boxed();
            r.stream(quiet)
        })
        .collect();
    let mut all = stream::select_all(streams);
    let (tx, rx) = iced::futures::channel::oneshot::channel::<()>();
    std::thread::spawn(move || {
        std::thread::sleep(d);
        let _ = tx.send(());
    });
    block_on(async move {
        let mut out = vec![];
        let mut stop = rx;
        loop {
            match future::select(all.next(), &mut stop).await {
                Either::Left((Some(m), _)) => out.push(m),
                Either::Left((None, _)) | Either::Right(_) => return out,
            }
        }
    })
}

fn names(ms: &[Message]) -> Vec<&'static str> {
    ms.iter()
        .map(|m| match m {
            Message::Wake => "wake",
            Message::Due => "due",
            Message::Tick => "tick",
            _ => "other",
        })
        .collect()
}

/// A window over a library, at rest: no clock, no deadline, and a second and
/// a half of its real subscriptions brings `update` nothing. Falsified by
/// putting the 50 ms clock back unconditionally (`clock` answering
/// `Some(50ms)`): thirty ticks.
#[test]
fn a_window_at_rest_is_never_updated() {
    let mut app = App::with_peer(alone("me"), String::new(), None);
    app.peer.ensure_playlist();
    let _ = app.update(Message::Wake);
    assert_eq!((app.clock(), app.deadline()), (None, None), "nothing moves by itself");
    let heard = heard_within(&app, Duration::from_millis(1_500));
    assert!(heard.is_empty(), "at rest: {:?}", names(&heard));
}

/// A frame on the socket is a wake, through the replica's waker to the
/// window's bell to the subscription; the window answering it pumps once,
/// and is then at rest again with no timer. A close arms the redial as the
/// one timer, and it fires. Falsified by not handing the replica the bell
/// (`set_waker` out of `with_peer`): the open reaches nobody.
#[test]
fn a_frame_on_the_socket_is_one_wake_and_a_close_one_timer() {
    let mut app = App::with_peer(signed_in("alice"), "http://127.0.0.1:9".into(), None);
    let made: std::sync::Arc<std::sync::Mutex<Vec<Queues>>> = Default::default();
    app.peer.client.connect_with("ws://queues", Queues::dialer(made.clone()));
    assert!(app.deadline().is_some(), "a new link is due at once");
    let _ = app.update(Message::Due);
    assert_eq!(app.deadline(), None, "dialled: nothing to time");
    let q = made.lock().unwrap()[0].clone();
    let pushed = {
        let q = q.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            q.push(Event::Open);
        })
    };
    let heard = heard_within(&app, Duration::from_millis(600));
    pushed.join().unwrap();
    assert_eq!(names(&heard), ["wake"], "the open, once");
    let _ = app.update(Message::Wake);
    assert!(app.peer.client.linked());
    // Linked, the default playlist waits a second of quiet: the one timer.
    assert!(app.quiet.is_some() && app.deadline() == app.quiet);
    app.quiet = None;
    assert_eq!((app.clock(), app.deadline()), (None, None));
    assert!(heard_within(&app, Duration::from_millis(500)).is_empty(), "linked and idle");

    q.push(Event::Closed("gone".into()));
    let heard = heard_within(&app, Duration::from_millis(200));
    assert_eq!(names(&heard), ["wake"], "the close, once");
    let _ = app.update(Message::Wake);
    let at = app.deadline().expect("the close armed the redial");
    let heard = heard_within(&app, Duration::from_millis(1_000));
    assert_eq!(names(&heard), ["due"], "the redial, once, at {at}");
    let _ = app.update(Message::Due);
    assert_eq!(made.lock().unwrap().len(), 2, "dialled again at the deadline");
}

/// A highlight in the explorer runs the 50 ms clock while it fades and
/// stops it when it has: lit, the window ticks; two seconds on, it does not.
/// Falsified by `clock` ignoring `fading()`: no tick while the row is lit.
#[test]
fn a_fading_highlight_runs_the_clock_until_it_has_faded() {
    let mut app = App::with_peer(alone("alice"), String::new(), None);
    app.peer.ensure_playlist();
    let _ = app.update(Message::Wake);
    let _ = app.update(Message::Key(
        iced::keyboard::Key::Character("E".into()),
        iced::keyboard::Modifiers::empty(),
    ));
    let _ = app.update(Message::Explore(ark_explorer::Msg::Open("playlist".into())));
    assert_eq!(app.clock(), None, "open, and nothing lit");
    app.peer
        .client
        .mutate("create_playlist", &CreatePlaylist { name: "Lit".into() })
        .expect("made");
    let _ = app.update(Message::Wake);
    assert!(app.explore.as_ref().unwrap().ui.fading(), "the new row is lit");
    assert_eq!(app.clock(), Some(Duration::from_millis(50)));
    let t = Instant::now();
    let heard = heard_within(&app, Duration::from_millis(300));
    assert!(
        names(&heard).iter().filter(|n| **n == "tick").count() >= 3,
        "{:?} in {:?}",
        names(&heard),
        t.elapsed()
    );
    app.observe_explorer_at(monotonic_ms() + 2_100);
    assert_eq!(app.clock(), None, "faded: the clock stops");
}

/// A submenu's dwell runs the clock while it can still open the submenu,
/// and not after. Falsified by `clock` ignoring `dwelling()`: resting on
/// the entry never opens it in a window that runs no clock.
#[cfg(feature = "demo")]
#[test]
fn a_dwell_runs_the_clock_until_the_submenu_opens() {
    let mut app = super::demo_app();
    let id = app.peer.rows()[2].id;
    let _ = app.update(Message::RowMenu(id, crate::Anchor::Pointer));
    let _ = app.update(Message::MenuAt(1));
    assert!(app.ctx.dwelling());
    assert_eq!(app.clock(), Some(Duration::from_millis(50)));
    let heard = heard_within(&app, Duration::from_millis(300));
    let ticks = names(&heard).iter().filter(|n| **n == "tick").count();
    assert!(ticks >= 4, "{ticks} ticks");
    for _ in 0..ticks {
        let _ = app.update(Message::Tick);
    }
    assert!(app.ctx.picker.is_some(), "the rest opened it");
    assert_eq!(app.clock(), None, "and the clock stopped");
}
