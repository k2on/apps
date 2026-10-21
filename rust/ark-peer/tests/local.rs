//! What a peer keeps beside its log, as rows of its local instance, held:
//! an instance's rows read back after a reopen, its journal folded; an
//! ephemeral one kept nowhere; a server's journal — each origin's last,
//! what waits behind a held intent — across a restart; a device's replica
//! row across a reopen.

use std::sync::Arc;

use ark::db::Volatile;
use ark::format::Encode;
use ark::frames::{Frame, Verdict};
use ark::log::{Entry, EntryId, Origin};
use ark::peer::{Role, Who};
use ark::prelude::*;
use ark::schema::Space;
use ark_peer::{dev, Config, Disk, Instance, Memory, Peer};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
pub struct Note {
    #[ark(key)]
    pub n: i64,
    pub text: String,
}

#[derive(Schema)]
pub struct Notes {
    pub note: Note,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(volatile)]
pub struct Seen {
    #[ark(key)]
    pub who: String,
}

#[derive(Schema)]
pub struct Room {
    pub seen: Seen,
}

fn note(n: i64) -> Note {
    Note {
        n,
        text: format!("note {n}"),
    }
}

/// What an instance wrote reads back from its disk, a refused run leaving
/// nothing; after more writes than rows its journal is folded into the
/// rows, and still reads back. Falsified by `Instance::write` not making
/// the entry durable (its `Persist` dropped): the reopen read no rows.
#[test]
fn an_instance_reads_back_what_it_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let files = || Box::new(ark_peer::Files::new(dir.path()).unwrap()) as Box<dyn Disk>;
    let mut i = Instance::<Notes>::open(files());
    for n in 0..3 {
        i.write(|db| db.note.insert(note(n))).unwrap();
    }
    assert!(i.write(|db| db.note.insert(note(1))).is_err(), "a key taken");
    let i = Instance::<Notes>::open(i.into_disk());
    assert_eq!(i.read(|db| db.note.scan()), (0..3).map(note).collect::<Vec<_>>());
    let mut i = Instance::<Notes>::open(files());
    for n in 0..200 {
        i.write(|db| {
            db.note.put(Note {
                n: n % 5,
                text: format!("{n}"),
            })
        })
        .unwrap();
    }
    let log = std::fs::metadata(dir.path().join("log")).unwrap().len();
    assert!(log < 64 * 200, "folded: the journal holds {log} bytes");
    let i = Instance::<Notes>::open(files());
    let want: Vec<Note> = (195..200)
        .map(|n| Note {
            n: n % 5,
            text: format!("{n}"),
        })
        .collect();
    assert_eq!(i.read(|db| db.note.scan()), want);
    assert!(i.failure.is_none());
}

/// An ephemeral instance holds its volatile rows and writes them nowhere.
/// Falsified by opening it in the durable space: its machine refused the
/// volatile row ("not in this space").
#[test]
fn an_ephemeral_instance_holds_volatile_rows() {
    let mut i = Instance::<Room, Volatile>::ephemeral();
    i.write(|db| db.seen.insert(Seen { who: "alice".into() })).unwrap();
    assert_eq!(i.read(|db| db.seen.scan()).len(), 1);
}

fn config() -> Config {
    Config {
        keep: Some(1),
        ..Config::of(Arc::new(ark_dev::demo::demo()))
    }
}

fn push(who: &Who, local: i64, func: &str, name: &str) -> Vec<u8> {
    let origin = Origin::named(&who.session);
    let entry = Entry {
        id: EntryId::of(origin, local),
        actor: who.actor.clone(),
        session: who.session.clone(),
        origin,
        local,
        roles: vec![],
        func: func.into(),
        input: ark_dev::demo::CreatePlaylist { name: name.into() }.to_bytes(),
        now: 1,
    };
    Frame::push(Space::Durable, [entry]).enc()
}

const CREATE: &str = "demo.create_playlist";

fn verdicts(out: &[(u64, Vec<u8>)]) -> Vec<Verdict> {
    out.iter()
        .filter_map(|(_, b)| match Frame::dec(b) {
            Ok(Frame::Verdict { verdict, .. }) => Some(verdict),
            _ => None,
        })
        .collect()
}

fn restarted(mut hub: Peer) -> Peer {
    let disk = std::mem::replace(&mut hub.disk, Box::new(Memory::default()));
    Peer::serve(config(), disk, 0)
}

