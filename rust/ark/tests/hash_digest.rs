//! §8.1 The state hash kept beside the rows (`docs/plan-db.md` D3): the
//! digest a store keeps is the digest of its rows however it came to hold
//! them, and a `Verify` after a settle reads digests rather than rows. Each
//! test says what falsified it.

#[path = "support/counting.rs"]
mod counting;

use std::time::Instant;

use ark::authoring::*;
use ark::canon::encode;
use ark::eval::{Args, Ctx};
use ark::hash::{leaf, state_hash, table_digest};
use ark::live::Silent;
use ark::peer::{Authority, Replica, Sequenced};
use ark::protocol::{open_access, trusting, Client, ClientMsg, Mode, Server, ServerMsg};
use ark::schema::{Column, Index, Ref, Schema, Table as SchemaTable, Ty};
use ark::sha256::sha256;
use ark::store::{Change, MemoryStore, Overlay, Row as StoreRow, Store};
use ark::value::Value;

use counting::Counting;

// The random walk -----------------------------------------------------------

/// xorshift64*: the walk is the same every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn walk_schema() -> Schema {
    let col = |n: &str, ty: Ty, nullable: bool| Column {
        name: n.into(),
        ty,
        nullable,
    };
    Schema {
        tables: vec![
            SchemaTable::new(
                "parent",
                vec![col("id", Ty::Int, false), col("name", Ty::Text, true)],
                vec!["id".into()],
                vec![Index {
                    columns: vec!["name".into()],
                    unique: false,
                }],
                vec![],
            ),
            SchemaTable::new(
                "child",
                vec![col("id", Ty::Int, false), col("parent_id", Ty::Int, true), col("n", Ty::Int, false)],
                vec!["id".into()],
                vec![],
                vec![Ref {
                    column: "parent_id".into(),
                    table: "parent".into(),
                }],
            ),
        ],
    }
}

fn parent(sch: &Schema, id: u64, name: Option<u64>) -> StoreRow {
    let name = name.map_or(Value::Null, |n| Value::text(format!("n{n}")));
    StoreRow::of(
        sch.lookup_table("parent").unwrap(),
        [("id".to_string(), Value::Int(id as i64)), ("name".to_string(), name)],
    )
}

fn child(sch: &Schema, id: u64, parent: Option<u64>, n: u64) -> StoreRow {
    let p = parent.map_or(Value::Null, |p| Value::Int(p as i64));
    StoreRow::of(
        sch.lookup_table("child").unwrap(),
        [
            ("id".to_string(), Value::Int(id as i64)),
            ("parent_id".to_string(), p),
            ("n".to_string(), Value::Int(n as i64)),
        ],
    )
}

/// One random write: a put (an add or an edit, over a small key space so
/// that edits are common), a delete (refused while referenced, which
/// changes nothing), or a raw fact — the way a replica takes the
/// authority's changes, unjudged (§4.5).
fn step(r: &mut Rng, sch: &Schema, st: &mut dyn Store) {
    let (k, v) = (r.below(60), r.below(5));
    let raw = r.below(5) == 0;
    match r.below(6) {
        0 | 1 => {
            let row = parent(sch, k, (v > 0).then_some(v));
            if raw {
                st.apply_change(&Change::Add("parent".into(), row));
            } else {
                let _ = st.put("parent", row);
            }
        }
        2 | 3 => {
            let row = child(sch, k, (v > 0).then(|| r.below(60)), r.below(1000));
            let _ = st.put("child", row);
        }
        4 => {
            let _ = st.delete("parent", &[Value::Int(k as i64)]);
        }
        _ => {
            if raw {
                if let Some(row) = st.get("child", &[Value::Int(k as i64)]) {
                    st.apply_change(&Change::Remove("child".into(), row));
                }
            } else {
                let _ = st.delete("child", &[Value::Int(k as i64)]);
            }
        }
    }
}

/// The digest a store keeps, table by table, against a store built afresh
/// from the rows that survived and against the sum of a scan.
fn kept_is_rebuilt(st: &dyn Store, sch: &Schema, at: usize) {
    let rebuilt = MemoryStore::from_value(sch.clone(), &st.store_value());
    for t in sch.tables() {
        let rows = st.scan(&t.name);
        assert_eq!(st.digest(&t.name), rebuilt.digest(&t.name), "{} after {at} writes", t.name);
        assert_eq!(st.digest(&t.name), Some(table_digest(&t.name, &rows)), "{} after {at}", t.name);
    }
    assert_eq!(state_hash(st), state_hash(&rebuilt), "after {at}");
}

