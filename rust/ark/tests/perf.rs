//! What the v5 machine's paths cost as the data grows, printed rather than
//! asserted: `rust/ark/tests/perf.rs`'s shapes (`docs/plan-v5.md` Part 3),
//! the same sizes four and sixteen times apart, the same columns — the
//! mean, the first hundred, the last hundred and their ratio — and the same
//! labels, so that the two outputs diff line for line. Where v5 has no
//! counterpart to a v4 line the line says `n/a` and why; where v5 does
//! something v4 split, or splits something v4 did at once, a note under the
//! header says what is in the number.
//!
//! Over the demo of `tests/demo.rs` (the v4 demo's tables and its
//! `create_playlist` and `add_to_playlist`, in plain Rust), driven through [`Peer::step`] with frames as bytes:
//! every number includes the frame's encoding or decoding, which v4's
//! engine-level calls did not.
//!
//! Beside v4's shapes, the views shape Part 3 adds: one change against
//! `playlist-with-items`, `top-two-by-pos` and `group-source` at 1k, 4k and
//! 16k rows, timed, with the entries the push read (`ark::db::count`), the
//! views typed plans (§1.16);
//! `rust/ark/tests/perf_views.rs` is the same over v4.
//!
//! Ignored by default; run with
//! `cargo test -p ark --release --test perf -- --ignored --nocapture --test-threads=1`.

#[path = "demo.rs"]
mod demo;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::{Duration, Instant};

use std::sync::Arc;

use ark::frames::{Frame, Verdict};
use ark::log::{Entry, EntryId, Logged, Origin, Seq};
use ark::peer::{Ctx, Feed, Functions, Input, Journal, Output, Peer, Role, Who, PAGE};
use ark::prelude::*;
use ark::schema::{Schema, Space};
use ark::store::{Change, Read, Scan, Store};
use demo::*;

// Counting allocations, on this thread ------------------------------------------
//
// v4's counter, as it is: per thread, a thread-local add.

struct Tally;

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
    static LIVE: Cell<isize> = const { Cell::new(0) };
}

fn count() {
    let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
}

fn live(delta: isize) {
    let _ = LIVE.try_with(|c| c.set(c.get() + delta));
}

unsafe impl GlobalAlloc for Tally {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count();
        live(l.size() as isize);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        live(-(l.size() as isize));
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count();
        live(n as isize - l.size() as isize);
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Tally = Tally;

fn allocs() -> usize {
    ALLOCS.with(Cell::get)
}

fn live_bytes() -> isize {
    LIVE.with(Cell::get)
}

// The demo, driven -----------------------------------------------------------------

/// What an entry calls a demo function by: its name.
fn hash(name: &str) -> String {
    name.to_string()
}

fn who(actor: &str, session: &str) -> Who {
    Who {
        actor: actor.into(),
        session: session.into(),
        roles: vec![],
    }
}

fn authority_with(cas: Functions) -> Peer {
    Peer::new(shared(), cas, Role::Authority { journal: Journal::default() }, who("server", "server"))
}

fn authority() -> Peer {
    authority_with(demo::cas())
}

fn replica(actor: &str, session: &str) -> Peer {
    Peer::new(shared(), demo::cas(), Role::Replica, who(actor, session))
}

/// An author that is not a peer: entries made as a session makes them
/// (`id` from `(session, local)`), for an authority to sequence.
#[derive(Clone)]
struct Author {
    who: Who,
    local: i64,
}

impl Author {
    fn new(actor: &str, session: &str) -> Author {
        Author {
            who: who(actor, session),
            local: 0,
        }
    }

    fn entry(&mut self, name: &str, input: Vec<u8>) -> Entry {
        self.local += 1;
        Entry {
            id: EntryId::of(Origin::named(&self.who.session), self.local),
            actor: self.who.actor.clone(),
            session: self.who.session.clone(),
            origin: Origin::named(&self.who.session),
            local: self.local,
            roles: vec![],
            func: hash(name),
            input,
            now: self.local,
        }
    }

    /// The push of the next entry, as a connection verified as this
    /// author delivers it.
    fn push(&mut self, name: &str, input: Vec<u8>) -> Input {
        let entry = self.entry(name, input);
        up(&self.who, Frame::push(Space::Durable, [entry]))
    }

