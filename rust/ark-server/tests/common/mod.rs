//! A server in this process on a real port, and peers that pump against it.
#![allow(dead_code)]

use std::path::Path;
use std::time::{Duration, Instant};

use ark::store::Store;
use ark::value::{Id, Value};
use ark_client::{demo, Options, Peer, Timing};
use ark_server::{Builder, Running};

/// Reconnect fast: a test has no thirty seconds to spare.
pub fn quick() -> Timing {
    Timing {
        first_backoff_ms: 20,
        max_backoff_ms: 200,
        ping_every_ms: 20_000,
        connect_timeout_ms: 2_000,
    }
}

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
}

/// The demo module, dev auth, and whatever `more` adds.
pub fn serve(rt: &tokio::runtime::Runtime, data: Option<&Path>, more: impl FnOnce(Builder) -> Builder) -> Running {
    let mut b = ark_server::builder(demo::domain()).name("test").trusting();
    if let Some(d) = data {
        b = b.data(d);
    }
    let app = more(b).build().expect("the server builds");
    rt.block_on(app.serve("127.0.0.1:0")).expect("the server listens")
}

/// A dev-auth peer in memory, dialling `running`.
pub fn peer(running: &Running, name: &str) -> Peer {
    let mut p = Peer::open_memory(demo::domain(), Options::dev(name).with_timing(quick())).unwrap();
    p.connect(&running.sync_url());
    p
}

/// Pump every peer until `done` says so, or ten seconds pass.
pub fn pump_until(peers: &mut [&mut Peer], mut done: impl FnMut(&mut [&mut Peer]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        for p in peers.iter_mut() {
            p.pump();
        }
        if done(peers) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out; {:#?}",
            peers.iter().map(|p| p.status()).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Pump for a while, whatever happens.
pub fn pump_for(peers: &mut [&mut Peer], ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        for p in peers.iter_mut() {
            p.pump();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The only playlist a peer's view holds, by name.
pub fn playlist(p: &Peer, name: &str) -> Id {
    let rows = p.store().scan("playlist");
    let row = rows
        .iter()
        .find(|r| r["name"] == Value::text(name))
        .unwrap_or_else(|| panic!("no playlist {name}: {rows:?}"));
    match &row["id"] {
        Value::Id(i) => *i,
        other => panic!("an id, not {other:?}"),
    }
}

/// The track ids of a playlist, in position order, as `items` answers.
pub fn tracks(p: &Peer, list: Id) -> Vec<String> {
    let v = p.query("items", &ark_client::args([("playlist_id", Value::Id(list))])).unwrap();
    rows_tracks(match &v {
        Value::List(xs) => xs,
        other => panic!("{other:?}"),
    })
}

pub fn rows_tracks(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .map(|r| match r {
            Value::Struct(m) => match &m["track_id"] {
                Value::Text(t) => t.clone(),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        })
        .collect()
}