/// After 4,000 random puts, edits, deletes and raw facts — checked every
/// 250 — the digest a `MemoryStore` keeps equals the digest of a store
/// rebuilt from the rows that survived, and of a scan; and an `Overlay`
/// written with 400 more over it derives the same from its writes.
/// Falsified by skipping the subtraction in `MemoryStore::set` (an edit or
/// a delete leaves the old row's leaf in): the first check fails, at 250.
#[test]
fn the_kept_digest_is_the_digest_of_the_surviving_rows() {
    let sch = walk_schema();
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let mut st = MemoryStore::empty(sch.clone());
    for i in 1..=4000 {
        step(&mut r, &sch, &mut st);
        if i % 250 == 0 {
            kept_is_rebuilt(&st, &sch, i);
        }
    }
    let counts: Vec<usize> = sch.tables().map(|t| st.scan(&t.name).len()).collect();
    assert!(counts.iter().all(|n| *n > 10), "the walk left rows in both tables: {counts:?}");
    let mut o = Overlay::new(&st);
    for i in 1..=400 {
        step(&mut r, &sch, &mut o);
        if i % 50 == 0 {
            for t in sch.tables() {
                assert_eq!(o.digest(&t.name), Some(table_digest(&t.name, &o.scan(&t.name))), "overlay, {i}");
            }
        }
    }
}

// A Verify after a settle ---------------------------------------------------

