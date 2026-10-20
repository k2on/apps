//! §1.11 held: every frame's bytes are canonical bytes of the frame type
//! the table in `frames.rs` writes, and read back as the frame.

#[path = "demo.rs"]
mod demo;

use ark::codec::{parse, Reader};
use ark::frames::{frame_ty, ty, Frame, Verdict};
use ark::hash::{EntryHash, FnHash, StateHash};
use ark::ir::Node;
use ark::log::{Entry, EntryId, Logged, Origin, Record, Snapshot};
use ark::schema::Space;
use ark::store::Change;

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
        Frame::push(Space::Volatile, [entry.clone()]),
        Frame::push(Space::Durable, [entry.clone(), entry]),
        Frame::Verdict {
            space: Space::Durable,
            id,
            verdict: Verdict::Ok { seq: 3 },
        },
        Frame::Verdict {
            space: Space::Volatile,
            id,
            verdict: Verdict::Refused {
                why: "a playlist needs a name".into(),
            },
        },
        Frame::Verdict {
            space: Space::Durable,
            id,
            verdict: Verdict::Held { func: FnHash([3; 32]) },
        },
        Frame::Page {
            space: Space::Durable,
            from: 2,
            logged: vec![l.clone(), l.clone()],
        },
        Frame::Page {
            space: Space::Durable,
            from: 0,
            logged: vec![],
        },
        Frame::Snapshot {
            space: Space::Durable,
            snapshot,
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
    let l = match &every_frame()[7] {
        Frame::Page { logged, .. } => logged[0].clone(),
        _ => unreachable!(),
    };
    let r = Record::Logged(l.clone());
    assert_eq!(Record::logged(&l.to_bytes()), r.enc(), "a logged record is the variant, then the row");
    parse(&ty("Record"), &r.enc()).unwrap();
    assert_eq!(Record::dec(&r.enc()), Ok(r));
    let mut rd = Reader::new(&[4]);
    assert!(rd.variant(4).is_err(), "there are four frames");
}

/// A push is a page of intents: each at sequence 0 with no facts and zero
/// hashes, its bytes those of `Logged::intent`, so the protocol carries
/// one shape of entry. Falsified by `Frame::push` keeping the entry's
/// `local` as its sequence: the page's first entry was at sequence 1.
#[test]
fn a_push_is_a_page_of_intents() {
    let f = every_frame().remove(3);
    let Frame::Page { from, logged, .. } = &f else { panic!("{f:?}") };
    assert_eq!(*from, 0);
    assert!(logged
        .iter()
        .all(|l| l.seq == 0 && l.facts.is_empty() && l == &Logged::intent(l.entry.clone())));
    assert_eq!(logged.len(), 2);
}
