//! harken's server in this process on a real port, and peers that pump
//! against it.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use ark_auth::Account;
use ark_client::{Domain, Options, Peer, Timing};
use harken_server::{start, Config, Server};

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

pub fn domain() -> Domain {
    Domain::new(&harken_domain::module())
}

/// Reconnect fast: a test has no thirty seconds to spare.
pub fn quick() -> Timing {
    Timing {
        first_backoff_ms: 20,
        max_backoff_ms: 200,
        ping_every_ms: 20_000,
        connect_timeout_ms: 2_000,
    }
}

/// A dev-auth server on an ephemeral port, told whatever `more` adds.
pub fn serve(
    rt: &tokio::runtime::Runtime,
    data: &std::path::Path,
    more: impl FnOnce(&mut Config),
) -> Server {
    let mut config = Config::dev("127.0.0.1:0", data);
    more(&mut config);
    rt.block_on(start(config)).expect("the server starts")
}

/// `user`, signed in on this server as a device of their own: a peer in
/// memory with a fresh login, dialling the socket.
pub fn signed_in(server: &Server, user: &str) -> Peer {
    let login = server
        .auth
        .issue(&Account {
            id: user.into(),
            ..Account::default()
        })
        .unwrap();
    let opts =
        Options::server(login.user.id, login.session, Some(login.token)).with_timing(quick());
    let mut p = Peer::open_memory(server.domain.clone(), opts).unwrap();
    p.connect(&server.running.sync_url());
    p
}

/// Pump every peer until `done` says so, or `secs` pass.
pub fn pump_until(
    peers: &mut [&mut Peer],
    secs: u64,
    mut done: impl FnMut(&mut [&mut Peer]) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(secs);
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

/// Wait for `f` to hold, or `secs` pass; what it last said either way.
pub fn eventually<T>(secs: u64, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(t) = f() {
            return Some(t);
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A playable WAV of `ms` milliseconds: 44.1kHz, mono, 16-bit silence —
/// silence because nothing listens to it, and a real header because lofty
/// reads a duration out of it the way it would from a real track.
pub fn wav(path: &std::path::Path, ms: u32) {
    let rate = 44_100u32;
    let samples = rate * ms / 1000;
    let data = samples * 2;
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // mono
    out.extend(rate.to_le_bytes());
    out.extend((rate * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data.to_le_bytes());
    out.extend(std::iter::repeat_n(0u8, data as usize));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Written beside and renamed in, so a watch sees a whole file appear
    // rather than one being written.
    let tmp = path.with_extension("partial");
    std::fs::write(&tmp, out).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}