pub struct Lists {
    pub playlist: Table<Playlist>,
}
impl Tables for Lists {
    fn open() -> Self {
        Lists { playlist: table() }
    }
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns().id(Self::id).text(Self::name).text(Self::user_id).key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct Create {
    pub name: Text,
}
impl Input for Create {
    fn schema() -> Object<Self> {
        object().field("name", text().min(1))
    }
}

fn lists() -> Router<Lists> {
    let r = router::<Lists>("lists");
    r.routes((r.input::<Create>().mutation("create_playlist", |ctx, db, input| {
        db.playlist.insert(Playlist {
            id: ctx.new_id("id"),
            name: input.name,
            user_id: ctx.user,
        })
    }),))
}

fn key(k: u8, n: u32) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = k;
    b[12..].copy_from_slice(&n.to_be_bytes());
    b
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// An authority with `n` playlists sequenced, and a fresh replica of it.
fn sequenced(n: u32) -> (Authority, Replica) {
    let m = Module::new((lists(),));
    let built = m.build();
    let (sch, bodies) = (built.schema.clone(), ark::hash::closures(built));
    let (create, _) = m.procedure("create_playlist").unwrap();
    let alice = Ctx::new("alice", "dev");
    let mut a = Authority::new(sch.clone(), bodies.clone());
    let mut author = Replica::open(sch.clone(), bodies.clone(), MemoryStore::empty(sch.clone()), 0, vec![]);
    for i in 1..=n {
        let e = author
            .mutate(
                key(9, i),
                &alice,
                &create,
                &args([("id", Value::Id(key(1, i)))]),
                &args([("name", Value::text(format!("p{i}")))]),
            )
            .unwrap();
        author.pending.clear();
        assert!(matches!(a.sequence_entry(&e), Sequenced::Appended(..)));
    }
    let fresh = Replica::open(sch.clone(), bodies, MemoryStore::empty(sch), 0, vec![]);
    (a, fresh)
}

/// Frames between one client and the server until neither has anything to
/// say, the client settling after each turn as a pump does; the `Agree`s
/// it was sent.
fn exchange(sv: &mut Server<Silent>, peer: &mut Client) -> Vec<(i64, bool)> {
    let mut agreed = vec![];
    loop {
        let up = peer.take_outgoing();
        for m in up.iter().cloned() {
            sv.recv(1, m);
        }
        let down: Vec<ServerMsg> = sv.take_outgoing().into_iter().map(|(_, m)| m).collect();
        if up.is_empty() && down.is_empty() {
            return agreed;
        }
        for m in down {
            if let ServerMsg::Agree { seq, ok, .. } = &m {
                agreed.push((*seq, *ok));
            }
            peer.recv(m);
        }
        peer.settle();
    }
}

/// A fresh peer pages 2,000 entries in, settles at the head, and verifies:
/// its claim and the authority's answer are each the state hash of a
/// store of 2,000 rows, and read through the counting store neither reads
/// one — no scan, no `get` — because both stores keep their digests. The
/// server's answer is `ok`. Falsified by `MemoryStore::digest` answering
/// `None` (the store keeping nothing, as before D3): both hashes are the
/// same, and each costs 2,000 rows.
#[test]
fn a_verify_after_a_settle_reads_no_row() {
    let (a, fresh) = sequenced(2000);
    let mut sv = Server::open(trusting(), open_access(), Silent, a);
    let mut peer = Client::open(fresh, Mode::Whole, Some("alice".into()));
    peer.connected();
    exchange(&mut sv, &mut peer);
    assert_eq!(peer.replica.cursor, 2000, "settled at the head");

    let mine = Counting::new(&peer.replica.confirmed);
    let claim = state_hash(&mine);
    assert_eq!((mine.reads().rows, mine.reads().gets), (0, 0), "the peer's claim read no row");
    let theirs = Counting::new(&sv.authority.store);
    assert_eq!(state_hash(&theirs), claim);
    assert_eq!((theirs.reads().rows, theirs.reads().gets), (0, 0), "the authority's answer read no row");
    assert_eq!(peer.replica.verify_at(), (2000, claim));

    peer.verify_all();
    assert!(matches!(peer.out.last(), Some(ClientMsg::Verify { seq: 2000, .. })));
    assert_eq!(exchange(&mut sv, &mut peer), [(2000, true)]);
}

/// A `Verify` at a sequence the authority holds no state at — below its
/// horizon, past its head — is answered "cannot say" (`unknown`), not
/// `ok: false`; one at a sequence it holds is answered as ever, with
/// `unknown` absent from the frame. Falsified by answering a missing state
/// as `(ok: false, unknown: false)`, as before: the answer below the
/// horizon reads as a divergence.
#[test]
fn a_verify_below_the_horizon_is_answered_cannot_say() {
    let (mut a, _) = sequenced(20);
    assert!(a.compact(10));
    let hash = state_hash(&a.store);
    let mut sv = Server::open(trusting(), open_access(), Silent, a);
    let hello = ClientMsg::Hello {
        sub: ark::protocol::Subscription {
            since: 20,
            mode: Mode::Whole,
            log_id: sv.authority.log.id(),
        },
        token: Some("alice".into()),
        spec: ark::ir::SPEC_VERSION,
    };
    sv.recv(1, hello);
    let _ = sv.take_outgoing();
    let mut answer = |seq| {
        sv.recv(1, ClientMsg::Verify { seq, hash: hash.clone() });
        sv.take_outgoing()
            .into_iter()
            .find_map(|(_, m)| match m {
                ServerMsg::Agree { ok, unknown, .. } => Some((ok, unknown)),
                _ => None,
            })
            .expect("an answer")
    };
    assert_eq!(answer(20), (true, false), "at the head");
    assert_eq!(answer(5), (false, true), "below the horizon: cannot say");
    assert_eq!(answer(30), (false, true), "past the head: cannot say");
    assert!(!answer(15).1, "above the horizon it can say");
}

/// `Log::hash_at` below the head, over a log with edits and removes as
/// well as adds, against hashing the replay at every retained sequence:
/// taking the facts above `n` back off the head's digests is `state_at(n)`.
/// Falsified by subtracting an edit's old row and adding its new one (the
/// swap the wrong way round): every sequence below the first edit
/// disagrees.
#[test]
fn the_hash_below_the_head_is_the_replays() {
    let sch = walk_schema();
    let mut r = Rng(7);
    let mut a = Authority::new(sch.clone(), Default::default());
    let mut st = MemoryStore::empty(sch.clone());
    let mut kinds = [0usize; 3];
    for i in 0..300u32 {
        let before = st.clone();
        step(&mut r, &sch, &mut st);
        // What the write changed, as the facts an entry records: each key
        // that moved, as an add, a remove or an edit of what was there.
        let mut facts = vec![];
        for t in sch.tables() {
            let (old, new) = (before.rows(&t.name), st.rows(&t.name));
            for (k, o) in &old {
                match new.get(k) {
                    None => facts.push(Change::Remove(t.name.clone(), o.clone())),
                    Some(n) if n != o => facts.push(Change::Edit(t.name.clone(), o.clone(), n.clone())),
                    _ => {}
                }
            }
            for (k, n) in &new {
                if !old.contains_key(k) {
                    facts.push(Change::Add(t.name.clone(), n.clone()));
                }
            }
        }
        for f in &facts {
            kinds[match f {
                Change::Add(..) => 0,
                Change::Remove(..) => 1,
                Change::Edit(..) => 2,
            }] += 1;
        }
        let e = ark::log::Entry {
            id: key(3, i),
            actor: "a".into(),
            session: "s".into(),
            fn_hash: vec![1],
            args: Args::new(),
            autos: Args::new(),
        };
        a.log.append(e, facts);
    }
    assert!(kinds.iter().all(|n| *n > 10), "adds, removes and edits: {kinds:?}");
    assert_eq!(state_hash(&a.log.state_at(300).unwrap()), state_hash(&st));
    for n in 0..=300 {
        assert_eq!(a.log.hash_at(n, &st), Some(state_hash(&a.log.state_at(n).unwrap())), "at {n}");
    }
}

// The cost ------------------------------------------------------------------

/// §8.1 as it was until D3: SHA-256 over every table's name and rows in
/// key order — what a `Verify` cost before, kept here to be measured.
fn state_hash_before(st: &dyn Store) -> Vec<u8> {
    let tables: Vec<Value> = st
        .schema()
        .tables()
        .map(|t| {
            Value::List(vec![
                Value::text(&t.name),
                Value::List(st.scan(&t.name).into_iter().map(|r| r.into_value()).collect()),
            ])
        })
        .collect();
    sha256(&encode(&Value::List(tables)))
}

fn us(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

/// What a `Verify` costs at 8,000 rows, before D3 and after: the claim
/// (the peer's state hash), the server's answer at the head and a hundred
/// entries below it, and what keeping the digest adds to a write (one leaf
/// hashed per row in, one per row out). Printed, not asserted; run with
/// `cargo test -p ark --release --test hash_digest -- --ignored --nocapture`.
#[test]
#[ignore]
fn perf_verify_at_8000_rows() {
    let (a, _) = sequenced(8000);
    let head = a.log.head_seq();
    let st = a.store.clone();
    let reps = 200u32;
    let time = |f: &dyn Fn() -> Vec<u8>| {
        let t = Instant::now();
        for _ in 0..reps {
            std::hint::black_box(f());
        }
        us(t.elapsed()) / f64::from(reps)
    };
    let _ = time(&|| state_hash(&st));
    let before = time(&|| state_hash_before(&st));
    let after = time(&|| state_hash(&st));
    let scanned = time(&|| {
        let t = "playlist";
        let d = table_digest(t, &st.scan(t));
        ark::hash::state_hash_of([(t, d)])
    });
    let below = time(&|| a.log.hash_at(head - 100, &a.store).unwrap());
    let replay = time(&|| state_hash(&a.log.state_at(head - 100).unwrap()));
    eprintln!("state hash of 8,000 rows, µs per call (mean of {reps}):");
    eprintln!("  before D3: sha256 over every row            {before:>10.1}");
    eprintln!("  after:     digests read                      {after:>10.1}");
    eprintln!("  after:     digests summed from a scan        {scanned:>10.1}");
    eprintln!("  hash_at(head - 100), facts taken back        {below:>10.1}");
    eprintln!("  …as replayed from the base (before D3)       {replay:>10.1}");

    let mut sv = Server::open(trusting(), open_access(), Silent, a);
    let hash = state_hash(&st);
    let hello = ClientMsg::Hello {
        sub: ark::protocol::Subscription {
            since: head,
            mode: Mode::Whole,
            log_id: sv.authority.log.id(),
        },
        token: Some("alice".into()),
        spec: ark::ir::SPEC_VERSION,
    };
    sv.recv(1, hello);
    let _ = sv.take_outgoing();
    let t = Instant::now();
    for _ in 0..reps {
        sv.recv(
            1,
            ClientMsg::Verify {
                seq: head,
                hash: hash.clone(),
            },
        );
    }
    let served = us(t.elapsed()) / f64::from(reps);
    assert!(sv.take_outgoing().iter().all(|(_, m)| matches!(m, ServerMsg::Agree { ok: true, .. })));
    eprintln!("  Server::recv(Verify at the head), after      {served:>10.1}");

    let rows = st.scan("playlist");
    let t = Instant::now();
    for r in rows.iter().take(1000) {
        std::hint::black_box(leaf("playlist", r));
    }
    eprintln!("  one leaf (what a write adds per row moved)   {:>10.2}", us(t.elapsed()) / 1000.0);
}
