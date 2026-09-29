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

use ark_client::{args, Args, Domain, Options, Value};

/// A replica in memory, opened with `opts`, wrapped as the window's peer.
pub fn peer(opts: Options) -> crate::peer::Peer {
    let client = ark_client::Peer::open_memory(Domain::new(&harken_domain::module()), opts).expect("a peer in memory opens");
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
/// Seeding and the first read of the demo's lists are seconds each in a debug
/// build — the domain's widest queries scan in nested loops — and every
/// context test starts from the same library. So the first test to ask seeds
/// it, keeps the replica's bytes and the lists read from it, and every test
/// opens its own peer from those bytes: the same library, nothing shared.
#[cfg(feature = "demo")]
pub fn demo_app() -> crate::App {
    use std::sync::OnceLock;

    use ark_client::storage::{Memory, ReplicaFile, Storage};

    use crate::peer::{Choice, Peer};
    use crate::rows::{Album, Artist, Composer, TrackDetail};

    struct Template {
        bytes: Vec<u8>,
        choices: Vec<Choice>,
        albums: Vec<Album>,
        artists: Vec<Artist>,
        composers: Vec<Composer>,
        details: std::collections::HashMap<ark_client::Id, TrackDetail>,
    }
    static TEMPLATE: OnceLock<Template> = OnceLock::new();
    let domain = Domain::new(&harken_domain::module());
    let t = TEMPLATE.get_or_init(|| {
        let peer = crate::App::demo().peer;
        let r = peer.client.replica();
        let file = ReplicaFile {
            mode: "alone".into(),
            cursor: r.cursor,
            confirmed: r.confirmed.clone(),
            pending: r.pending.clone(),
            user: crate::seed::DEMO.into(),
            session: "local".into(),
        };
        Template {
            bytes: file.encode(),
            choices: peer.choices.clone(),
            albums: peer.albums.clone(),
            artists: peer.artists.clone(),
            composers: peer.composers.clone(),
            details: peer.details.clone(),
        }
    });
    let mut disk = Memory::new();
    disk.save(ReplicaFile::KEY, &t.bytes).unwrap();
    let client = ark_client::Peer::open(domain, Box::new(disk), Options::alone(crate::seed::DEMO)).unwrap();
    let mut peer = Peer::hydrated(client);
    peer.choices = t.choices.clone();
    peer.albums = t.albums.clone();
    peer.artists = t.artists.clone();
    peer.composers = t.composers.clone();
    peer.details = t.details.clone();
    crate::App::with_peer(peer, String::new(), None)
}
