//! §1.11 held: every frame's bytes are canonical bytes of the frame type
//! the table in `frames.rs` writes, and read back as the frame.

#[path = "demo.rs"]
mod demo;

use ark5::codec::{parse, Reader};
use ark5::frames::{frame_ty, ty, Frame};
use ark5::hash::{EntryHash, FnHash, StateHash};
use ark5::log::{Entry, EntryId, Logged, Record, Snapshot};
use ark5::schema::Space;
use ark5::store::Change;

fn every_frame() -> Vec<Frame> {
    let entry = Entry {
        id: EntryId::of("s", 1),
        actor: "a\0b".into(),
        session: "s".into(),
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
            device: Some("phone".into()),
        },
        Frame::Push {
            space: Space::Durable,
            entry: entry.clone(),
            body: None,
        },
        Frame::Push {
            space: Space::Durable,
            entry,
            body: Some(b"demo".to_vec()),
        },
        Frame::Fetch {
            hashes: vec![FnHash([2; 32]), FnHash::default()],
        },
        Frame::Ack { id, seq: 3, facts: None },
        Frame::Ack {
            id,
            seq: 3,
            facts: Some(facts),
        },
        Frame::Reject {
            id,
            why: "a playlist needs a name".into(),
        },
        Frame::Held { id, func: FnHash([3; 32]) },
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
        Frame::Bodies {
            bodies: vec![(FnHash([4; 32]), vec![0, 0])],
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
    let r = Record::Logged(match &every_frame()[9] {
        Frame::Batch { logged, .. } => logged[0].clone(),
        _ => unreachable!(),
    });
    parse(&ty("Record"), &r.enc()).unwrap();
    assert_eq!(Record::dec(&r.enc()), Ok(r));
    let mut rd = Reader::new(&[10]);
    assert!(rd.variant(10).is_err(), "there are ten frames");
}
