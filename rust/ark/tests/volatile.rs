//! §1.9 held at the machine: the volatile space's machine (`ark::live`) is
//! its own, lighter one, sharing a connection with the log's — its frames
//! name their space, its store hashes its own tables, it makes nothing
//! durable and keeps no history, a change to the other space is refused
//! by the same judgment as the log's, every `Hello` is answered with the
//! rows, and an authority that restarted takes a session's pushes past the
//! gap its restart made.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use std::sync::Arc;

use ark::frames::{Frame, Verdict};
use ark::live::{Live, REMEMBERED};
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

fn log(role: Role, name: &str) -> Peer {
    Peer::new(schema(), cas(), role, who(name))
}

fn live(name: &str) -> Live {
    Live::new(schema(), cas(), false, who(name))
}

fn authority() -> Live {
    Live::new(schema(), cas(), true, who("server"))
}

/// A replica's frames to the authority as `w`: what it answered, and what
/// it sent everyone.
fn deliver(server: &mut Live, w: &ark::peer::Who, outs: &[Output]) -> (Vec<Frame>, Vec<Frame>) {
    let (mut s, mut f) = (vec![], vec![]);
    for frame in sends(outs) {
        let o = server.step(up(w, &frame));
        s.extend(sends(&o));
        f.extend(fanouts(&o));
    }
    (s, f)
}

/// Every frame of a round trip given to the replica.
fn back(live: &mut Live, (s, f): (Vec<Frame>, Vec<Frame>)) {
    for frame in s.iter().chain(&f) {
        live.step(down(frame));
    }
}

fn waving(name: &str) -> Input {
    Input::Mutate {
        func: "waves.wave".into(),
        input: Wave { name: name.into() }.to_bytes(),
        now: 1,
    }
}

/// A wave goes up and comes back in the volatile space only: every frame
/// names it, nothing is made durable, the log's machine on the same
/// connection takes none of it, the volatile hash moves and the durable
/// does not. Falsified by `frame` not filtering `Ack` by space: the
/// durable replica settled its own pending intent of the same id on the
/// volatile authority's `Ack` (its head past the `Ack`'s sequence).
#[test]
fn a_volatile_intent_is_its_own_space_end_to_end() {
    let (mut server, mut live, mut log) = (authority(), live("alice"), log(Role::Replica, "alice"));
    let mut durable = self::log(Role::Authority { journal: Journal::default() }, "server");
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
    let (s, f) = deliver(&mut server, &who("alice"), &o);
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
    assert_eq!((live.store == server.store, live.seq), (true, 1));
    assert!(live.store.get("here", &here_key("alice-session")).is_some());
}

/// A volatile intent that writes a durable table is refused, and so is a
/// durable one writing a volatile table: one judgment (`peer::run`) for
/// both machines. Falsified by `run` judging no space: the playlist was
/// created in the volatile store.
#[test]
fn a_change_to_the_other_space_is_refused() {
    let mut live = live("alice");
    let o = live.step(mutate(demo::CREATE, demo::create_input("Mix"), 1));
    assert!(
        matches!(&o[..], [Output::Refused { why, .. }] if why == "playlist is not in this space"),
        "{o:?}"
    );
    let mut log = log(Role::Replica, "alice");
    let o = log.step(waving("phone"));
    assert!(
        matches!(&o[..], [Output::Refused { why, .. }] if why == "here is not in this space"),
        "{o:?}"
    );
}

/// An authority restarted from nothing: the replica's `Hello` is answered
/// with its rows, from a `seq` below the replica's, and the replica's next
/// intent — its second, the first lost with the old authority — is taken.
/// Falsified by keeping the durable gap rule: the second wave waited for a
/// first that would never come.
#[test]
fn a_restarted_authority_resyncs_and_takes_the_gap() {
    let (mut server, mut live) = (authority(), live("alice"));
    let o = live.step(waving("phone"));
    back(&mut live, deliver(&mut server, &who("alice"), &o));
    let mut fresh = authority();
    let o = live.step(Input::Connected);
    let (s, _) = deliver(&mut fresh, &who("alice"), &o);
    assert!(matches!(&s[..], [Frame::Snapshot { space: Space::Volatile, .. }]), "{s:?}");
    live.step(down(&s[0]));
    assert!(live.store.get("here", &here_key("alice-session")).is_none(), "the restart lost the wave");
    assert_eq!(live.seq, 0, "back to the restarted authority's");
    let o = live.step(waving("laptop"));
    let (s, f) = deliver(&mut fresh, &who("alice"), &o);
    assert!(matches!(&verdicts(&s)[..], [Some(Verdict::Ok { seq: 1 })]), "{s:?}");
    live.step(down(&f[0]));
    assert!(live.pending.is_empty() && live.store == fresh.store);
}

