//! §1.9 held at the machine: a peer in the volatile space is the same
//! machine over the volatile tables, sharing a connection with the durable
//! one — its frames name their space, its store hashes its own tables, it
//! makes nothing durable, a change to the other space is refused, and a
//! volatile authority that restarted takes a session's pushes past the gap
//! its restart made and answers a `Hello` it lacks the hash for with a
//! snapshot (`resync`).

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use std::sync::Arc;

use ark::frames::{Frame, Verdict};
use ark::peer::{Functions, Input, Journal, Output, Peer, Role};
use ark::prelude::*;
use ark::schema::{Schema, Space};
use ark::store::Read;
use support::wire::*;

/// Who is here, by session: volatile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(volatile)]
pub struct Here {
    #[ark(key)]
    pub session: String,
    pub name: String,
}

/// The demo's tables and `here`.
#[derive(Schema)]
pub struct Waves {
    pub playlist: demo::Playlist,
    pub item: demo::Item,
    pub here: Here,
}

fn schema() -> Arc<Schema> {
    Arc::new(<Waves as Tables>::schema())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Wave {
    pub name: String,
}

/// `wave(name)`: the caller's row of `here`, put.
#[live("waves.wave")]
pub fn wave(ctx: &Ctx, db: &mut Db<Waves, Volatile>, input: Wave) -> Result<(), Refusal> {
    db.here.delete(ctx.device())?;
    db.here.insert(Here {
        session: ctx.device().into(),
        name: input.name,
    })
}

fn here_key(session: &str) -> Vec<u8> {
    let mut k = vec![];
    ark::format::put_key(&(session,), &mut k);
    k
}

fn cas() -> Functions {
    let mut c = demo::only(&[demo::CREATE]);
    let waves = ark::register!(Domain::new::<Waves>("waves", "1"); wave).functions();
    c.insert("waves.wave", waves.get("waves.wave").unwrap().clone());
    c
}

fn peer(role: Role, name: &str, space: Space) -> Peer {
    Peer::new(schema(), cas(), role, who(name)).in_space(space)
}

fn authority() -> Peer {
    peer(Role::Authority { journal: Journal::default() }, "server", Space::Volatile)
}

fn waving(name: &str) -> Input {
    Input::Mutate {
        func: "waves.wave".into(),
        input: Wave { name: name.into() }.to_bytes(),
        now: 1,
        cause: None,
    }
}

/// A wave goes up and comes back in the volatile space only: every frame
/// names it, nothing is made durable, the durable machine on the same
/// connection takes none of it, the volatile hash moves and the durable
/// does not. Falsified by `frame` not filtering `Ack` by space: the
/// durable replica settled its own pending intent of the same id on the
/// volatile authority's `Ack` (its head past the `Ack`'s sequence).
#[test]
fn a_volatile_intent_is_its_own_space_end_to_end() {
    let (mut server, mut live, mut log) = (
        authority(),
        peer(Role::Replica, "alice", Space::Volatile),
        peer(Role::Replica, "alice", Space::Durable),
    );
    let mut durable = peer(Role::Authority { journal: Journal::default() }, "server", Space::Durable);
    for n in ["a", "b", "c"] {
        let o = durable.step(mutate(demo::CREATE, demo::create_input(n), 1));
        log.step(down(&fanouts(&o)[0]));
    }
    let o = log.step(mutate(demo::CREATE, demo::create_input("Mix"), 1));
    assert!(sends(&o).iter().all(|f| matches!(f, Frame::Page { space: Space::Durable, .. })));
    let o = live.step(waving("phone"));
    assert!(
        matches!(&sends(&o)[..], [Frame::Page { space: Space::Volatile, logged, .. }] if logged[0].entry.id == log.pending[0].entry.id),
        "one id, two spaces"
    );
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    assert!(matches!(
        &s[..],
        [Frame::Verdict {
            space: Space::Volatile,
            verdict: Verdict::Ok { seq: 1 },
            ..
        }]
    ));
    let durable_hash = log.store.state_hash();
    for frame in s.iter().chain(&f) {
        assert!(log.step(down(frame)).is_empty(), "the durable machine takes nothing of {frame:?}");
        let out = live.step(down(frame));
        assert!(!out.iter().any(|x| matches!(x, Output::Persist(_))), "nothing volatile is made durable");
    }
    assert_eq!((log.pending.len(), live.pending.len()), (1, 0));
    assert_eq!(log.store.state_hash(), durable_hash);
    assert_ne!(
        live.store.state_hash(),
        authority().store.state_hash(),
        "the wave is in the volatile hash"
    );
    assert_eq!(live.store, server.store);
    assert!(live.store.get("here", &here_key("alice-session")).is_some());
}

/// A volatile intent that writes a durable table is refused, and so is a
/// durable one writing a volatile table. Falsified by `attempt` judging no
/// space: the playlist was created in the volatile store.
#[test]
fn a_change_to_the_other_space_is_refused() {
    let mut live = peer(Role::Replica, "alice", Space::Volatile);
    let o = live.step(mutate(demo::CREATE, demo::create_input("Mix"), 1));
    assert!(
        matches!(&o[..], [Output::Refused { why, .. }] if why == "playlist is not in this space"),
        "{o:?}"
    );
    let mut log = peer(Role::Replica, "alice", Space::Durable);
    let o = log.step(waving("phone"));
    assert!(
        matches!(&o[..], [Output::Refused { why, .. }] if why == "here is not in this space"),
        "{o:?}"
    );
}

/// An authority restarted from nothing: the replica's `Hello` names a hash
/// it lacks and is answered with its snapshot, and the replica's next
/// intent — its second, the first lost with the old authority — is taken.
/// Falsified by keeping the durable gap rule in the volatile space: the
/// second wave waited for a first that would never come.
#[test]
fn a_restarted_authority_resyncs_and_takes_the_gap() {
    let (mut server, mut live) = (authority(), peer(Role::Replica, "alice", Space::Volatile));
    let o = live.step(waving("phone"));
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    for frame in s.iter().chain(&f) {
        live.step(down(frame));
    }
    let mut fresh = authority();
    let o = live.step(Input::Connected);
    let (s, _) = deliver_up(&mut fresh, &who("alice"), &o);
    assert!(matches!(&s[..], [Frame::Snapshot { space: Space::Volatile, .. }]), "{s:?}");
    live.step(down(&s[0]));
    assert!(live.store.get("here", &here_key("alice-session")).is_none(), "the restart lost the wave");
    let o = live.step(waving("laptop"));
    let (s, f) = deliver_up(&mut fresh, &who("alice"), &o);
    assert!(matches!(&verdicts(&s)[..], [Some(Verdict::Ok { seq: 1 })]), "{s:?}");
    live.step(down(&f[0]));
    assert!(live.pending.is_empty() && live.store == fresh.store);
}

/// A volatile push that comes after a later one of its session was taken
/// is refused as overtaken, and its replica lets it go. Falsified by
/// `push` without the rule: nothing answered it and it stayed pending.
#[test]
fn a_volatile_push_overtaken_is_refused() {
    let (mut server, mut live) = (authority(), peer(Role::Replica, "alice", Space::Volatile));
    let first = live.step(waving("phone"));
    let second = live.step(waving("laptop"));
    for o in [&second, &first] {
        let (s, f) = deliver_up(&mut server, &who("alice"), o);
        for frame in s.iter().chain(&f) {
            live.step(down(frame));
        }
    }
    assert!(live.pending.is_empty(), "both answered");
    assert_eq!(server.log().head().0, 1, "only the later one taken");
}

/// A replica moves its own horizon (`Compact`, at most its head): the
/// entries below it go, a `Hello` from there is answered with no snapshot,
/// and an intent pending across the fold is rebased under another's entry
/// and confirmed as before. Falsified by `compact` taking an authority's
/// only, as it did: the replica held all five entries.
#[test]
fn a_replica_folds_its_own_log() {
    let (mut server, mut alice, mut bob) = (
        authority(),
        peer(Role::Replica, "alice", Space::Volatile),
        peer(Role::Replica, "bob", Space::Volatile),
    );
    let carry = |server: &mut Peer, w: &str, o: &[Output], to: &mut [&mut Peer]| {
        let (s, f) = deliver_up(server, &who(w), o);
        for frame in s.iter().chain(&f) {
            for p in to.iter_mut() {
                p.step(down(frame));
            }
        }
    };
    for n in 0..5 {
        let o = alice.step(waving(&format!("phone {n}")));
        carry(&mut server, "alice", &o, &mut [&mut alice]);
    }
    let pending = alice.step(waving("laptop"));
    alice.step(Input::Compact(i64::MAX));
    assert_eq!((alice.log().entries().len(), alice.log().base_seq()), (0, 5), "it stands on its head");
    let o = bob.step(waving("tablet"));
    let (_, f) = deliver_up(&mut server, &who("bob"), &o);
    alice.step(down(&f[0]));
    assert_eq!(alice.log().head(), server.log().head());
    assert_eq!(alice.pending.len(), 1, "still pending, rebased");
    carry(&mut server, "alice", &pending, &mut [&mut alice]);
    assert!(alice.pending.is_empty() && alice.store == server.store);
    let hello = sends(&alice.step(Input::Connected));
    let (s, _) = deliver_up(&mut server, &who("alice"), &[Output::Send(hello[0].enc())]);
    assert!(s.is_empty(), "nothing to send from its head: {s:?}");
}

/// A durable replica's fold makes its rows durable as confirmed: its
/// pending intent's changes are not in them. Falsified by persisting the
/// store's rows as they stand: the pending playlist was in them.
#[test]
fn a_replica_folds_its_confirmed_rows() {
    let mut durable = peer(Role::Authority { journal: Journal::default() }, "server", Space::Durable);
    let mut log = peer(Role::Replica, "alice", Space::Durable);
    let o = durable.step(mutate(demo::CREATE, demo::create_input("a"), 1));
    log.step(down(&fanouts(&o)[0]));
    log.step(mutate(demo::CREATE, demo::create_input("Mix"), 1));
    let o = log.step(Input::Compact(i64::MAX));
    let rows = o.iter().find_map(|x| match x {
        Output::Persist(b) => match ark::log::Record::dec(b) {
            Ok(ark::log::Record::Rows(r)) => Some(r),
            _ => None,
        },
        _ => None,
    });
    let rows = rows.expect("the rows made durable whole");
    let playlists = rows.iter().find(|(t, _)| t == "playlist").map_or(0, |(_, rs)| rs.len());
    assert_eq!(playlists, 1, "the confirmed playlist only");
    assert_eq!(log.pending.len(), 1);
}

/// A volatile authority keeps each origin's last 64 verdicts: a push again
/// inside them is answered as it was, one below them refused as
/// overtaken, and no more are kept however many come. Falsified by
/// keeping every verdict, as before: it held 70.
#[test]
fn volatile_verdicts_are_a_window() {
    let (mut server, mut live) = (authority(), peer(Role::Replica, "alice", Space::Volatile));
    let mut pushes = vec![];
    for n in 0..70 {
        let o = live.step(waving(&format!("phone {n}")));
        let (s, f) = deliver_up(&mut server, &who("alice"), &o);
        for frame in s.iter().chain(&f) {
            live.step(down(frame));
        }
        pushes.push(o);
    }
    let Role::Authority { journal } = &server.role else { unreachable!() };
    assert_eq!(journal.verdicts.len(), 64);
    let (s, _) = deliver_up(&mut server, &who("alice"), &pushes[69]);
    assert!(matches!(&verdicts(&s)[..], [Some(Verdict::Ok { seq: 70 })]), "{s:?}");
    let (s, _) = deliver_up(&mut server, &who("alice"), &pushes[6]);
    assert!(matches!(&verdicts(&s)[..], [Some(Verdict::Ok { seq: 7 })]), "the 64th last: {s:?}");
    let (s, _) = deliver_up(&mut server, &who("alice"), &pushes[5]);
    assert!(
        matches!(&verdicts(&s)[..], [Some(Verdict::Refused { why })] if why.contains("overtaken")),
        "{s:?}"
    );
}