/// An origin whose every entry is below the horizon is remembered by its
/// row of `origin`: after a restart its next intent is taken, not held for
/// a gap. Falsified by `keep_journal` not putting the `origin` rows: alice's
/// third intent was answered with nothing.
#[test]
fn the_last_of_each_origin_survives_a_restart() {
    let (alice, bob) = (dev("alice@laptop").unwrap(), dev("bob@phone").unwrap());
    let mut hub = Peer::serve(config(), Box::new(Memory::default()), 0);
    let (a, b) = (hub.accept(alice.clone()), hub.accept(bob.clone()));
    for n in 1..=2 {
        hub.frame(a, push(&alice, n, CREATE, &format!("A{n}")), 1);
    }
    for n in 1..=4 {
        hub.frame(b, push(&bob, n, CREATE, &format!("B{n}")), 1);
    }
    let entries = hub.log.log().entries();
    assert!(entries.iter().all(|l| l.entry.actor != "alice"), "alice's are below the horizon");
    let mut hub = restarted(hub);
    let a = hub.accept(alice.clone());
    let out = hub.frame(a, push(&alice, 3, CREATE, "A3"), 1);
    assert!(matches!(verdicts(&out)[..], [Verdict::Ok { .. }]), "{:?}", verdicts(&out));
}

/// What waits behind a held intent is a row of `waiting`: after a restart
/// the held one and the one behind it wait still, in order, as who pushed
/// them. Falsified by `Peer::serve` not reading the `waiting` rows: nothing
/// waited after the restart.
#[test]
fn what_waits_survives_a_restart() {
    let alice = dev("alice@laptop").unwrap();
    let mut hub = Peer::serve(config(), Box::new(Memory::default()), 0);
    let a = hub.accept(alice.clone());
    let held = verdicts(&hub.frame(a, push(&alice, 1, "demo.gone", "x"), 1));
    assert!(matches!(held[..], [Verdict::Held { .. }]), "{held:?}");
    assert!(verdicts(&hub.frame(a, push(&alice, 2, CREATE, "Road"), 1)).is_empty(), "it waits");
    let hub = restarted(hub);
    let Role::Authority { journal } = &hub.log.role else {
        panic!("an authority")
    };
    let waits: Vec<(String, i64)> = journal.waiting.values().flatten().map(|(w, e)| (w.session.clone(), e.local)).collect();
    assert_eq!(waits, vec![(alice.session.clone(), 1), (alice.session.clone(), 2)]);
}

/// A device's replica row is what it opens as: its origin, its fork while
/// alone, who last signed in, its last `local`. Falsified by `Peer::alone`
/// not putting the fork: the reopened device was a replica.
#[test]
fn a_device_reopens_as_its_replica_row() {
    let config = Config::of(Arc::new(ark_dev::demo::demo()));
    let mut d = Peer::open_as(config.clone(), Box::new(Memory::default()), None);
    d.alone();
    let _ = d.author(
        Space::Durable,
        CREATE,
        ark_dev::demo::CreatePlaylist { name: "Road".into() }.to_bytes(),
        1,
    );
    let (origin, me, local) = (d.log.origin, d.log.me.clone(), d.log.local);
    let mut d = d.reopen();
    assert!(d.is_alone(), "alone still");
    assert_eq!((d.log.origin, d.log.me.session.clone(), d.log.local), (origin, me.session.clone(), local));
    let disk = std::mem::replace(&mut d.disk, Box::new(Memory::default()));
    let d = Peer::open_as(config, disk, None);
    assert_eq!((d.log.origin, &d.log.me.session), (origin, &me.session), "who last signed in, read back");
}

/// A login remembered is recalled by the next run of the app, from its
/// row; forgotten, it is gone. Falsified by `remember::remember` writing
/// nothing (its `put` dropped): the next run recalled no login.
#[test]
fn a_login_is_remembered_across_runs() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_CONFIG_HOME", dir.path());
    let login = ark_auth::Login {
        token: "t0k".into(),
        session: "s1".into(),
        user: ark_auth::Account {
            id: "alice".into(),
            name: "Alice".into(),
            email: String::new(),
            roles: vec!["library".into()],
        },
        expires_ms: 42,
    };
    ark_peer::Logins::new("app").remember("http://a", &login);
    assert_eq!(ark_peer::Logins::new("app").recall("http://a"), Some(login));
    assert_eq!(ark_peer::Logins::new("app").recall("http://b"), None);
    ark_peer::Logins::new("app").forget("http://a");
    assert_eq!(ark_peer::Logins::new("app").recall("http://a"), None);
    assert!(dir.path().join("app/logins/log").exists());
}
