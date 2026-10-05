//! Nobody has to sign in, and signing in later keeps everything; and every
//! change this window makes can say what became of it.
use ark_auth::{Account, Login};
use ark_client::ark::protocol::ServerMsg;
use ark_client::{Options, Standing};

use super::{peer, song};
use crate::places::Pane;
use crate::{App, Message};

fn login(who: &str) -> Login {
    Login {
        token: format!("token-{who}"),
        session: format!("session-{who}"),
        user: Account {
            id: who.into(),
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
/// `sign_in` on this one: the item is gone and the pending count is 0.
#[test]
fn signing_in_moves_the_same_replica_on() {
    let mut p = peer(Options::signed_out());
    assert!(p.client.is_signed_out());
    let id = p
        .client
        .mutate("add_song", song("Air", "Bach", "Suites", "music/air.mp3"))
        .expect("a window holding the library role takes a song signed out");
    p.refresh();
    let mut app = App::with_peer(p, "http://127.0.0.1:9".into(), None);
    assert_eq!(app.peer.items.len(), 1, "what was done signed out is on screen");
    assert_eq!(app.peer.client.standing(&id), Standing::Pending);
    assert!(app.peer.client.replica().pending.iter().all(|e| e.actor.is_empty()), "authored as nobody");

    let _ = app.update(Message::SignedIn(Ok(login("alice"))));
    assert!(!app.peer.client.is_signed_out());
    assert_eq!(app.login.as_ref().map(|l| l.user.id.as_str()), Some("alice"));
    assert_eq!(app.peer.items.len(), 1, "the same replica, not a new one");
    assert_eq!(app.peer.client.pending_len(), 1);
    assert!(
        app.peer.client.replica().pending.iter().all(|e| e.actor == "alice"),
        "…and the work is hers now"
    );
    assert_eq!(app.peer.client.standing(&id), Standing::Pending, "the id `mutate` gave still names it");
    assert!(app.note.contains("1 change from before"), "{}", app.note);

    // Signing out keeps the replica and goes on as her, offline.
    let _ = app.update(Message::SignOut);
    assert!(app.login.is_none());
    assert!(app.peer.client.is_signed_out());
    assert_eq!(app.peer.items.len(), 1);
}

/// A refusal is said about the change it refuses — "put Air on Favorites" —
/// not as "a change was refused"; and the debug screen's standing agrees.
///
/// Falsified by dropping the `edits` lookup in `pump`: the note says "a
/// change" and the first assertion fails.
#[test]
fn a_refusal_names_what_was_refused() {
    let mut p = peer(Options::dev("alice"));
    p.client.mutate("add_song", song("Air", "Bach", "Suites", "music/air.mp3")).unwrap();
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

    app.peer.client.recv(ServerMsg::Reject {
        id: edit.id,
        reason: "not your playlist".into(),
    });
    let _ = app.update(Message::Tick);
    assert_eq!(app.note, "the server refused to put Air on Favorites: not your playlist");
    assert_eq!(
        app.refused.last(),
        Some(&("put Air on Favorites".to_string(), "not your playlist".to_string()))
    );
    assert!(matches!(app.peer.client.standing(&edit.id), Standing::Rejected(why) if why == "not your playlist"));
    assert!(!app.peer.items[0].on_playlist(), "the rebase took the optimistic row back");
}

/// Signed in, the default "Favorites" waits until the log has been heard —
/// a second of quiet after the link opened — because a device that has not
/// heard the log sees no playlists for want of news, and a second
/// "Favorites" would come back renamed. Nineteen ticks is not enough; the
/// twentieth makes it; and a person who already has one gets none.
///
/// The tick counts are numbers, not `QUIET_TICKS`: written against the
/// constant, setting it to 1 would empty the "not yet" loop. Falsified by
/// setting `QUIET_TICKS` to 1 (the first assertion fails) and to 30 (the
/// second does).
#[test]
fn the_default_playlist_waits_for_the_log() {
    let mut app = App::with_peer(peer(Options::dev("alice")), "http://127.0.0.1:9".into(), Some(login("alice")));
    assert!(app.peer.playlists().is_empty(), "nothing is made before anything is heard");
    app.quiet = Some(0);
    for _ in 0..19 {
        let _ = app.update(Message::Tick);
    }
    assert!(app.peer.playlists().is_empty(), "nineteen ticks is not a second of quiet");
    let _ = app.update(Message::Tick);
    assert_eq!(app.peer.playlists().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Favorites"]);

    // Somebody who has one — arrived with the log — gets no second.
    app.quiet = Some(0);
    for _ in 0..40 {
        let _ = app.update(Message::Tick);
    }
    assert_eq!(app.peer.playlists().len(), 1);
}
