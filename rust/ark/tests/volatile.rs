//! §1.9 held at the machine: a peer in the volatile space is the same
//! machine over the volatile tables, sharing a connection with the durable
//! one — its frames name their space, its store hashes its own tables, it
//! makes nothing durable, a change to the other space is refused, and a
//! volatile authority that restarted runs the log's rules: it answers a
//! `Hello` it lacks the hash for with a fill, and passes the gap its
//! restart made as a session's pushes say they begin past it.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use std::sync::Arc;

use ark::frames::{Frame, Verdict};
use ark::peer::{Input, Journal, Output, Peer, Role, To};
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
#[mutator("waves.wave")]
pub fn wave(ctx: &Ctx, db: &mut Db<Waves, Volatile>, input: Wave) -> Result<(), String> {
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

fn cas() -> Domain {
    let mut c = Domain {
        schema: schema(),
        ..Arc::unwrap_or_clone(demo::only(&[demo::CREATE]))
    };
    let waves = ark::register!(Domain::new::<Waves>("waves", "1"); wave);
    c.fns.insert("waves.wave".into(), waves.fns["waves.wave"].clone());
    c
}

fn peer(role: Role, name: &str, space: Space) -> Peer {
    Peer::new(Arc::new(cas()), role, who(name)).in_space(space)
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
            verdict: Verdict::At { seq: 1, .. },
            ..
        }]
    ));
    let durable_hash = log.store.state_hash();
    for frame in s.iter().chain(&f) {
        assert!(log.step(down(frame)).is_empty(), "the durable machine takes nothing of {frame:?}");
        let out = live.step(down(frame));
        assert!(
            !out.iter().any(|x| matches!(x, Output::Send(ark::peer::To::Disk, _))),
            "nothing volatile is made durable"
        );
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
        matches!(&o[..], [Output::Settled(_, Err(why))] if why == "playlist is not in this space"),
        "{o:?}"
    );
    let mut log = peer(Role::Replica, "alice", Space::Durable);
    let o = log.step(waving("phone"));
    assert!(
        matches!(&o[..], [Output::Settled(_, Err(why))] if why == "here is not in this space"),
        "{o:?}"
    );
}

/// An authority restarted from nothing runs the log's rules: the
/// replica's `Hello` names a hash it lacks and is answered with a fill of
/// its store, and the replica's next intent — its second, the first lost
/// with the old authority — is taken at once, its push saying its pending
/// begin after 1. Falsified by a replica passing by a first part at a
/// point of its log: it kept the lost wave.
#[test]
fn a_restarted_authority_fills_and_holds_the_gap() {
    let (mut server, mut live) = (authority(), peer(Role::Replica, "alice", Space::Volatile));
    let o = live.step(waving("phone"));
    let (s, f) = deliver_up(&mut server, &who("alice"), &o);
    for frame in s.iter().chain(&f) {
        live.step(down(frame));
    }
    let mut fresh = authority();
    let o = live.step(Input::Connected);
    let (s, _) = deliver_up(&mut fresh, &who("alice"), &o);
    assert!(
        matches!(
            &s[..],
            [Frame::Part {
                space: Space::Volatile,
                from: None,
                next: None,
                ..
            }]
        ),
        "{s:?}"
    );
    live.step(down(&s[0]));
    assert!(live.store.get("here", &here_key("alice-session")).is_none(), "the restart lost the wave");
    let o = live.step(waving("laptop"));
    let (s, f) = deliver_up(&mut fresh, &who("alice"), &o);
    assert!(matches!(&verdicts(&s)[..], [Some(Verdict::At { seq: 1, last: 2 })]), "{s:?}");
    live.step(down(&f[0]));
    assert!(live.pending.is_empty() && live.store == fresh.store);
}