/// A volatile push that comes after a later one of its session was taken
/// is refused as overtaken, and its replica lets it go. Falsified by
/// `push` without the rule: the first was taken after the second.
#[test]
fn a_volatile_push_overtaken_is_refused() {
    let (mut server, mut live) = (authority(), live("alice"));
    let first = live.step(waving("phone"));
    let second = live.step(waving("laptop"));
    for o in [&second, &first] {
        back(&mut live, deliver(&mut server, &who("alice"), o));
    }
    assert!(live.pending.is_empty(), "both answered");
    assert_eq!(server.seq, 1, "only the later one taken");
}

/// A replica that joins after a hundred entries is sent the rows as they
/// are, in one snapshot at the authority's `seq`, and stands where the
/// authority does; neither keeps an entry. Falsified by answering a
/// `Hello` at cursor 0 with nothing: the late replica stood at 0, empty.
#[test]
fn a_replica_joining_mid_session_is_sent_the_rows() {
    let mut server = authority();
    for n in 0..100 {
        let w = who(&format!("d{}", n % 4));
        let mut l = live(&w.actor);
        l.local = n;
        let o = l.step(waving(&format!("at {n}")));
        deliver(&mut server, &w, &o);
    }
    let mut late = live("late");
    let (s, f) = deliver(&mut server, &who("late"), &late.step(Input::Connected));
    assert!(
        matches!(&s[..], [Frame::Snapshot { space: Space::Volatile, snapshot }] if snapshot.seq == 100),
        "{s:?}"
    );
    assert!(f.is_empty());
    back(&mut late, (s, f));
    assert_eq!((late.seq, &late.store), (100, &server.store));
    assert_eq!(late.store.rows("here").count(), 4, "one row per device");
    assert!(server.store.log().is_empty() && late.store.log().is_empty(), "no history kept");
}

/// An intent pushed again after its verdict was lost is answered with the
/// verdict it was given, not refused as overtaken; and what the authority
/// remembers of an origin stays at [`REMEMBERED`] verdicts. Falsified by
/// `push` remembering no verdict: the repushed wave was refused as
/// overtaken and the replica told so.
#[test]
fn a_push_again_is_answered_as_it_was() {
    let (mut server, mut live) = (authority(), live("alice"));
    let o = live.step(waving("phone"));
    deliver(&mut server, &who("alice"), &o);
    let o = live.step(Input::Connected);
    let (s, f) = deliver(&mut server, &who("alice"), &o);
    assert!(matches!(&verdicts(&s)[..], [None, Some(Verdict::Ok { seq: 1 })]), "{s:?}");
    let mut told = vec![];
    for frame in s.iter().chain(&f) {
        told.extend(live.step(down(frame)));
    }
    assert!(told.iter().any(|o| matches!(o, Output::Confirmed(_))), "{told:?}");
    assert!(!told.iter().any(|o| matches!(o, Output::Refused { .. })), "{told:?}");
    for n in 0..2 * REMEMBERED {
        let o = live.step(waving(&format!("{n}")));
        back(&mut live, deliver(&mut server, &who("alice"), &o));
    }
    let judged = server.authority.as_ref().unwrap();
    assert_eq!(judged.verdicts[&live.origin].len(), REMEMBERED);
}

/// An entry a replica missed — a gap in what it was sent — is not applied:
/// the replica says `Hello` once and the snapshot it is answered with puts
/// it where the authority is, its pending intent on top and pushed again.
/// Falsified by `confirm` applying an entry past a gap: carol's row was in
/// the replica's store at 0, before bob's.
#[test]
fn a_gap_is_healed_by_the_rows() {
    let (mut server, mut live) = (authority(), live("alice"));
    let (mut bob, mut carol) = (self::live("bob"), self::live("carol"));
    let first = deliver(&mut server, &who("bob"), &bob.step(waving("one")));
    let second = deliver(&mut server, &who("carol"), &carol.step(waving("two")));
    let mine = live.step(waving("phone"));
    let o = live.step(down(&second.1[0]));
    let hello = sends(&o);
    assert!(
        matches!(
            &hello[..],
            [Frame::Hello {
                space: Space::Volatile,
                hash: None,
                ..
            }]
        ),
        "{hello:?}"
    );
    assert_eq!(live.seq, 0);
    assert!(
        live.store.get("here", &here_key("carol-session")).is_none(),
        "nothing past the gap applied"
    );
    assert!(live.step(down(&second.1[0])).iter().all(|o| !matches!(o, Output::Send(_))), "asked once");
    let (s, _) = deliver(&mut server, &who("alice"), &o);
    let o = live.step(down(&s[0]));
    assert_eq!(live.seq, 2);
    assert_eq!(live.pending.len(), 1, "its own still pending");
    assert!(matches!(&sends(&o)[..], [Frame::Page { .. }]), "pushed again");
    assert!(
        live.step(down(&first.1[0])).iter().all(|o| !matches!(o, Output::Changes(_))),
        "one it has, skipped"
    );
    back(&mut live, deliver(&mut server, &who("alice"), &mine));
    assert!(live.pending.is_empty() && live.store == server.store && live.seq == 3);
}
