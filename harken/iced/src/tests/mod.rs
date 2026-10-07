//! The window, driven the way a person drives it: messages in, state out.
//! Nothing here opens a window — none of these can say what anything looks
//! like, only that the arithmetic and the rules behind it hold.

mod geometry;
mod signing_in;

#[cfg(feature = "demo")]
mod bench;
#[cfg(feature = "demo")]
mod context;
#[cfg(feature = "demo")]
mod demo;
#[cfg(feature = "demo")]
mod views;

use ark_client::{args, Args, Domain, Options, Value};

/// A replica in memory, opened with `opts`, wrapped as the window's peer.
///
/// Its device holds the `library` role (`docs/plan-guards.md` D1): these
/// windows fill their own library with `add_song`, which `is_library` gives
/// that role alone, as a server's scanner holds it.
pub fn peer(opts: Options) -> crate::peer::Peer {
    let mut client = ark_client::Peer::open_memory(Domain::new(&harken_domain::module()), opts).expect("a peer in memory opens");
    client.set_roles([harken_domain::schema::LIBRARY]);
    crate::peer::Peer::open(client)
}

/// `add_song`'s arguments, for a track called `title` in `file`.
pub fn song(title: &str, artist: &str, album: &str, file: &str) -> Args {
    let t = |s: &str| Value::text(s);
    args([
        ("title", t(title)),
        ("artist", t(artist)),
        ("album", t(album)),
        ("duration_ms", Value::Int(200_000)),
        ("file", t(file)),
        ("track", Value::Int(0)),
        ("part", t("")),
        ("catalogue", t("")),
        ("performer", t("")),
        ("bpm", Value::Int(0)),
        ("album_art", t("")),
        ("artist_art", t("")),
        ("disc", Value::Int(0)),
        ("work_title", t("")),
        ("movement_no", Value::Int(0)),
    ])
}

/// The demo, seeded once per test run and handed to each test as a replica
/// of its own.
///
/// Seeding is seconds in a debug build, and every context test starts from
/// the same library. So the first test to ask seeds it and keeps the
/// replica's bytes, and every test opens its own peer from those bytes — the
/// same library, nothing shared — and hydrates its own views, which is what
/// a window opening does and is cheap now that every query is a plan.
#[cfg(feature = "demo")]
pub fn demo_app() -> crate::App {
    use std::sync::OnceLock;

    use ark_client::storage::{Memory, ReplicaFile, Storage};

    static TEMPLATE: OnceLock<Vec<u8>> = OnceLock::new();
    let domain = Domain::new(&harken_domain::module());
    let bytes = TEMPLATE.get_or_init(|| {
        let client = crate::seed::seeded(domain.clone());
        let r = client.replica();
        ReplicaFile {
            fork: Default::default(),
            cursor: r.cursor,
            confirmed: r.confirmed.clone(),
            pending: r.pending.clone(),
            user: crate::seed::DEMO.into(),
            session: "local".into(),
            partial: None,
        }
        .encode()
    });
    let mut disk = Memory::new();
    disk.save(ReplicaFile::KEY, bytes).unwrap();
    let mut client = ark_client::Peer::open(domain, Box::new(disk), Options::alone(crate::seed::DEMO)).unwrap();
    // The demo authored its own library, as the scanner would: it holds the
    // role the library's guard asks (`docs/plan-guards.md` D1).
    client.set_roles([harken_domain::schema::LIBRARY]);
    crate::App::with_peer(crate::peer::Peer::open(client), String::new(), None)
}