/// A replica moves its own horizon (`compact`, at most its head): the
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
    alice.compact(i64::MAX);
    assert_eq!((alice.log().entries().len(), alice.log().base_seq()), (0, 5), "it stands on its head");
    let o = bob.step(waving("tablet"));
    let (_, f) = deliver_up(&mut server, &who("bob"), &o);
    alice.step(down(&f[0]));
    assert_eq!(alice.log().head(), server.log().head());
    assert_eq!(alice.pending.len(), 1, "still pending, rebased");
    carry(&mut server, "alice", &pending, &mut [&mut alice]);
    assert!(alice.pending.is_empty() && alice.store == server.store);
    let hello = sends(&alice.step(Input::Connected));
    let (s, _) = deliver_up(&mut server, &who("alice"), &[Output::Send(To::Back, hello[0].enc())]);
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
    let o = log.compact(i64::MAX);
    let rows = o.iter().find_map(|x| match x {
        Output::Send(ark::peer::To::Disk, b) => match ark::log::Record::dec(b) {
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

/// A volatile authority keeps nothing of an intent it took, and the
/// refusals of each origin's last 64 only: a push again of one taken is
/// answered as sequenced, at the head, however many came. Falsified by
/// journaling every verdict, as before: it held 70.
#[test]
fn volatile_verdicts_are_not_kept() {
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
    assert!(journal.refused.is_empty() && journal.last.len() == 1, "{journal:?}");
    for p in [&pushes[69], &pushes[5]] {
        let (s, _) = deliver_up(&mut server, &who("alice"), p);
        assert!(matches!(&verdicts(&s)[..], [Some(Verdict::At { seq: 70, .. })]), "{s:?}");
    }
}

/// `here` with a column appended, `mood`, an `Option`: a schema the
/// waves' grows into.
fn moody() -> Arc<Schema> {
    let mut s = (*schema()).clone();
    let here = s.tables.iter_mut().find(|t| t.name == "here").unwrap();
    here.columns.push(ark::schema::Column {
        name: "mood".into(),
        ty: ark::row::Type::Option(Box::new(ark::row::Type::Text)),
        default: None,
    });
    Arc::new(s)
}

/// A wave as the newer build writes it, with a mood.
fn moody_wave(store: &mut ark::store::Store, ctx: &Ctx, input: &[u8]) -> Result<Vec<ark::store::Change>, String> {
    let Wave { name } = <Wave as ark::format::Encode>::from_bytes(input)?;
    let t = moody().tables.iter().find(|t| t.name == "here").unwrap().clone();
    let row = ark::dynamic::row_bytes(&t, &serde_json::json!({"session": ctx.device(), "name": name, "mood": "glad"}))?;
    let c = ark::store::Change::Add { table: "here".into(), row };
    store.apply(&c)?;
    Ok(vec![c])
}

/// Live state is the log's system (§1.2, §1.9): a volatile authority of a
/// newer build — `here` with a mood — and a replica of the build before
/// sync, the replica holding the wave cut to its columns; and a replica of
/// the newer build syncs with an authority of the older, holding it
/// completed. Neither diverges. Falsified by a replica applying facts of
/// another schema as they are: the older one diverged at the wave.
#[test]
fn live_state_syncs_across_a_schema_that_grew() {
    let fns = Arc::new(Domain {
        schema: moody(),
        ..cas().run("waves.wave", Arc::new(moody_wave))
    });
    let wider = |role: Role, name: &str| Peer::new(fns.clone(), role, who(name)).in_space(Space::Volatile);
    let authority_role = || Role::Authority { journal: Journal::default() };
    for (mut server, mut replica) in [
        (wider(authority_role(), "server"), peer(Role::Replica, "alice", Space::Volatile)),
        (authority(), wider(Role::Replica, "alice")),
    ] {
        let o = replica.step(waving("phone"));
        let (s, f) = deliver_up(&mut server, &who("alice"), &o);
        for frame in s.iter().chain(&f) {
            replica.step(down(frame));
        }
        assert!(replica.pending.is_empty() && replica.diverged.is_empty(), "synced");
        let (_, t) = replica.store.schema_arc().table("here").unwrap();
        let row = replica.store.get("here", &here_key("alice-session")).expect("the wave");
        assert_eq!(ark::dynamic::row(t, &row).unwrap()["name"], "phone");
    }
}
