//! §1.6 Ids from a replica's origin: an intent's id is `(origin, local)`'s
//! whoever is signed in, so what a device made signed out keeps its id and
//! the rows it drew theirs; the authority stamps the actor, session and
//! roles of the connection it judged the intent on, and binds an origin to
//! the actor whose intents first came from it.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use ark::frames::{Frame, Verdict};
use ark::log::{EntryId, Origin};
use ark::peer::{Peer, Role, Who};
use demo::{create_input, pair_input, ADD, CREATE};
use support::wire::*;

/// A device signed out (nobody, an empty session) makes a playlist and puts
/// a track on it; then alice signs in on that connection. The authority
/// takes both under the ids the device gave them — the add names the
/// playlist by the row id its create drew — stamped as alice's; a push
/// again is acknowledged where it was; bob pushing from alice's origin is
/// refused. Falsified by the authority deriving ids from the session again
/// (`Entry::id_checks` over `(session, local)`): both were refused as ids
/// not their own; and by not binding an origin to an actor: bob's push was
/// sequenced at 3.
#[test]
fn ids_are_the_origins_whoever_signs_in() {
    let nobody = Who::default();
    let origin = Origin::named(b"a device's sixteen bytes");
    let mut device = Peer::new(demo::shared(), demo::cas(), Role::Replica, nobody);
    device.origin = origin;
    let pid = EntryId::of(origin, 1).draw(0);
    let mut pushes = sends(&device.step(mutate(CREATE, create_input("Road"), 1)));
    pushes.extend(sends(&device.step(mutate(ADD, pair_input(&pid, "t1"), 2))));
    let mut server = authority(demo::cas());
    let mut back = vec![];
    for f in &pushes {
        back.extend(sends(&server.step(up(&who("alice"), f))));
    }
    assert!(
        matches!(verdicts(&back)[..], [Some(Verdict::Ok { seq: 1 }), Some(Verdict::Ok { seq: 2 })]),
        "{back:?}"
    );
    let e = &server.log.entries[0].entry;
    assert_eq!((e.id, e.origin, e.local), (EntryId::of(origin, 1), origin, 1));
    assert_eq!(
        (e.actor.as_str(), e.session.as_str()),
        ("alice", "alice-session"),
        "stamped as the connection"
    );
    assert_eq!(
        demo::pos_of(&server.store, &pid, "t1"),
        Some(1),
        "the add named the playlist by its drawn id"
    );
    let again = sends(&server.step(up(&who("alice"), &pushes[1])));
    assert!(matches!(verdicts(&again)[..], [Some(Verdict::Ok { seq: 2 })]), "{again:?}");
    let mut third = pushes[1].clone();
    if let Frame::Page { logged, .. } = &mut third {
        logged[0].entry.local = 3;
        logged[0].entry.id = EntryId::of(origin, 3);
    }
    let bob = sends(&server.step(up(&who("bob"), &third)));
    assert!(
        matches!(&verdicts(&bob)[..], [Some(Verdict::Refused { why })] if why.contains("another actor")),
        "{bob:?}"
    );
    assert_eq!(server.log.head().0, 2);
}