    /// The playlist its `local`-th entry created.
    fn pid(&self, local: i64) -> [u8; 16] {
        EntryId::of(Origin::named(&self.who.session), local).draw(0)
    }
}

fn up(w: &Who, f: Frame) -> Input {
    Input::Frame {
        from: Some(w.clone()),
        bytes: f.enc(),
    }
}

fn down(bytes: Vec<u8>) -> Input {
    Input::Frame { from: None, bytes }
}

fn batch(logged: &[Logged]) -> Vec<u8> {
    Frame::Page {
        space: Space::Durable,
        from: logged[0].seq - 1,
        logged: logged.to_vec(),
    }
    .enc()
}

/// A log an authority sequenced: `playlists` playlists, then `n` adds
/// spread over them in turn, the `i`-th track named `track(i)`.
struct Built {
    a: Peer,
    author: Author,
    pids: Vec<[u8; 16]>,
}

impl Built {
    fn logged(&self) -> Vec<Logged> {
        self.a.log().entries()
    }
}

fn log_named(mut a: Peer, playlists: u64, n: u64, track: impl Fn(u64) -> String) -> Built {
    let mut author = Author::new("alice", "alice-dev");
    let mut pids = vec![];
    for j in 0..playlists {
        let p = author.push(CREATE, create_input(&format!("P{j}")));
        a.step(p);
        pids.push(author.pid(author.local));
    }
    for i in 0..n {
        let p = author.push(ADD, pair_input(&pids[(i % playlists) as usize], &track(i)));
        a.step(p);
    }
    assert_eq!(a.log().head().0, (playlists + n) as Seq, "every entry sequenced");
    Built { a, author, pids }
}

fn log(playlists: u64, n: u64) -> Built {
    log_named(authority(), playlists, n, |i| format!("t{i}"))
}

/// Every frame a step sent upstream, to the authority as `w`; and what it
/// said back, sent and fanned out, to the replica.
fn commit(a: &mut Peer, r: &mut Peer, outs: Vec<Output>) {
    let w = r.me.clone();
    for o in outs {
        if let Output::Send(b) = o {
            for back in a.step(Input::Frame {
                from: Some(w.clone()),
                bytes: b,
            }) {
                if let Output::Send(b) | Output::Fanout(b) = back {
                    r.step(down(b));
                }
            }
        }
    }
}

// Reporting --------------------------------------------------------------------------

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

fn note(s: &str) {
    eprintln!("   v5: {s}");
}

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

/// A v4 line v5 has nothing to measure for, and why.
fn na(label: &str, n: usize, why: &str) {
    eprintln!("{:<44} {:>7} {:>10} n/a: {why}", label, n, "");
}

const SIZES: [u64; 3] = [500, 2000, 8000];

// (a) A peer alone ---------------------------------------------------------------------

/// v4's `Replica::mutate` + `local_commit` is, in v5, a replica's
/// `step(Mutate)` (run, apply, the `Push` encoded) and then the authority's
/// `step(Push)` with the replica's `step` of the `Batch` and `Ack` it
/// answers (sequence, encode, decode, confirm by facts). Beside them what
/// a v5 peer alone is: one machine in the authority's role, whose
/// `step(Mutate)` runs and logs the intent once.
#[test]
#[ignore]
fn perf_a_mutate_alone() {
    header("(a) Replica::mutate + local_commit (a peer alone, no storage)");
    note("whole = replica step(Mutate) + authority step(Push) + replica step(Batch, Ack), frames as bytes;");
    note("`alone` = one Peer in the authority's role (Input::Alone), step(Mutate): v5's peer alone.");
    for &n in &SIZES {
        for (shape, playlists) in [("one playlist", 1u64), ("playlists of 10", (n / 10).max(1))] {
            let (mut a, mut r) = (authority(), replica("me", "me-local"));
            for j in 0..playlists {
                let o = r.step(Input::Mutate {
                    func: hash(CREATE),
                    input: create_input(&format!("P{j}")),
                    now: 0,
                });
                commit(&mut a, &mut r, o);
            }
            assert!(r.pending.is_empty());
            let pid = |j: u64| EntryId::of(Origin::named("me-local"), j as i64 + 1).draw(0);
            let (mut total, mut optimistic, mut confirm) = (vec![], vec![], vec![]);
            for i in 0..n {
                let input = pair_input(&pid(i % playlists), &format!("t{i}"));
                let t = Instant::now();
                let o = r.step(Input::Mutate {
                    func: hash(ADD),
                    input,
                    now: i as i64,
                });
                let t1 = Instant::now();
                commit(&mut a, &mut r, o);
                let t2 = Instant::now();
                optimistic.push(t1 - t);
                confirm.push(t2 - t1);
                total.push(t2 - t);
            }
            assert!(r.pending.is_empty());
            assert_eq!(r.log().head(), a.log().head());
            line(&format!("add_to_playlist, {shape}: whole"), &total);
            line("  … Replica::mutate (optimistic apply)", &optimistic);
            line("  … local_commit (authority + confirm)", &confirm);
            // The peer alone.
            let mut p = replica("me", "me-local");
            p.step(Input::Alone);
            for j in 0..playlists {
                p.step(Input::Mutate {
                    func: hash(CREATE),
                    input: create_input(&format!("P{j}")),
                    now: 0,
                });
            }
            let per: Vec<Duration> = (0..n)
                .map(|i| {
                    let input = pair_input(&pid(i % playlists), &format!("t{i}"));
                    let t = Instant::now();
                    let o = p.step(Input::Mutate {
                        func: hash(ADD),
                        input,
                        now: i as i64,
                    });
                    let dt = t.elapsed();
                    assert!(matches!(o.last(), Some(Output::Confirmed(_))));
                    dt
                })
                .collect();
            line("  … alone: Peer (authority) step(Mutate)", &per);
        }
    }
    header("(a) Replica::mutate offline (pending grows, nothing confirmed)");
    for &n in &SIZES {
        let mut r = replica("me", "me-local");
        r.step(Input::Mutate {
            func: hash(CREATE),
            input: create_input("P0"),
            now: 0,
        });
        let pid = EntryId::of(Origin::named("me-local"), 1).draw(0);
        let per: Vec<Duration> = (0..n)
            .map(|i| {
                let input = pair_input(&pid, &format!("t{i}"));
                let t = Instant::now();
                r.step(Input::Mutate {
                    func: hash(ADD),
                    input,
                    now: i as i64,
                });
                t.elapsed()
            })
            .collect();
        assert_eq!(r.pending.len() as u64, n + 1);
        line("add_to_playlist, one playlist, all pending", &per);
    }
}

// (d) Initial sync -------------------------------------------------------------------

/// A fresh replica receiving a log of N entries a page at a time, as the
/// `Batch` frames an authority serves it: v5 has one way, by facts, each
/// entry's state hash checked against the store's (§1.6) — so v4's three
/// ways by intent are `n/a`. The bytes are the frames'. Then the server's
/// half and one page of the log.
#[test]
#[ignore]
fn perf_d_initial_sync() {
    header("(d) initial sync: Replica::receive_batch, pages of BATCH_LIMIT");
    note(&format!(
        "Replica.step(Batch frame of {PAGE}): decode, apply facts, check each entry's state hash, link the chain"
    ));
    for &n in &SIZES {
        let b = log(10, n);
        let logged = b.logged();
        let pages: Vec<Vec<u8>> = logged.chunks(PAGE).map(batch).collect();
        let bytes: usize = pages.iter().map(Vec::len).sum();
        na(
            "receive by intent (native)",
            logged.len(),
            "a confirmed entry is applied by its facts (§1.6); no code runs",
        );
        na("receive by intent (interpreted)", logged.len(), "as above");
        na(
            "receive intent + facts, compared",
            logged.len(),
            "as above; the facts are checked by the state hash, below",
        );
        let mut r = replica("bob", "bob-dev");
        let per: Vec<Duration> = pages
            .iter()
            .zip(logged.chunks(PAGE))
            .map(|(f, page)| {
                let t = Instant::now();
                r.step(down(f.clone()));
                t.elapsed() / page.len() as u32
            })
            .collect();
        assert_eq!(r.log().head(), b.a.log().head(), "caught up");
        let total: Duration = per.iter().sum::<Duration>() * PAGE as u32;
        once(
            "receive by facts alone",
            logged.len(),
            total,
            &format!(
                "first page {:.2} µs/entry, last page {:.2} µs/entry; {:.1} B/entry on the wire",
                us(per[0]),
                us(*per.last().unwrap()),
                bytes as f64 / logged.len() as f64
            ),
        );
    }
    header("(d) initial sync: the server paging a connection from 0 (Hello, Batch, Hello …)");
    note("one Hello at 0 is answered with every page at once (Peer::serve); there is no Hello per page");
    for &n in &SIZES {
        let mut b = log(10, n);
        let hello = up(
            &who("bob", "bob-dev"),
            Frame::Hello {
                space: Space::Durable,
                cursor: 0,
                hash: None,
                device: None,
                schema: Default::default(),
                version: String::new(),
            },
        );
        let t = Instant::now();
        let out = b.a.step(hello);
        let dt = t.elapsed();
        let frames: Vec<&Vec<u8>> = out
            .iter()
            .filter_map(|o| match o {
                Output::Send(b) => Some(b),
                _ => None,
            })
            .collect();
        let bytes: usize = frames.iter().map(|f| f.len()).sum();
        let n_entries = b.logged().len();
        once(
            "Server: page a fresh connection to the head",
            n_entries,
            dt,
            &format!(
                "{} pages in one step; {bytes} B, {:.1} B/entry",
                frames.len(),
                bytes as f64 / n_entries as f64
            ),
        );
    }
    header("(d) Log::entries_after(cursor, BATCH_LIMIT): one page");
    note("Log::after(cursor) is a slice; the page is its first PAGE entries cloned");
    for &n in &SIZES {
        let b = log(10, n);
        for cursor in [0, n as Seq / 2, n as Seq] {
            let reps = 20;
            let t = Instant::now();
            for _ in 0..reps {
                std::hint::black_box(b.a.log().after(cursor).take(PAGE).collect::<Vec<_>>());
            }
            once(&format!("entries_after({cursor}) of {n}"), 1, t.elapsed() / reps, "");
        }
    }
}

// (e) The rebase ------------------------------------------------------------------------

/// K intents of this peer's pending while M entries of another's land over
/// a confirmed store of S rows: as one page, and one entry at a time. A
/// rebase undoes the pending changes newest first, applies the facts and
/// runs the pending intents again (§1.7) — per confirmed entry, in a page
/// as one at a time, so v4's "one pump" (place singly, settle once) has no
/// counterpart. K goes to 400 (Part 3's depth). Then confirming K of this
/// peer's own, one `Batch` each: the first pending is the entry, its facts
/// the preview's, and nothing moves but the hash beside the rest.
#[test]
#[ignore]
fn perf_e_rebase() {
    header("(e) rebase: K pending, M landing, over S confirmed rows");
    eprintln!("{:<44} {:>7} {:>10} {:>10}", "shape", "S", "per entry", "per page");
    let m = 256u64;
    for &s in &SIZES {
        let base = log(10, s);
        // The other peer's next M entries, sequenced after the base.
        let mut landed = base.a.clone();
        let mut other = base.author.clone();
        for i in 0..m {
            let p = other.push(ADD, pair_input(&base.pids[(i % 10) as usize], &format!("t{}", 900_000 + i)));
            landed.step(p);
        }
        let landing: Vec<Logged> = landed.log().after(base.a.log().head().0).collect();
        assert_eq!(landing.len() as u64, m);
        let page = batch(&landing);
        let singles: Vec<Vec<u8>> = landing.iter().map(|l| batch(std::slice::from_ref(l))).collect();
        let mut caught_up = replica("me", "me-s");
        for f in base.logged().chunks(PAGE).map(batch) {
            caught_up.step(down(f));
        }
        assert_eq!(caught_up.log().head(), base.a.log().head());
        for k in [10u64, 100, 400] {
            let mine = |r: &mut Peer| {
                for i in 0..k {
                    r.step(Input::Mutate {
                        func: hash(ADD),
                        input: pair_input(&base.pids[3], &format!("t{}", 800_000 + i)),
                        now: 0,
                    });
                }
                assert_eq!(r.pending.len() as u64, k);
            };
            // One at a time, a frame each.
            let mut r = caught_up.clone();
            mine(&mut r);
            let t = Instant::now();
            for f in &singles {
                r.step(down(f.clone()));
            }
            let each = t.elapsed();
            assert_eq!((r.pending.len() as u64, r.log().head()), (k, landed.log().head()));
            // One page.
            let mut r = caught_up.clone();
            mine(&mut r);
            let t = Instant::now();
            r.step(down(page.clone()));
            let paged = t.elapsed();
            assert_eq!((r.pending.len() as u64, r.log().head()), (k, landed.log().head()));
            let row = |label: String, d: Duration| eprintln!("{:<44} {:>7} {:>8.1}µs {:>8.1}ms", label, s, us(d) / m as f64, d.as_secs_f64() * 1e3);
            row(format!("K={k} M={m}: one entry at a time, a pump each"), each);
            na(
                &format!("K={k} M={m}: one entry at a time, one pump"),
                s as usize,
                "every confirmed entry rebases as it is applied; no settle to defer",
            );
            row(format!("K={k} M={m}: one page"), paged);
        }
        // What a copy of the confirmed store is: v5's rebase makes none.
        let reps = 20;
        let t = Instant::now();
        for _ in 0..reps {
            std::hint::black_box(caught_up.store.clone());
        }
        eprintln!(
            "{:<44} {:>7} {:>8.1}µs",
            "  … MemoryStore::clone of the confirmed store",
            s,
            us(t.elapsed()) / reps as f64
        );
        // Confirming K of this peer's own, one at a time.
        for k in [100u64, 1000] {
            let mut a = base.a.clone();
            let mut r = caught_up.clone();
            let mut confirms = vec![];
            for i in 0..k {
                let o = r.step(Input::Mutate {
                    func: hash(ADD),
                    input: pair_input(&base.pids[4], &format!("t{}", 700_000 + i)),
                    now: 0,
                });
                for x in o {
                    if let Output::Send(b) = x {
                        for y in a.step(Input::Frame {
                            from: Some(r.me.clone()),
                            bytes: b,
                        }) {
                            if let Output::Fanout(b) = y {
                                confirms.push(b);
                            }
                        }
                    }
                }
            }
            let t = Instant::now();
            for f in confirms {
                r.step(down(f));
            }
            let dt = t.elapsed();
            assert!(r.pending.is_empty() && r.log().head() == a.log().head());
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

// (g) The authority and the fan-out ------------------------------------------------------

/// The authority's `step(Push)` over a growing log — judge, run, log, and
/// encode its `Ack` and the one `Batch` every connection fed whole is
/// sent. Then that push to C connections: v5 encodes the fan-out once, in
/// the step; what is left per connection is the hub copying it to each
/// (`Hub::step`), timed here as the "encode" column. `ark-server`'s
/// `tests/perf.rs` takes the same push through the real hub.
#[test]
#[ignore]
fn perf_g_authority_and_fanout() {
    header("(g) Authority::sequence_entry");
    note("Authority.step(Push frame): decode, judge, run, log, encode Ack and the fan-out Batch");
    for &n in &SIZES {
        for (shape, playlists) in [("one playlist", 1u64), ("playlists of 10", (n / 10).max(1))] {
            let mut author = Author::new("alice", "alice-dev");
            let mut creates = vec![];
            let mut pids = vec![];
            for j in 0..playlists {
                creates.push(author.push(CREATE, create_input(&format!("P{j}"))));
                pids.push(author.pid(author.local));
            }
            let adds: Vec<Input> = (0..n)
                .map(|i| author.push(ADD, pair_input(&pids[(i % playlists) as usize], &format!("t{i}"))))
                .collect();
            let mut a = authority();
            for c in creates {
                a.step(c);
            }
            let per: Vec<Duration> = adds
                .into_iter()
                .map(|p| {
                    let t = Instant::now();
                    let o = a.step(p);
                    let dt = t.elapsed();
                    assert!(o.iter().any(|x| matches!(x, Output::Fanout(_))));
                    dt
                })
                .collect();
            line(&format!("sequence add_to_playlist, {shape}"), &per);
        }
    }
    header("(g) fan-out of one pushed entry to C connections (Server::recv + encode)");
    note("recv = Authority.step(Push), the Batch encoded once inside it; encode = that frame copied to each of C connections");
    eprintln!("{:<44} {:>7} {:>10} {:>10} {:>10}", "shape", "C", "recv µs", "encode µs", "per conn");
    for c in [10usize, 40, 160] {
        for s in [500u64, 8000] {
            let mut b = log(10, s);
            let mut pusher = Author::new("alice", "alice-push");
            let reps = 20u64;
            let pushes: Vec<Input> = (0..reps)
                .map(|i| pusher.push(ADD, pair_input(&b.pids[(i % 10) as usize], &format!("t{}", 600_000 + i))))
                .collect();
            let (mut recv, mut enc) = (Duration::ZERO, Duration::ZERO);
            let mut bytes = 0;
            for p in pushes {
                let t = Instant::now();
                let out = b.a.step(p);
                let t1 = Instant::now();
                for o in &out {
                    match o {
                        Output::Fanout(f) => {
                            for _ in 0..c {
                                bytes += std::hint::black_box(f.clone()).len();
                            }
                        }
                        Output::Send(f) => bytes += f.len(),
                        _ => {}
                    }
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
        na(
            &format!("state_at + state_hash, log of {n}"),
            1,
            "no Verify frame: every entry carries its state hash and the chain's (§1.6)",
        );
    }
    header("(g) Server::recv(Verify at the head) — the answer, as served");
    note("its counterpart: a Hello at the head with the chain's hash there, answered with nothing");
    for &n in &SIZES {
        let mut b = log(10, n);
        let (cursor, hash) = b.a.log().head();
        let t = Instant::now();
        let out = b.a.step(up(
            &who("bob", "bob-dev"),
            Frame::Hello {
                space: Space::Durable,
                cursor,
                hash: Some(hash),
                device: None,
                schema: Default::default(),
                version: String::new(),
            },
        ));
        let dt = t.elapsed();
        assert!(out.is_empty(), "nothing to send a peer at the head");
        once(&format!("Verify at the head, log of {n}"), 1, dt, "(a Hello at the head)");
    }
}

// (i) Connections fed by a scope ----------------------------------------------------------

/// The scope of the partial shape (`demo.mine`): every playlist, and the
/// items whose track is the reader's — named `actor:…`. v4's scope held a
/// person's playlists and every item without its position; v5 has no
/// projection, and a union must hold the rows its rows reference, so the
/// playlists are everyone's.
/// The fan-out of one `add_to_playlist` to C connections fed by the scope:
/// half of them alice (the author, whose union holds every item, its
/// tracks being `alice:…`), half strangers (who hold the playlists and
/// none of the items). Each is sent its own `Batch` — the entry's facts
/// through the scope, applied to its union, the union's state hash and its
/// own chain — encoded inside the step (`Output::To`); the "encode"
/// column is the hub's copy of each. Beside it the same push to C
/// connections fed whole.
#[test]
#[ignore]
fn perf_i_partial_fanout() {
    let cas = demo::cas();
    let scope_h = "demo.mine";
    assert!(cas.get(scope_h).is_some(), "the scope is there");
    header("(i) fan-out of one pushed entry to C partial connections (Server::recv + encode)");
    note("recv = Authority.step(Push) including each scoped connection's filter, union, hash, chain and encode; encode = the copies routed");
    eprintln!(
        "{:<44} {:>7} {:>10} {:>10} {:>10} {:>12}",
        "shape", "C", "recv µs", "encode µs", "per conn", "per fact/conn"
    );
    for c in [10usize, 40, 160] {
        for s in [500u64, 8000] {
            for partial in [true, false] {
                let mut b = log_named(authority_with(cas.clone()), 10, s, |i| format!("alice:{i}"));
                let head = b.a.log().head();
                for conn in 0..c {
                    let actor = if conn % 2 == 0 { "alice".to_string() } else { format!("u{conn}") };
                    let w = who(&actor, &format!("c{conn}"));
                    if partial {
                        b.a.step(Input::Feed {
                            who: w.clone(),
                            feed: Feed::Scoped(scope_h.into()),
                        });
                        let o = b.a.step(up(
                            &w,
                            Frame::Hello {
                                space: Space::Durable,
                                cursor: head.0,
                                hash: Some(head.1),
                                device: None,
                                schema: Default::default(),
                                version: String::new(),
                            },
                        ));
                        assert!(matches!(o[..], [Output::To { .. }]), "a union is served: {o:?}");
                    }
                }
                let mut pusher = Author::new("alice", "c0");
                let reps = 20u64;
                let pushes: Vec<Input> = (0..reps)
                    .map(|i| pusher.push(ADD, pair_input(&b.pids[(i % 10) as usize], &format!("alice:{}", 600_000 + i))))
                    .collect();
                let (mut recv, mut enc) = (Duration::ZERO, Duration::ZERO);
                let (mut bytes, mut sent) = (0, vec![]);
                for p in pushes {
                    let t = Instant::now();
                    let out = b.a.step(p);
                    let t1 = Instant::now();
                    for o in &out {
                        match o {
                            Output::Fanout(f) if !partial => {
                                for _ in 0..c {
                                    bytes += std::hint::black_box(f.clone()).len();
                                }
                            }
                            Output::To { bytes: f, .. } => bytes += std::hint::black_box(f.clone()).len(),
                            Output::Send(f) => bytes += f.len(),
                            _ => {}
                        }
                    }
                    enc += t1.elapsed();
                    recv += t1 - t;
                    sent.extend(out);
                }
                let facts: usize = sent
                    .iter()
                    .filter_map(|o| match o {
                        Output::To { bytes, .. } => match Frame::dec(bytes) {
                            Ok(Frame::Page { logged, .. }) => Some(logged.iter().map(|l| l.facts.len()).sum::<usize>()),
                            _ => None,
                        },
                        _ => None,
                    })
                    .sum();
                if partial {
                    let tos = sent.iter().filter(|o| matches!(o, Output::To { .. })).count();
                    assert_eq!(tos, c * reps as usize, "every scoped connection is sent each entry");
                }
                let (recv, enc) = (recv / reps as u32, enc / reps as u32);
                let per_conn = (us(recv) + us(enc)) / c as f64;
                let facts_per_conn = facts as f64 / (reps as f64 * c as f64);
                eprintln!(
                    "{:<44} {:>7} {:>10.1} {:>10.1} {:>10.2} {:>12}",
                    format!(
                        "{} log of {s}; {} B per push",
                        if partial { "partial," } else { "whole," },
                        bytes / reps as usize
                    ),
                    c,
                    us(recv),
                    us(enc),
                    per_conn,
                    if partial && facts_per_conn > 0.0 {
                        format!("{:.2}", per_conn / facts_per_conn)
                    } else {
                        "-".into()
                    }
                );
            }
        }
    }
}

// (h) The wire --------------------------------------------------------------------------

/// A page of logged entries as a `Batch` frame, encoded and decoded, with
/// the allocations each makes (`#[derive(Serialize, Deserialize)]`'s codec over §1.1's
/// bytes); and a `Push` of one entry.
#[test]
#[ignore]
fn perf_h_frames() {
    header("(h) a Batch of BATCH_LIMIT entries: canon::encode(to_value) and decode");
    note("Frame::enc / Frame::dec; a v5 Batch always carries its facts (§1.6)");
    eprintln!(
        "{:<44} {:>7} {:>10} {:>10} {:>10} {:>12} {:>12}",
        "frame", "entries", "bytes", "encode µs", "decode µs", "enc. allocs", "dec. allocs"
    );
    let b = log(10, PAGE as u64);
    let logged = b.logged();
    let page = &logged[logged.len() - PAGE..];
    na("Batch, intents only (Whole)", PAGE, "a Batch always carries facts (§1.6)");
    let f = Frame::Page {
        space: Space::Durable,
        from: page[0].seq - 1,
        logged: page.to_vec(),
    };
    let reps = 50;
    let a0 = allocs();
    let t = Instant::now();
    let mut bytes = vec![];
    for _ in 0..reps {
        bytes = std::hint::black_box(f.enc());
    }
    let (enc, enc_a) = (t.elapsed() / reps, (allocs() - a0) / reps as usize);
    let a0 = allocs();
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(Frame::dec(&bytes).unwrap());
    }
    let (dec, dec_a) = (t.elapsed() / reps, (allocs() - a0) / reps as usize);
    eprintln!(
        "{:<44} {:>7} {:>10} {:>10.1} {:>10.1} {:>12} {:>12}",
        "Batch, with facts (ByFacts)",
        PAGE,
        bytes.len(),
        us(enc),
        us(dec),
        enc_a,
        dec_a
    );
    let p = Frame::push(Space::Durable, [page[0].entry.clone()]);
    let reps = 2000;
    let t = Instant::now();
    let mut bytes = vec![];
    for _ in 0..reps {
        bytes = std::hint::black_box(p.enc());
    }
    let enc = t.elapsed() / reps;
    let a0 = allocs();
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(Frame::dec(&bytes).unwrap());
    }
    let dec = t.elapsed() / reps;
    let dec_a = (allocs() - a0) / reps as usize;
    eprintln!(
        "{:<44} {:>7} {:>10} {:>10.2} {:>10.2} {:>12} {:>12}",
        "Push of one entry",
        1,
        bytes.len(),
        us(enc),
        us(dec),
        "",
        dec_a
    );
}

/// The item table's rows handed out of a store, as `Read::scan` does: each
/// row's bytes copied.
#[test]
#[ignore]
fn perf_rows_handed_out() {
    header("rows handed out: MemoryStore::scan(\"item\")");
    note("Store::scan(\"item\", Scan::all()): each row's bytes, copied");
    for &n in &SIZES {
        let b = log(10, n);
        let reps = 20;
        let t = Instant::now();
        let mut rows = 0;
        for _ in 0..reps {
            rows = std::hint::black_box(b.a.store.scan("item", &Scan::all())).len();
        }
        once(&format!("scan of {rows} rows"), rows, t.elapsed() / reps, "(per row)");
    }
}

/// v4 kept every id in its log and folded those below the horizon to 8
/// bytes. v5's log keeps no id map (dedupe is `(session, local)`, §1.6);
/// what an authority keeps per id is its journal's verdict, counted here as
/// the bytes this thread holds, with a lookup's time. Nothing folds them.
#[test]
#[ignore]
fn perf_ids_below_the_horizon() {
    eprintln!("\n== the ids a log keeps (docs/plan-db.md D6), per id and per million");
    eprintln!("   v5: Journal.verdicts (EntryId -> Verdict), what an authority keeps per id; no fold");
    for n in [100_000u64, 1_000_000] {
        let ids: Vec<EntryId> = (0..n).map(|i| EntryId::of("s", i as i64)).collect();
        let before = live_bytes();
        let mut j = Journal::default();
        for (i, id) in ids.iter().enumerate() {
            j.verdicts.insert(*id, Verdict::Ok { seq: i as Seq + 1 });
        }
        let whole = live_bytes() - before;
        let probes = 100_000.min(n as usize);
        let t = Instant::now();
        for id in ids.iter().take(probes) {
            assert!(j.verdicts.contains_key(id));
        }
        let lookup = t.elapsed() / probes as u32;
        eprintln!(
            "{n:>9} ids: verdicts {:.1} B/id; fold n/a (none); lookup {:.0} ns",
            whole as f64 / n as f64,
            lookup.as_nanos()
        );
    }
}

// Views: one change against a maintained view at 1k/4k/16k -------------------------------
//
// The scenarios of `tests/views.rs`'s work contract, timed: the library's
// schema, its playlists and items, and its songs.

/// A song, as the library's `song` table has it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(index(creator), index(movement_id))]
pub struct Song {
    #[ark(key)]
    pub id: String,
    pub title: String,
    pub creator: String,
    pub movement_id: Option<String>,
    pub pos: i64,
}

/// The playlists, their items, and songs.
#[derive(Schema)]
pub struct Lib {
    pub playlist: Playlist,
    pub item: Item,
    pub song: Song,
}

fn library() -> Schema {
    <Lib as Tables>::schema()
}

fn vpid(n: u32) -> Vec<u8> {
    [&[0; 12][..], &n.to_be_bytes()].concat()
}

fn pl(n: u32, name: &str, user: &str) -> Change {
    Change::Add {
        table: "playlist".into(),
        row: playlist_row(&vpid(n), name, user),
    }
}

fn it_row(n: u32, track: &str, pos: i64) -> Vec<u8> {
    item_row(&vpid(n), track, pos)
}

fn song_row(id: &str, creator: &str, pos: i64) -> Vec<u8> {
    Song {
        id: id.into(),
        title: "x".into(),
        creator: creator.into(),
        movement_id: None,
        pos,
    }
    .to_bytes()
}

fn add(table: &str, row: Vec<u8>) -> Change {
    Change::Add { table: table.into(), row }
}

fn store_of(rows: &[Change]) -> Store {
    let schema = library();
    let mut by: Vec<(String, Vec<Vec<u8>>)> = schema.tables.iter().map(|t| (t.name.clone(), vec![])).collect();
    for c in rows {
        let Change::Add { table, row } = c else { panic!("a store is rows") };
        by.iter_mut().find(|(t, _)| t == table).unwrap().1.push(row.clone());
    }
    Store::from_rows(schema.into(), Space::Durable, &by).unwrap().0
}

type Made<T> = ark::plan::Make<Lib, T>;

/// `top-two-by-pos(playlist_id)`: the playlist's items by `pos`, the first two.
fn top_two_by_pos(pid: Id<Playlist>) -> Made<Item> {
    Arc::new(move |_, _| {
        Ok(ark::plan::scan::<Lib, Item>()
            .filter(move |i| i.playlist_id == pid)
            .sort(|i| (i.pos, i.playlist_id, i.track_id.clone()))
            .take(2))
    })
}

type WithItems = (Id<Playlist>, String, String, Vec<Item>);

/// `playlist-with-items`: playlists by name, each with its last three items.
fn playlist_with_items() -> Made<WithItems> {
    Arc::new(|_, _| {
        Ok(ark::plan::scan::<Lib, Playlist>().sort(|p| (p.name.clone(), p.id)).map_with(|db, p| {
            let last = db.item.by(Item::PLAYLIST_ID_POS).from(p.id).rev().take(3);
            (p.id, p.name.clone(), p.user_id.clone(), last)
        }))
    })
}

/// `group-source`: songs grouped by creator, each with how many, in the
/// creator's order.
fn group_source() -> Made<(String, i64)> {
    Arc::new(|_, _| {
        Ok(ark::plan::scan::<Lib, Song>()
            .group(|s| s.creator.clone(), |_, k, ms| (k.clone(), ms.len() as i64))
            .sort(|g| g.0.clone()))
    })
}

fn vctx() -> Ctx {
    Ctx {
        actor: "alice".into(),
        session: "dev".into(),
        ..Default::default()
    }
}

/// One view of `make` over `store`: hydrated, then `ops` applied to the
/// store and pushed one at a time, `rounds` times over — the mean per
/// push; and the reads one push of the first op makes, counted.
fn time_view<T: ark::plan::Elem + std::fmt::Debug>(
    make: Made<T>,
    mut store: Store,
    ops: &[Change],
    rounds: usize,
) -> (Duration, f64, ark::db::Reads) {
    let t = Instant::now();
    let mut v = ark::plan::View::new(make.clone(), vctx(), &store).unwrap();
    let hydrate = t.elapsed();
    // The first op, counted.
    store.apply(&ops[0]).unwrap();
    let (_, reads) = ark::db::count(|| v.push(std::slice::from_ref(&ops[0]), &store).unwrap());
    for c in &ops[1..] {
        store.apply(c).unwrap();
        v.push(std::slice::from_ref(c), &store).unwrap();
    }
    let mut spent = Duration::ZERO;
    for _ in 0..rounds {
        for c in ops {
            store.apply(c).unwrap();
            let t = Instant::now();
            v.push(std::slice::from_ref(c), &store).unwrap();
            spent += t.elapsed();
        }
    }
    assert_eq!(
        v.rows(),
        ark::plan::answer(&make, &vctx(), &store).unwrap(),
        "the view is a fresh evaluation"
    );
    (hydrate, us(spent) / (rounds * ops.len()) as f64, reads)
}

/// One change against each of three maintained views at 1k, 4k and 16k
/// rows: an item added to (and taken off) one of n/4 playlists of four
/// under `playlist-with-items`; the first item of a playlist of n leaving
/// (and coming back) under `top-two-by-pos`; a song changing creator (and
/// back) among n songs of eight creators under `group-source`. Time per
/// push, the store already moved; and what one push read — gets and rows
/// a scan returned (v5's store examines only what it returns).
#[test]
#[ignore]
fn perf_views() {
    let rounds = 2000;
    eprintln!("\n== views: one change against a maintained view (View::push), the store already moved");
    eprintln!(
        "{:<24} {:>7} {:>12} {:>12} {:>8} {:>8}",
        "query", "rows", "hydrate ms", "push µs", "gets", "rows"
    );
    for n in [1000u32, 4000, 16000] {
        let mut rows = vec![];
        for p in 0..n / 4 {
            rows.push(pl(p, &format!("p{p}"), "alice"));
            rows.extend((1..=4).map(|k| add("item", it_row(p, &format!("t{k}"), k as i64))));
        }
        let fresh = add("item", it_row(n / 8, "t5", 5));
        let ops = [fresh.clone(), fresh.inverse()];
        let (hy, push, r) = time_view(playlist_with_items(), store_of(&rows), &ops, rounds);
        eprintln!(
            "{:<24} {:>7} {:>12.2} {:>12.2} {:>8} {:>8}",
            "playlist-with-items",
            n,
            hy.as_secs_f64() * 1e3,
            push,
            r.gets,
            r.rows
        );
        let mut rows = vec![pl(1, "Viewed", "alice")];
        rows.extend((0..n).map(|i| add("item", it_row(1, &format!("t{i}"), i as i64 + 1))));
        let first = add("item", it_row(1, "t0", 1));
        let ops = [first.inverse(), first];
        let (hy, push, r) = time_view(top_two_by_pos(Id::new(vpid(1).try_into().unwrap())), store_of(&rows), &ops, rounds);
        eprintln!(
            "{:<24} {:>7} {:>12.2} {:>12.2} {:>8} {:>8}",
            "top-two-by-pos",
            n,
            hy.as_secs_f64() * 1e3,
            push,
            r.gets,
            r.rows
        );
        let rows: Vec<Change> = (0..n)
            .map(|i| add("song", song_row(&format!("s{i}"), &format!("c{}", i % 8), i as i64)))
            .collect();
        let (c0, c1) = (song_row("s0", "c0", 0), song_row("s0", "c1", 0));
        let ops = [
            Change::Edit {
                table: "song".into(),
                old: c0.clone(),
                new: c1.clone(),
            },
            Change::Edit {
                table: "song".into(),
                old: c1,
                new: c0,
            },
        ];
        let (hy, push, r) = time_view(group_source(), store_of(&rows), &ops, rounds);
        eprintln!(
            "{:<24} {:>7} {:>12.2} {:>12.2} {:>8} {:>8}",
            "group-source",
            n,
            hy.as_secs_f64() * 1e3,
            push,
            r.gets,
            r.rows
        );
    }
}
