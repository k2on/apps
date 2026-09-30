//! What the engine's paths cost as the data grows, printed rather than
//! asserted: the evidence an open-ended performance pass ranks its work by.
//! Every path is run at sizes four and sixteen times apart, and reported
//! per operation — the mean over all of them, over the first hundred and
//! over the last hundred — so that growth is a ratio on one line rather
//! than an impression across three.
//!
//! Engine-shaped, over the spec's demo (`spec/AUTHORING.md` Appendix B,
//! authored below as `ark-client`'s `demo` has it): the log machine, the
//! authority, the rebase, the server's fan-out and the wire. What a peer
//! costs with its storage and a server in the loop is `ark-server`'s
//! `tests/perf.rs`; what harken's own schema costs is
//! `harken/domain/tests/perf.rs`.
//!
//! Ignored by default; run with
//! `cargo test -p ark --release --test perf -- --ignored --nocapture --test-threads=1`.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use ark::authoring::*;
use ark::canon;
use ark::eval::{Args, Ctx};
use ark::hash::{closures, state_hash, Closure, FnHash};
use ark::live::{ConnId, Silent};
use ark::log::{Entry, Facts, Seq};
use ark::peer::{local_commit, Authority, Procedure, Replica, Sequenced};
use ark::protocol::{open_access, trusting, ClientMsg, Mode, Server, ServerMsg, Subscription, BATCH_LIMIT};
use ark::schema::Schema;
use ark::store::{MemoryStore, Store};
use ark::value::Value;

// The demo ---------------------------------------------------------------------

pub struct Demo {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
}
impl Tables for Demo {
    fn open() -> Self {
        Demo {
            playlist: table(),
            item: table(),
        }
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
        columns()
            .id(Self::id)
            .text(Self::name)
            .text(Self::user_id)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
#[allow(non_upper_case_globals)]
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct Item {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
    pub pos: Int,
}
impl Row for Item {
    const NAME: &str = "item";
    type Key = (Id<Playlist>, Text);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .text(Self::track_id)
            .int(Self::pos)
            .key((Self::playlist_id, Self::track_id))
            .unique((Self::playlist_id, Self::pos))
    }
}
#[allow(non_upper_case_globals)]
impl Item {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const track_id: Col<Self, Text> = col("track_id");
    pub const pos: Col<Self, Int> = col("pos");
}

pub struct CreatePlaylist {
    pub name: Text,
}
impl Input for CreatePlaylist {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name"))
    }
}

pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
}
impl Input for AddToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("track_id", text().min(1))
    }
}

fn module() -> Module {
    let demo = router::<Demo>("demo");
    Module::new((demo.routes((
        demo.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
            db.playlist
                .insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    user_id: ctx.user,
                })
                .on((Playlist::user_id, Playlist::name))
        }),
        demo.input::<AddToPlaylist>().mutation("add_to_playlist", |_ctx, db, input| {
            let last = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
            db.item.insert(Item {
                playlist_id: input.playlist_id,
                track_id: input.track_id,
                pos: last.map_or(0, |row| row.pos).add(1),
            })
        }),
    )),))
}

struct Fixture {
    schema: Schema,
    bodies: BTreeMap<FnHash, Closure>,
    procs: Vec<(FnHash, Procedure)>,
    create: FnHash,
    add: FnHash,
}

fn fixture() -> Fixture {
    let m = module();
    let procs = m.procedures();
    let hash = |n: &str| procs.iter().find(|(_, p)| p.name() == n).unwrap().0.clone();
    Fixture {
        schema: m.build().schema.clone(),
        bodies: closures(m.build()),
        create: hash("create_playlist"),
        add: hash("add_to_playlist"),
        procs,
    }
}

