//! §1.11 held: every frame's bytes are canonical bytes of the frame type
//! the table in `frames.rs` writes, and read back as the frame.

#[path = "demo.rs"]
mod demo;

use ark5::codec::{parse, Reader};
use ark5::frames::{frame_ty, ty, Frame};
use ark5::hash::{EntryHash, FnHash, StateHash};
use ark5::log::{Entry, EntryId, Logged, Origin, Record, Snapshot};
use ark5::schema::Space;
use ark5::store::Change;

fn every_frame() -> Vec<Frame> {
    let entry = Entry {
        id: EntryId::of(Origin::named("s"), 1),
        actor: "a\0b".into(),
        session: "s".into(),
        origin: Origin::named("s"),
        local: 1,
        roles: vec!["r0".into(), "r1".into()],
        func: demo::h(demo::ADD),
        input: vec![0, 0xFF, 1],
        now: -5,
    };
    let facts = vec![
        Change::Add {
            table: "item".into(),
            row: vec![0],
        },
        Change::Remove {
            table: "item".into(),
            row: vec![],
        },
        Change::Edit {
            table: "p".into(),
            old: vec![1],
            new: vec![2, 0],
        },
    ];
    let l = Logged {
        seq: 3,
        entry: entry.clone(),
        facts: facts.clone(),
        state: StateHash([7; 32]),
        hash: EntryHash([0; 32]),
    };
    let snapshot = Snapshot {
        seq: 9,
        hash: EntryHash([1; 32]),
        state: StateHash([0; 32]),
        rows: vec![("item".into(), vec![vec![0, 1], vec![]]), ("p".into(), vec![])],
    };
    let id = entry.id;
    vec![
        Frame::Hello {
            space: Space::Durable,
            cursor: 0,
            hash: None,
            device: None,
        },
        Frame::Hello {
            space: Space::Volatile,
            cursor: 4,
            hash: Some(EntryHash([0xFF; 32])),
            device: Some(b"phone\0".to_vec()),
        },
        Frame::Push {
            space: Space::Volatile,
            entry: entry.clone(),
        },
        Frame::Push {
            space: Space::Durable,
            entry,
        },
        Frame::Ack {
            space: Space::Durable,
            id,
            seq: 3,
            facts: None,
        },
        Frame::Ack {
            space: Space::Volatile,
            id,
            seq: 3,
            facts: Some(facts),
        },
        Frame::Reject {
            space: Space::Durable,
            id,
            why: "a playlist needs a name".into(),
        },
        Frame::Held {
            space: Space::Durable,
            id,
            func: FnHash([3; 32]),
        },
        Frame::Batch {
            space: Space::Durable,
            from: 2,
            logged: vec![l.clone(), l.clone()],
        },
        Frame::Batch {
            space: Space::Durable,
            from: 0,
            logged: vec![],
        },
        Frame::Snapshot {
            space: Space::Durable,
            snapshot,
        },
        Frame::Join {
            space: Space::Durable,
            from: 0,
            hash: EntryHash::default(),
            logged: vec![l],
        },
    ]
}

/// Each frame parses under `frame_ty()` whole, decodes to itself, and a
/// byte added or cut is refused or read as other canonical bytes. Falsified
/// by giving `Hello`'s row of the table `Bytes` for its cursor instead of
/// `Int`: the hello frames no longer parsed.
#[test]
fn every_frame_is_its_type() {
    let t = frame_ty();
    for f in every_frame() {
        let b = f.enc();
        parse(&t, &b).unwrap_or_else(|e| panic!("{f:?}: {e}"));
        assert_eq!(Frame::dec(&b), Ok(f.clone()));
        let mut longer = b.clone();
        longer.push(0);
        assert!(Frame::dec(&longer).is_err() && parse(&t, &longer).is_err());
        assert!(Frame::dec(&b[..b.len() - 1]).is_err());
    }
    let r = Record::Logged(match &every_frame()[8] {
        Frame::Batch { logged, .. } => logged[0].clone(),
        _ => unreachable!(),
    });
    parse(&ty("Record"), &r.enc()).unwrap();
    assert_eq!(Record::dec(&r.enc()), Ok(r));
    let mut rd = Reader::new(&[8]);
    assert!(rd.variant(8).is_err(), "there are eight frames");
}
