//! What bulk work costs, printed rather than asserted: authoring many
//! intents on one peer, and a fresh replica catching up on a long log.
//! Ignored by default; run with
//! `cargo test -p ark-server --release --test bench -- --ignored --nocapture`.

use std::time::Instant;

use ark_client::ark::eval::Ctx;
use ark_client::ark::peer::{Authority, Replica, Sequenced};
use ark_client::ark::store::MemoryStore;
use ark_client::{args, demo, Args, Options, Peer, Value};

const N: usize = 1500;

fn playlist_of(p: &Peer) -> Value {
    use ark_client::ark::store::Store;
    p.store().scan("playlist")[0]["id"].clone()
}

fn item(pl: &Value, i: usize) -> Args {
    args([("playlist_id", pl.clone()), ("track_id", Value::text(format!("t{i}")))])
}

fn bulk(label: &str, opts: Options) {
    let mut p = Peer::open_memory(demo::domain(), opts).unwrap();
    p.mutate("create_playlist", args([("name", Value::text("Bulk"))])).unwrap();
    let pl = playlist_of(&p);
    let t0 = Instant::now();
    let mut first = None;
    let mut last = None;
    for i in 0..N {
        let t = Instant::now();
        p.mutate("add_to_playlist", item(&pl, i)).unwrap();
        let d = t.elapsed();
        if i < 100 {
            *first.get_or_insert(std::time::Duration::ZERO) += d;
        }
        if i >= N - 100 {
            *last.get_or_insert(std::time::Duration::ZERO) += d;
        }
    }
    eprintln!(
        "{label:>12}: {N} mutates in {:.1?}; first 100 {:.1?}, last 100 {:.1?}",
        t0.elapsed(),
        first.unwrap(),
        last.unwrap()
    );
}

#[test]
#[ignore]
fn bench_bulk_mutate_server_mode() {
    bulk("server mode", Options::dev("alice"));
}

#[test]
#[ignore]
fn bench_bulk_mutate_alone() {
    bulk("alone", Options::alone("me"));
}

/// The engine alone: `Replica::mutate`, nothing written, nothing committed.
#[test]
#[ignore]
fn bench_engine_mutate() {
    let d = demo::domain();
    let schema = d.module().schema.clone();
    let ctx = Ctx::new("alice", "dev");
    let id = |n: usize| -> [u8; 16] {
        let mut b = [0u8; 16];
        b[8..].copy_from_slice(&(n as u64).to_be_bytes());
        b
    };
    let mut r = Replica::open(schema.clone(), d.closures().clone(), MemoryStore::empty(schema.clone()), 0, vec![]);
    r.hold(d.native_list());
    let (create, _) = d.mutator("create_playlist").unwrap();
    let (add, _) = d.mutator("add_to_playlist").unwrap();
    let (create, add) = (create.clone(), add.clone());
    let pl = Value::Id(id(1_000_000));
    r.mutate(id(0), &ctx, &create, &args([("id", pl.clone())]), &args([("name", Value::text("Bulk"))]))
        .unwrap();
    let t0 = Instant::now();
    let (mut first, mut last) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    for i in 0..N {
        let t = Instant::now();
        r.mutate(id(i + 1), &ctx, &add, &args([]), &item(&pl, i)).unwrap();
        let dt = t.elapsed();
        if i < 100 {
            first += dt;
        }
        if i >= N - 100 {
            last += dt;
        }
    }
    eprintln!(
        "      engine: {N} mutates in {:.1?}; first 100 {first:.1?}, last 100 {last:.1?}",
        t0.elapsed()
    );
}

#[test]
#[ignore]
fn bench_initial_sync() {
    let d = demo::domain();
    let schema = d.module().schema.clone();
    let ctx = Ctx::new("alice", "dev");
    let id = |n: usize| -> [u8; 16] {
        let mut b = [0u8; 16];
        b[8..].copy_from_slice(&(n as u64).to_be_bytes());
        b
    };
    // An author, and the authority it pushes to.
    let mut author = Replica::open(schema.clone(), d.closures().clone(), MemoryStore::empty(schema.clone()), 0, vec![]);
    author.hold(d.native_list());
    let mut a = Authority::new(schema.clone(), d.closures().clone());
    a.hold(d.native_list());
    let (create, _) = d.mutator("create_playlist").unwrap();
    let (add, _) = d.mutator("add_to_playlist").unwrap();
    let (create, add) = (create.clone(), add.clone());
    let pl = Value::Id(id(1_000_000));
    let mut entries = vec![author
        .mutate(id(0), &ctx, &create, &args([("id", pl.clone())]), &args([("name", Value::text("Bulk"))]))
        .unwrap()];
    for i in 0..N {
        entries.push(author.mutate(id(i + 1), &ctx, &add, &args([]), &item(&pl, i)).unwrap());
    }
    let t0 = Instant::now();
    let mut log = Vec::new();
    for e in &entries {
        match a.sequence_entry(e) {
            Sequenced::Appended(n, facts) => log.push((n, e.clone(), facts)),
            other => panic!("{other:?}"),
        }
    }
    eprintln!("   authority: {} entries sequenced in {:.1?}", log.len(), t0.elapsed());
    // A fresh replica receiving all of it, in order, as a batch delivers it.
    let mut r = Replica::open(schema.clone(), d.closures().clone(), MemoryStore::empty(schema.clone()), 0, vec![]);
    r.hold(d.native_list());
    // As the protocol delivers it: pages of BATCH_LIMIT entries.
    let pages = || {
        log.chunks(ark_client::ark::protocol::BATCH_LIMIT)
            .map(|page| page.iter().map(|(n, e, f)| (*n, e.clone(), Some(f.clone()))))
    };
    let t1 = Instant::now();
    for page in pages() {
        r.receive_batch(page);
    }
    assert_eq!(r.cursor, log.len() as i64);
    eprintln!("     replica: {} entries received in {:.1?}", log.len(), t1.elapsed());
    // And one that had a pending intent of its own throughout: a rebase per entry.
    let mut r = Replica::open(schema.clone(), d.closures().clone(), MemoryStore::empty(schema.clone()), 0, vec![]);
    r.hold(d.native_list());
    r.mutate(
        id(9_999_999),
        &ctx,
        &create,
        &args([("id", Value::Id(id(2_000_000)))]),
        &args([("name", Value::text("Mine"))]),
    )
    .unwrap();
    let t2 = Instant::now();
    for page in pages() {
        r.receive_batch(page);
    }
    eprintln!("   + pending: {} entries received in {:.1?}", log.len(), t2.elapsed());
}
