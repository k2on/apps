//! Nobody has to sign in, and signing in later keeps everything; and every
//! change this window makes can say what became of it.
use ark::frames::{Frame, Verdict};
use ark::schema::Space;
use ark_auth::{Account, Login};

use super::{signed_in, signed_out, song};
use crate::client::Standing;
use crate::places::Pane;
use crate::{App, Message};

/// A login holding the library's role, as the scanner's account does: the
/// songs these windows made are theirs to make.
fn login(who: &str) -> Login {
    Login {
        token: format!("token-{who}"),
        session: format!("session-{who}"),
        user: Account {
            id: who.into(),
            roles: vec![crate::domain::LIBRARY.into()],
            ..Account::default()
        },
        expires_ms: 0,
    }
}

/// A window with nobody signed in works on its own replica — what is done is
/// kept, pending, as nobody's — and signing in moves *that* replica on: the
/// same rows, the same pending work, now the signer's, and nothing dropped.
///
/// Falsified by making `signed_in` open a fresh replica rather than call
/// `sign_in` on this one: the item is gone and the pending count is 0. An
/// intent's id is its replica's origin's (§1.6), so the pending one keeps
/// its id through the sign-in; falsified by `ark_client::Client::sign_in`
/// authoring it again from a new origin: the id moved.
#[test]
fn signing_in_moves_the_same_replica_on() {
    let mut p = signed_out();
    assert!(p.client.is_signed_out());
    let id = p
        .client
        .mutate("add_song", &song("Air", "Bach", "Suites", "music/air.mp3"))
        .expect("a window holding the library role takes a song signed out");
    p.refresh();
    let mut app = App::with_peer(p, "http://127.0.0.1:9".into(), None);
    assert_eq!(app.peer.items().len(), 1, "what was done signed out is on screen");
    assert_eq!(app.peer.client.standing(&id), Standing::Pending);
    let pending = |app: &App| app.peer.client.log.pending.iter().map(|p| p.entry.actor.clone()).collect::<Vec<_>>();
    assert_eq!(pending(&app), [""], "authored as nobody");

    let _ = app.update(Message::SignedIn(Ok(login("alice"))));
    assert!(!app.peer.client.is_signed_out());
    assert_eq!(app.login.as_ref().map(|l| l.user.id.as_str()), Some("alice"));
    assert_eq!(app.peer.items().len(), 1, "the same replica, not a new one");
    assert_eq!(app.peer.client.pending_len(), 1);
    assert_eq!(pending(&app), ["alice"], "…and the work is hers now");
    let now = app.peer.client.log.pending[0].entry.id;
    assert_eq!(now, id, "its id kept");
    assert_eq!(app.peer.client.standing(&id), Standing::Pending);
    assert!(app.note.contains("1 change from before"), "{}", app.note);

    // Signing out revokes the session through the domain, keeps the
    // replica and goes on as her, offline. Falsified by `SignOut` only
    // dropping the token: no `session.revoke` was authored.
    let _ = app.update(Message::SignOut);
    assert!(app.login.is_none());
    assert!(app.peer.client.is_signed_out());
    let funcs: Vec<String> = app.peer.client.log.pending.iter().map(|p| p.entry.func.clone()).collect();
    assert_eq!(funcs.last().map(String::as_str), Some("session.revoke"), "{funcs:?}");
    assert_eq!(app.peer.items().len(), 1);
}

/// A refusal is said about the change it refuses — "put Air on Favorites" —
/// not as "a change was refused"; and the debug screen's standing agrees.
///
/// Falsified by dropping the `edits` lookup in `pump`: the note says "a
/// change" and the first assertion fails.
#[test]
fn a_refusal_names_what_was_refused() {
    let mut p = signed_in("alice");
    p.client.mutate("add_song", &song("Air", "Bach", "Suites", "music/air.mp3")).unwrap();
    p.refresh();
    p.ensure_playlist();
    let mut app = App::with_peer(p, "http://127.0.0.1:9".into(), Some(login("alice")));
    app.pane = Pane::Tracks;

    let _ = app.update(Message::OpenPicker);
    let picker = app.ctx.picker.as_ref().expect("the picker opened on the row under the cursor");
    assert_eq!(picker.choices.first().map(|c| (c.name.as_str(), c.on)), Some(("Favorites", false)));
    let _ = app.update(Message::PickerActivate);
    let edit = app.edits.last().cloned().expect("the toggle was authored");
    assert_eq!(edit.what, "put Air on Favorites");
    assert_eq!(app.peer.client.standing(&edit.id), Standing::Pending);

    let reject = Frame::Verdict {
        space: Space::Durable,
        id: edit.id,
        verdict: Verdict::Refused {
            why: "not your playlist".into(),
        },
    };
    app.peer.client.receive(reject.enc());
    let _ = app.update(Message::Wake);
    assert_eq!(app.note, "the server refused to put Air on Favorites: not your playlist");
    assert_eq!(
        app.refused.last(),
        Some(&("put Air on Favorites".to_string(), "not your playlist".to_string()))
    );
    assert!(matches!(app.peer.client.standing(&edit.id), Standing::Rejected(why) if why == "not your playlist"));
    assert!(!app.peer.items()[0].on_playlist(), "the rebase took the optimistic row back");
}

/// Signed in, the default "Favorites" waits until the log has been heard —
/// a second of quiet after the link opened — because a device that has not
/// heard the log sees no playlists for want of news, and a second
/// "Favorites" would come back renamed. The second is a deadline now
/// (`docs/plan-reactive.md`): the link opening sets it a second out, 999 ms
/// is not enough, the thousandth makes it, and a person who already has one
/// gets none.
///
/// The numbers are numbers, not `QUIET_MS`: written against the constant
/// the window would agree with itself whatever it was. Falsified by setting
/// `QUIET_MS` to 1 (the deadline is not a second out) and to 1500 (likewise).
#[test]
fn the_default_playlist_waits_for_the_log() {
    use super::Queues;
    use crate::client::monotonic_ms;
    use ark_peer::Event;
    let mut app = App::with_peer(signed_in("alice"), "http://127.0.0.1:9".into(), Some(login("alice")));
    let made: std::sync::Arc<std::sync::Mutex<Vec<Queues>>> = Default::default();
    app.peer.client.connect_with("ws://queues", Queues::dialer(made.clone()));
    let _ = app.update(Message::Due);
    assert!(app.peer.playlists().is_empty(), "nothing is made before anything is heard");
    let opened = monotonic_ms();
    let q = made.lock().unwrap()[0].clone();
    q.push(Event::Open);
    let _ = app.update(Message::Wake);
    let at = app.quiet.expect("the open set the quiet deadline");
    assert!((opened + 1_000..opened + 1_100).contains(&at), "a second out: {} ms", at - opened);
    assert_eq!(app.deadline(), Some(at), "the window's one timer");
    app.due_at(at - 1);
    assert!(app.peer.playlists().is_empty(), "999 ms is not a second of quiet");
    app.due_at(at);
    let _ = app.update(Message::Due);
    assert_eq!(app.peer.playlists().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Favorites"]);
    assert_eq!(app.quiet, None, "and no timer left");

    // Somebody who has one — arrived with the log — gets no second.
    app.quiet = Some(at);
    app.due_at(at + 5_000);
    assert_eq!(app.peer.playlists().len(), 1);
}