fn idv(n: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[8..].copy_from_slice(&n.to_be_bytes());
    b
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// The playlist a numbered one's id is.
fn pl(j: u64) -> Value {
    Value::Id(idv(1_000_000 + j))
}

impl Fixture {
    fn replica(&self, native: bool) -> Replica {
        let mut r = Replica::open(
            self.schema.clone(),
            self.bodies.clone(),
            MemoryStore::empty(self.schema.clone()),
            0,
            vec![],
        );
        if native {
            r.hold(self.procs.clone());
        }
        r
    }

    fn authority(&self) -> Authority {
        let mut a = Authority::new(self.schema.clone(), self.bodies.clone());
        a.hold(self.procs.clone());
        a
    }

    fn create(&self, r: &mut Replica, ctx: &Ctx, n: u64, j: u64) -> Entry {
        r.mutate(
            idv(n),
            ctx,
            &self.create,
            &args([("id", pl(j))]),
            &args([("name", Value::text(format!("P{j}")))]),
        )
        .unwrap()
    }

    fn add(&self, r: &mut Replica, ctx: &Ctx, n: u64, j: u64, track: u64) -> Entry {
        let a = args([("playlist_id", pl(j)), ("track_id", Value::text(format!("t{track}")))]);
        r.mutate(idv(n), ctx, &self.add, &Args::new(), &a).unwrap()
    }

    /// A log of `n` entries sequenced by an authority: `playlists`
    /// playlists, then `n` adds spread over them in turn.
    fn log(&self, playlists: u64, n: u64) -> (Authority, Vec<(Seq, Entry, Facts)>) {
        let ctx = Ctx::new("alice", "dev");
        let mut author = self.replica(true);
        let mut a = self.authority();
        let mut out = vec![];
        let mut push = |a: &mut Authority, e: Entry| match a.sequence_entry(&e) {
            Sequenced::Appended(s, f) => out.push((s, e, f)),
            o => panic!("{o:?}"),
        };
        for j in 0..playlists {
            let e = self.create(&mut author, &ctx, j, j);
            push(&mut a, e);
            local_commit_none(&mut author);
        }
        for i in 0..n {
            let e = self.add(&mut author, &ctx, 1_000 + i, i % playlists, i);
            push(&mut a, e);
            local_commit_none(&mut author);
        }
        (a, out)
    }
}

/// An author that is not the log's replica: it drops what it authored once
/// it is pushed, so its view stays one store (no rebase to pay for).
fn local_commit_none(r: &mut Replica) {
    r.pending.clear();
}

// Reporting --------------------------------------------------------------------

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

fn mean(xs: &[Duration]) -> Duration {
    if xs.is_empty() {
        return Duration::ZERO;
    }
    xs.iter().sum::<Duration>() / xs.len() as u32
}

fn header(title: &str) {
    eprintln!("\n== {title}");
    eprintln!(
        "{:<44} {:>7} {:>10} {:>10} {:>10} {:>10} {:>7}",
        "path", "n", "total", "mean µs", "first100", "last100", "l/f"
    );
}

/// One line: total, mean, the first and last hundred's means, and their
/// ratio — the growth within one run.
fn line(label: &str, per: &[Duration]) {
    let n = per.len();
    let k = n.min(100);
    let (f, l) = (mean(&per[..k]), mean(&per[n - k..]));
    eprintln!(
        "{:<44} {:>7} {:>9.1}ms {:>10.2} {:>10.2} {:>10.2} {:>7.2}",
        label,
        n,
        per.iter().sum::<Duration>().as_secs_f64() * 1e3,
        us(mean(per)),
        us(f),
        us(l),
        us(l) / us(f).max(1e-9)
    );
}

/// One line for something timed once.
fn once(label: &str, n: usize, d: Duration, note: &str) {
    eprintln!(
        "{:<44} {:>7} {:>9.1}ms {:>10.2} {}",
        label,
        n,
        d.as_secs_f64() * 1e3,
        us(d) / n.max(1) as f64,
        note
    );
}

const SIZES: [u64; 3] = [500, 2000, 8000];

// (a) A peer alone: the engine -------------------------------------------------

/// `Replica::mutate` then `local_commit`, as `ark_client::Peer::mutate`
/// does alone, with nothing written. Split: the optimistic apply, which is
/// the intent's one run, and the commit, which since R2 of
/// `docs/plan-perf.md` appends and confirms by that run's record rather
/// than running it twice more.
#[test]
#[ignore]
fn perf_a_mutate_alone() {
    let d = fixture();
    let me = Ctx::new("me", "local");
    header("(a) Replica::mutate + local_commit (a peer alone, no storage)");
    for &n in &SIZES {
        for (shape, playlists) in [("one playlist", 1u64), ("playlists of 10", (n / 10).max(1))] {
            let (mut a, mut r) = (d.authority(), d.replica(true));
            for j in 0..playlists {
                d.create(&mut r, &me, j, j);
                local_commit(&mut a, &mut r);
            }
            let (mut total, mut optimistic, mut commit) = (vec![], vec![], vec![]);
            for i in 0..n {
                let t = Instant::now();
                d.add(&mut r, &me, 1_000_000 + i, i % playlists, i);
                let t1 = Instant::now();
                local_commit(&mut a, &mut r);
                let t2 = Instant::now();
                optimistic.push(t1 - t);
                commit.push(t2 - t1);
                total.push(t2 - t);
            }
            assert!(r.pending.is_empty());
            line(&format!("add_to_playlist, {shape}: whole"), &total);
            line("  … Replica::mutate (optimistic apply)", &optimistic);
            line("  … local_commit (authority + confirm)", &commit);
        }
    }
    // Server mode, offline: every intent stays pending. What the engine
    // itself does per intent (the pending list only grows).
    header("(a) Replica::mutate offline (pending grows, nothing confirmed)");
    for &n in &SIZES {
        let mut r = d.replica(true);
        d.create(&mut r, &me, 0, 0);
        let per: Vec<Duration> = (0..n)
            .map(|i| {
                let t = Instant::now();
                d.add(&mut r, &me, 1_000_000 + i, 0, i);
                t.elapsed()
            })
            .collect();
        line("add_to_playlist, one playlist, all pending", &per);
    }
}

// (d) Initial sync --------------------------------------------------------------

/// A fresh replica receiving a log of N entries a page at a time, as a
/// `Batch` delivers it: by intent (`Mode::Whole`: no facts, the closure
/// held), with facts and the closure (replayed and compared), and by facts
/// alone (a replica that holds no closure). Over ten playlists, so the
/// intent's `MAX(pos)` read is a tenth of the log.
#[test]
#[ignore]
fn perf_d_initial_sync() {
    let d = fixture();
    header("(d) initial sync: Replica::receive_batch, pages of BATCH_LIMIT");
    for &n in &SIZES {
        let (_, log) = d.log(10, n);
        for (how, native, facts, bodies) in [
            ("by intent (native)", true, false, true),
            ("by intent (interpreted)", false, false, true),
            ("intent + facts, compared", true, true, true),
            ("by facts alone", false, true, false),
        ] {
            let mut r = d.replica(native);
            if !bodies {
                r.bodies.clear();
            }
            let per: Vec<Duration> = log
                .chunks(BATCH_LIMIT)
                .map(|page| {
                    let items: Vec<_> = page.iter().map(|(s, e, f)| (*s, e.clone(), facts.then(|| f.clone()))).collect();
                    let t = Instant::now();
                    r.receive_batch(items);
                    t.elapsed() / page.len() as u32
                })
                .collect();
            assert_eq!(r.cursor, log.len() as Seq, "{how}");
            let total: Duration = per.iter().sum::<Duration>() * BATCH_LIMIT as u32;
            once(
                &format!("receive {how}"),
                log.len(),
                total,
                &format!(
                    "first page {:.2} µs/entry, last page {:.2} µs/entry",
                    us(per[0]),
                    us(*per.last().unwrap())
                ),
            );
        }
    }
    // The server's half: every page a connection at 0 is sent, built by
    // `Server::fanout` through `Log::entries_after`, which clones every
    // entry above the cursor before it takes a page of them.
    header("(d) initial sync: the server paging a connection from 0 (Hello, Batch, Hello …)");
    for &n in &SIZES {
        let (a, log) = d.log(10, n);
        let mut s = Server::open(trusting(), open_access(), Silent, a);
        let c: ConnId = 1;
        let mut since = 0;
        let mut pages = 0;
        let mut first = Duration::ZERO;
        let mut last;
        let t0 = Instant::now();
        loop {
            let t = Instant::now();
            s.recv(c, hello(since, Mode::Whole));
            let out = s.take_outgoing();
            let dt = t.elapsed();
            if pages == 0 {
                first = dt;
            }
            last = dt;
            let mut more = false;
            for (_, m) in out {
                if let ServerMsg::Batch { items, has_more, .. } = m {
                    since = items.last().map_or(since, |(s, _, _)| *s);
                    more = has_more;
                }
            }
            pages += 1;
            if !more {
                break;
            }
        }
        assert_eq!(since, log.len() as Seq);
        once(
            "Server: page a fresh connection to the head",
            log.len(),
            t0.elapsed(),
            &format!("{pages} pages; first {:.0} µs, last {:.0} µs", us(first), us(last)),
        );
    }
    header("(d) Log::entries_after(cursor, BATCH_LIMIT): one page");
    for &n in &SIZES {
        let (a, _) = d.log(10, n);
        for cursor in [0, n as Seq / 2, n as Seq] {
            let t = Instant::now();
            let reps = 20;
            for _ in 0..reps {
                std::hint::black_box(a.page(cursor, BATCH_LIMIT));
            }
            once(&format!("entries_after({cursor}) of {n}"), 1, t.elapsed() / reps, "");
        }
    }
}

fn hello(since: Seq, mode: Mode) -> ClientMsg {
    ClientMsg::Hello {
        sub: Subscription { since, mode, log_id: None },
        token: Some("alice".into()),
        spec: ark::ir::SPEC_VERSION,
    }
}

// (e) The rebase -----------------------------------------------------------------

/// K intents of this peer's pending while M entries of another peer's land
/// over a confirmed store of S rows: once as one page (one rebase), and one
/// entry at a time as a live server's fan-out delivers them — a pump each
/// (a rebase each), and all in one pump, placed singly and settled once
/// (one rebase, `docs/plan-perf.md` R8). A rebase undoes the pending
/// intents' recorded changes, applies what landed and runs them again
/// (R2); the clone line is what each rebase copied before that.
#[test]
#[ignore]
fn perf_e_rebase() {
    let d = fixture();
    let me = Ctx::new("me", "s");
    header("(e) rebase: K pending, M landing, over S confirmed rows");
    eprintln!("{:<44} {:>7} {:>10} {:>10}", "shape", "S", "per entry", "per page");
    for &s in &SIZES {
        let (a0, base) = d.log(10, s);
        for k in [10u64, 100] {
            let m = 256u64;
            // The other peer's next M entries, sequenced after the base.
            let mut a = a0.clone();
            let mut other = d.replica(true);
            for (n, e, f) in &base {
                other.receive_with(*n, e.clone(), f.clone());
            }
            other.settle();
            let them = Ctx::new("alice", "dev");
            let landing: Vec<(Seq, Entry, Facts)> = (0..m)
                .map(|i| {
                    let e = d.add(&mut other, &them, 5_000_000 + i, i % 10, 900_000 + i);
                    match a.sequence_entry(&e) {
                        Sequenced::Appended(n, f) => (n, e, f),
                        o => panic!("{o:?}"),
                    }
                })
                .collect();
            let mine = |r: &mut Replica| {
                for i in 0..k {
                    d.add(r, &me, 7_000_000 + i, 3, 800_000 + i);
                }
            };
            let caught_up = || {
                let mut r = d.replica(true);
                r.receive_batch(base.iter().map(|(n, e, f)| (*n, e.clone(), Some(f.clone()))));
                r
            };
            // One page.
            let mut r = caught_up();
            mine(&mut r);
            let t = Instant::now();
            r.receive_batch(landing.iter().map(|(n, e, f)| (*n, e.clone(), Some(f.clone()))));
            let page = t.elapsed();
            assert_eq!(r.pending.len(), k as usize);
            // One at a time, a pump each: a settle per entry, which is
            // what a live push costs when it is the only frame of its pump.
            let mut r = caught_up();
            mine(&mut r);
            let t = Instant::now();
            for (n, e, f) in &landing {
                r.receive_with(*n, e.clone(), f.clone());
                r.settle();
            }
            let each = t.elapsed();
            assert_eq!(r.pending.len(), k as usize);
            // One at a time, one pump: every entry placed as its own
            // frame, then the one settle a pump makes (R8).
            let mut r = caught_up();
            mine(&mut r);
            let t = Instant::now();
            for (n, e, f) in &landing {
                r.receive_with(*n, e.clone(), f.clone());
            }
            r.settle();
            let pumped = t.elapsed();
            assert_eq!(r.pending.len(), k as usize);
            eprintln!(
                "{:<44} {:>7} {:>8.1}µs {:>8.1}ms",
                format!("K={k} M={m}: one entry at a time, a pump each"),
                s,
                us(each) / m as f64,
                each.as_secs_f64() * 1e3
            );
            eprintln!(
                "{:<44} {:>7} {:>8.1}µs {:>8.1}ms",
                format!("K={k} M={m}: one entry at a time, one pump"),
                s,
                us(pumped) / m as f64,
                pumped.as_secs_f64() * 1e3
            );
            eprintln!(
                "{:<44} {:>7} {:>8.1}µs {:>8.1}ms",
                format!("K={k} M={m}: one page"),
                s,
                us(page) / m as f64,
                page.as_secs_f64() * 1e3
            );
        }
        // What a replay's copy is, alone.
        let r = {
            let mut r = d.replica(true);
            r.receive_batch(base.iter().map(|(n, e, f)| (*n, e.clone(), Some(f.clone()))));
            r
        };
        let t = Instant::now();
        let reps = 20;
        for _ in 0..reps {
            std::hint::black_box(r.confirmed.clone());
        }
        eprintln!(
            "{:<44} {:>7} {:>8.1}µs",
            "  … MemoryStore::clone of the confirmed store",
            s,
            us(t.elapsed()) / reps as f64
        );
        // Confirming K pending one at a time: each ack walks pending.
        for k in [100u64, 1000] {
            let mut a = a0.clone();
            let mut r = caught_up_from(&d, &base);
            let es: Vec<Entry> = (0..k).map(|i| d.add(&mut r, &me, 9_000_000 + i, 4, 700_000 + i)).collect();
            let seqs: Vec<(Seq, Facts)> = es
                .iter()
                .map(|e| match a.sequence_entry(e) {
                    Sequenced::Appended(n, f) => (n, f),
                    o => panic!("{o:?}"),
                })
                .collect();
            let t = Instant::now();
            for (e, (n, _)) in es.iter().zip(&seqs) {
                r.ack(&e.id, *n);
                r.settle();
            }
            let dt = t.elapsed();
            assert!(r.pending.is_empty());
            eprintln!(
                "{:<44} {:>7} {:>8.1}µs {:>8.1}ms",
                format!("ack K={k} own pending, one at a time"),
                s,
                us(dt) / k as f64,
                dt.as_secs_f64() * 1e3
            );
        }
    }
}

fn caught_up_from(d: &Fixture, base: &[(Seq, Entry, Facts)]) -> Replica {
    let mut r = d.replica(true);
    r.receive_batch(base.iter().map(|(n, e, f)| (*n, e.clone(), Some(f.clone()))));
    r
}

// (g) The authority and the fan-out -------------------------------------------------

/// `sequence_entry` alone over a growing log (the intent applied to the
/// head state), then a server with C connections caught up, one pushing a
/// single entry: what the fan-out builds per connection, and what
/// encoding each message costs — the hub encodes every connection's own.
#[test]
#[ignore]
fn perf_g_authority_and_fanout() {
    let d = fixture();
    header("(g) Authority::sequence_entry");
    for &n in &SIZES {
        for (shape, playlists) in [("one playlist", 1u64), ("playlists of 10", (n / 10).max(1))] {
            let ctx = Ctx::new("alice", "dev");
            let mut author = d.replica(true);
            let mut es = vec![];
            for j in 0..playlists {
                es.push(d.create(&mut author, &ctx, j, j));
            }
            for i in 0..n {
                es.push(d.add(&mut author, &ctx, 1_000_000 + i, i % playlists, i));
            }
            let mut a = d.authority();
            let mut per = vec![];
            for (i, e) in es.iter().enumerate() {
                let t = Instant::now();
                let r = a.sequence_entry(e);
                if i as u64 >= playlists {
                    per.push(t.elapsed());
                }
                assert!(matches!(r, Sequenced::Appended(..)));
            }
            line(&format!("sequence add_to_playlist, {shape}"), &per);
        }
    }
    header("(g) fan-out of one pushed entry to C connections (Server::recv + encode)");
    eprintln!("{:<44} {:>7} {:>10} {:>10} {:>10}", "shape", "C", "recv µs", "encode µs", "per conn");
    for c in [10i64, 40, 160] {
        for s in [500u64, 8000] {
            let (a, log) = d.log(10, s);
            let mut srv = Server::open(trusting(), open_access(), Silent, a);
            for conn in 0..c {
                srv.recv(conn, hello(log.len() as Seq, Mode::Whole));
            }
            let _ = srv.take_outgoing();
            let ctx = Ctx::new("alice", "dev");
            let mut author = d.replica(true);
            for (n, e, f) in &log {
                author.receive_with(*n, e.clone(), f.clone());
            }
            author.settle();
            let reps = 20u64;
            let (mut recv, mut enc) = (Duration::ZERO, Duration::ZERO);
            let mut bytes = 0;
            for i in 0..reps {
                let e = d.add(&mut author, &ctx, 3_000_000 + i, i % 10, 600_000 + i);
                author.pending.clear();
                let t = Instant::now();
                srv.recv(0, ClientMsg::Push { entries: vec![e] });
                let out = srv.take_outgoing();
                let t1 = Instant::now();
                for (_, m) in &out {
                    bytes += std::hint::black_box(canon::encode(&m.to_value())).len();
                }
                enc += t1.elapsed();
                recv += t1 - t;
            }
            let (recv, enc) = (recv / reps as u32, enc / reps as u32);
            eprintln!(
                "{:<44} {:>7} {:>10.1} {:>10.1} {:>10.2}",
                format!("log of {s}; {} B per push", bytes / reps as usize),
                c,
                us(recv),
                us(enc),
                (us(recv) + us(enc)) / c as f64
            );
        }
    }
    header("(g) Log::state_at(head) — what a Verify costs the server");
    for &n in &SIZES {
        let (a, _) = d.log(10, n);
        let t = Instant::now();
        let st = a.log.state_at(a.log.head_seq()).unwrap();
        let replay = t.elapsed();
        let t = Instant::now();
        let h = state_hash(&st);
        once(
            &format!("state_at + state_hash, log of {n}"),
            1,
            replay + t.elapsed(),
            &format!("state_at {:.0} µs, hash {:.0} µs ({} B)", us(replay), us(t.elapsed()), h.len()),
        );
    }
    // What the server does with a Verify at the head since R4: hash the
    // authority's store as it stands, replaying nothing.
    header("(g) Server::recv(Verify at the head) — the answer, as served");
    for &n in &SIZES {
        let (a, _) = d.log(10, n);
        let (seq, hash) = (a.log.head_seq(), state_hash(&a.store));
        let mut s = Server::open(trusting(), open_access(), Silent, a);
        s.recv(1, hello(seq, Mode::Whole));
        let _ = s.take_outgoing();
        let t = Instant::now();
        s.recv(1, ClientMsg::Verify { seq, hash });
        let dt = t.elapsed();
        let agreed = s.take_outgoing().into_iter().any(|(_, m)| matches!(m, ServerMsg::Agree { ok: true, .. }));
        assert!(agreed, "the authority agrees with its own head");
        once(&format!("Verify at the head, log of {n}"), 1, dt, "");
    }
}

// (h) The wire ---------------------------------------------------------------------

/// A page of entries as a `Batch` frame: encoded and decoded, with facts
/// and without.
#[test]
#[ignore]
fn perf_h_frames() {
    let d = fixture();
    header("(h) a Batch of BATCH_LIMIT entries: canon::encode(to_value) and decode");
    eprintln!("{:<44} {:>7} {:>10} {:>10} {:>10}", "frame", "entries", "bytes", "encode µs", "decode µs");
    let (_, log) = d.log(10, BATCH_LIMIT as u64);
    for facts in [false, true] {
        let items: Vec<_> = log
            .iter()
            .rev()
            .take(BATCH_LIMIT)
            .rev()
            .map(|(n, e, f)| (*n, e.clone(), facts.then(|| f.clone())))
            .collect();
        let m = ServerMsg::Batch {
            items,
            has_more: false,
            log_id: None,
        };
        let reps = 50;
        let t = Instant::now();
        let mut bytes = vec![];
        for _ in 0..reps {
            bytes = std::hint::black_box(canon::encode(&m.to_value()));
        }
        let enc = t.elapsed() / reps;
        let t = Instant::now();
        for _ in 0..reps {
            let v = canon::decode(&bytes).unwrap();
            std::hint::black_box(ServerMsg::from_value(&v).unwrap());
        }
        let dec = t.elapsed() / reps;
        eprintln!(
            "{:<44} {:>7} {:>10} {:>10.1} {:>10.1}",
            if facts {
                "Batch, with facts (ByFacts)"
            } else {
                "Batch, intents only (Whole)"
            },
            BATCH_LIMIT,
            bytes.len(),
            us(enc),
            us(dec)
        );
    }
    // A client's Push of one entry, which is every mutation linked.
    let e = log[0].1.clone();
    let m = ClientMsg::Push { entries: vec![e] };
    let reps = 2000;
    let t = Instant::now();
    let mut bytes = vec![];
    for _ in 0..reps {
        bytes = std::hint::black_box(canon::encode(&m.to_value()));
    }
    let enc = t.elapsed() / reps;
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(ClientMsg::from_value(&canon::decode(&bytes).unwrap()).unwrap());
    }
    eprintln!(
        "{:<44} {:>7} {:>10} {:>10.2} {:>10.2}",
        "Push of one entry",
        1,
        bytes.len(),
        us(enc),
        us(t.elapsed() / reps)
    );
    let _ = d.schema.tables().count();
}

/// Every table's rows copied out of a store, as `scan` does, against the
/// store's own size: what a `Row` costs to hand out, keys and all.
#[test]
#[ignore]
fn perf_rows_handed_out() {
    let d = fixture();
    header("rows handed out: MemoryStore::scan(\"item\")");
    for &n in &SIZES {
        let (a, _) = d.log(10, n);
        let reps = 20;
        let t = Instant::now();
        let mut rows = 0;
        for _ in 0..reps {
            rows = std::hint::black_box(a.store.scan("item")).len();
        }
        once(&format!("scan of {rows} rows"), rows, t.elapsed() / reps, "(per row)");
    }
}
