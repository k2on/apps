//! What is playing, said three times: the bar's track, the row the list
//! lights, and the tab's title — one state, after every press of `}`,
//! alone, joined and playing here, and joined while another device plays.
//! The joined windows dial a harken hub in this process
//! (`harken_player::config::hub`), as the phones' hall does.
use std::sync::Arc;

use ark::peer::Who;
use arkui::vim;

use super::{song, who};
use crate::client::Replica;
use crate::{App, Message};

/// The bar's track, the lit row and the title, as `(title, title, title)`:
/// the bar's and the row's named by the library, the tab's as it reads.
fn said(app: &App) -> (Option<String>, Option<String>, String) {
    let name = |id: crate::rows::Id| app.peer.rows().iter().find(|i| i.id == id).map(|i| i.title.clone());
    let bar = app.now().and_then(|n| name(n.track().id.0));
    let lit = app.highlighted().and_then(|(id, _)| name(id));
    (bar, lit, app.title())
}

/// All three on `t`.
fn on(app: &App, t: &str) {
    let want = Some(t.to_string());
    assert_eq!(said(app), (want.clone(), want, format!("{t} \u{2014} Bach")));
}

fn library(p: &mut Replica) {
    for t in ["a", "b", "c"] {
        p.mutate("add_song", &song(t, "Bach", "Suites", &format!("music/{t}.mp3"))).unwrap();
    }
}

fn play_first(app: &mut App) {
    let id = app.peer.rows().iter().find(|i| i.title == "a").unwrap().id;
    let _ = app.update(Message::PlayItem(id));
}

/// A harken hub in this process.
fn hub() -> ark_peer::Handle {
    let c = harken_player::config::hub(crate::client::domain(), None);
    ark_peer::Handle::spawn(ark_peer::Peer::serve(c, Box::new(ark_peer::Memory::default()), ark_peer::now_ms()))
}

/// The hub's in-process transport and the tokens it knows: `dev` is this
/// window (`me-device`, holding the library role), `phone` the phone.
fn dial(hub: &ark_peer::Handle) -> impl ark_peer::link::Dial {
    hub.dialer(Arc::new(|t: &str| match t {
        "dev" => Some(Who {
            roles: vec![crate::domain::LIBRARY.to_string()],
            ..who("me")
        }),
        "phone" => Some(phone_who()),
        _ => None,
    }))
}

/// The phone: the same account, its own login.
fn phone_who() -> Who {
    Who {
        actor: "me".into(),
        session: "phone".into(),
        roles: vec![],
    }
}

/// Everything said answered: the hub's turn, then the window's update and
/// the phone's pump, until both have been quiet twice.
fn carry(hub: &ark_peer::Handle, app: &mut App, phone: &mut Option<Replica>) {
    for _ in 0..64 {
        let _ = hub.with(|_| ());
        let _ = app.update(Message::Wake);
        if let Some(p) = phone {
            p.pump();
        }
    }
}

/// A window joined to the hub, saying it `can_play`.
fn joined(can_play: bool, hub: &ark_peer::Handle) -> App {
    let me = who("me");
    let mut client = crate::client::memory_as(me, Some("dev"), crate::client::device(can_play, "Browser"));
    client.set_roles([crate::domain::LIBRARY]);
    library(&mut client);
    client.connect_with("hub", dial(hub));
    let mut app = App::with_peer(crate::peer::Peer::open(client), String::new(), None);
    app.listening.open("me-device");
    app
}

/// Alone: the window's own player and queue.
///
/// Falsified by a list that lit the row before `App::now`'s track: after
/// `}` the bar and the title said `b` and the row `a` (as in both joined
/// cases).
#[test]
fn alone_the_bar_the_row_and_the_title_are_one_track() {
    let mut p = super::alone("me");
    library(&mut p.client);
    p.refresh();
    let mut app = App::with_peer(p, String::new(), None);
    play_first(&mut app);
    on(&app, "a");
    let _ = app.act(vim::Action::Key('}'));
    on(&app, "b");
}

/// Joined, this window the output: the session moves, and so does all of
/// it at once — before the hub has answered, and after.
///
/// Falsified as the case alone is.
#[test]
fn joined_and_playing_here_the_bar_the_row_and_the_title_are_one_track() {
    let hub = hub();
    let mut app = joined(true, &hub);
    carry(&hub, &mut app, &mut None);
    assert!(app.listening.joined(&app.peer.client), "joined");
    play_first(&mut app);
    carry(&hub, &mut app, &mut None);
    let s = app.listening.session(&app.peer.client).unwrap();
    assert!(app.listening.outputs_here(&s), "this window plays");
    assert_eq!(app.player.track().map(|t| t.title.as_str()), Some("a"), "and its player has it");
    on(&app, "a");
    let _ = app.act(vim::Action::Key('}'));
    on(&app, "b");
    carry(&hub, &mut app, &mut None);
    on(&app, "b");
}

/// Joined while the phone plays: this window is a remote, and what it
/// draws, lights and titles is the session's track, not its silent
/// player's.
///
/// The bug this file is for. Failed before the fix, when the list and the
/// title read the window's `Player`, which never loaded anything: the bar
/// said `a`, no row was lit and the title was `harken`. Falsified again by
/// a title that read the player: `harken`.
#[test]
fn joined_while_another_device_plays_the_bar_the_row_and_the_title_are_one_track() {
    let hub = hub();
    let mut app = joined(false, &hub);
    let mut phone = crate::client::memory_as(phone_who(), Some("phone"), crate::client::device(true, "Phone"));
    phone.connect_with("hub", dial(&hub));
    let mut phone = Some(phone);
    carry(&hub, &mut app, &mut phone);
    assert!(app.listening.joined(&app.peer.client), "joined");
    // The phone presses the window's queue: the session's, its output the
    // phone.
    let queue: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|t| {
            let i = app.peer.rows().iter().find(|i| i.title == *t).unwrap().clone();
            crate::listening::Track {
                id: crate::peer::key(i.id),
                title: i.title.clone(),
                creator: i.creator.clone(),
                album: String::new(),
                duration_ms: i.duration_ms,
                file: i.file.clone(),
            }
        })
        .collect();
    let mut player = harken_player::Player::new("phone", None);
    let press = harken_player::Press::Start {
        queue,
        at: 0,
        position_ms: 0,
        from: String::new(),
    };
    assert_eq!(player.press(phone.as_mut().unwrap(), &press, None), None);
    carry(&hub, &mut app, &mut phone);
    let s = app.listening.session(&app.peer.client).unwrap();
    assert!(!app.listening.outputs_here(&s) && app.listening.elsewhere(&s), "the phone plays");
    on(&app, "a");
    let _ = app.act(vim::Action::Key('}'));
    on(&app, "b");
    carry(&hub, &mut app, &mut phone);
    on(&app, "b");
}
