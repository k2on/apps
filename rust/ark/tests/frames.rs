//! §1.11 held: every frame is its JSON and reads back as the frame.

#[path = "demo.rs"]
mod demo;

use ark::format::Encode;
use ark::frames::{Frame, Verdict};
use ark::hash::{EntryHash, SchemaHash, StateHash};
use ark::log::{Entry, EntryId, Logged, Origin, Record, Snapshot, Span, TraceId};
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
        input: br#"{"name":"a\u0000b","n":-1}"#.to_vec(),
        now: -5,
        cause: Some(Span {
            trace: TraceId([1; 16]),
            id: [2; 16],
        }),
    };
    let facts = vec![
        Change::Add {
            table: "item".into(),
            row: br#"{"k":0}"#.to_vec(),
        },
        Change::Remove {
            table: "item".into(),
            row: b"{}".to_vec(),
        },
        Change::Edit {
            table: "p".into(),
            old: br#"{"k":1}"#.to_vec(),
            new: br#"{"k":[2,null]}"#.to_vec(),
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
        rows: vec![("item".into(), vec![br#"{"k":0}"#.to_vec(), b"{}".to_vec()]), ("p".into(), vec![])],
    };
    let id = entry.id;
    vec![
        Frame::Hello {
            space: Space::Durable,
            cursor: 0,
            hash: None,
            device: None,
            schema: SchemaHash([0; 32]),
            version: String::new(),
        },
        Frame::Hello {
            space: Space::Volatile,
            cursor: 4,
            hash: Some(EntryHash([0xFF; 32])),
            device: Some(b"phone\0".to_vec()),
            schema: SchemaHash([2; 32]),
            version: "1.2".into(),
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
            verdict: Verdict::Held { func: "demo.gone".into() },
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

/// Each frame is its JSON and reads back as itself, whole: a byte added or
/// cut is refused; a logged record is the variant's object around the
/// entry's row as it is. Falsified by carrying a change's row as a JSON
/// string instead of the JSON it is: the logged record was not its row
/// wrapped.
#[test]
fn every_frame_reads_back() {
    for f in every_frame() {
        let b = f.enc();
        assert_eq!(Frame::dec(&b), Ok(f.clone()), "{}", String::from_utf8_lossy(&b));
        let mut longer = b.clone();
        longer.push(b'x');
        assert!(Frame::dec(&longer).is_err());
        assert!(Frame::dec(&b[..b.len() - 1]).is_err());
    }
    let l = match &every_frame()[7] {
        Frame::Page { logged, .. } => logged[0].clone(),
        _ => unreachable!(),
    };
    let r = Record::Logged(l.clone());
    assert_eq!(Record::logged(&l.to_bytes()), r.enc(), "a logged record is the variant, then the row");
    assert_eq!(Record::dec(&r.enc()), Ok(r));
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
