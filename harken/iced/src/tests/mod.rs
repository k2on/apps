//! The window, driven the way a person drives it: messages in, state out.
//! Nothing here opens a window — none of these can say what anything looks
//! like, only that the arithmetic and the rules behind it hold.

mod account;
mod explorer;
mod geometry;
mod queue;
mod signing_in;
mod volume;
mod wake;

#[cfg(feature = "demo")]
mod bench;
#[cfg(feature = "demo")]
mod context;
#[cfg(feature = "demo")]
mod demo;
#[cfg(feature = "demo")]
mod views;

use crate::domain::AddSong;
use ark::peer::Who;

use crate::client::Replica;
use crate::listening::{Device, Kind};

fn who(me: &str) -> Who {
    Who {
        actor: me.into(),
        session: format!("{me}-device"),
        roles: vec![],
    }
}

/// A replica's lists, its device holding the `library` role
/// (`docs/plan-guards.md` D1): these windows fill their own library with
/// `add_song`, which `is_library` gives that role alone, as a server's
/// scanner holds it.
fn lists(mut client: Replica) -> crate::peer::Peer {
    client.set_roles([crate::domain::LIBRARY]);
    crate::peer::Peer::open(client)
}

/// A replica in memory, its own authority, as `me`.
pub fn alone(me: &str) -> crate::peer::Peer {
    let mut client = crate::client::memory(who(me), None);
    client.alone();
    lists(client)
}

/// A replica in memory, signed in as `me` with a token, reaching no server
/// until it is given one.
pub fn signed_in(me: &str) -> crate::peer::Peer {
    lists(crate::client::memory(who(me), Some("dev")))
}

/// A replica in memory nobody has signed in on.
pub fn signed_out() -> crate::peer::Peer {
    lists(crate::client::memory(crate::client::nobody(), None))
}

/// `add_song`'s input, for a track called `title` in `file`.
pub fn song(title: &str, artist: &str, album: &str, file: &str) -> AddSong {
    AddSong {
        title: title.into(),
        artist: artist.into(),
        album: album.into(),
        duration_ms: 200_000,
        file: file.into(),
        ..AddSong::default()
    }
}

/// A device standing in a session, as `on_join` makes one.
pub fn device(id: &str, name: &str, kind: Kind, can_play: bool) -> Device {
    Device {
        can_play,
        can_volume: false,
        family: None,
        follows: None,
        id: id.into(),
        kind: kind.name().into(),
        muted: false,
        name: name.into(),
        user: "me".into(),
        volume: crate::domain::host::UNSAID_VOLUME,
    }
}

/// A connection made of two queues, for a test: what the test pushes is
/// what the connection says; what the replica sends is kept.
#[derive(Clone, Default)]
pub struct Queues {
    pub said: std::sync::Arc<std::sync::Mutex<Vec<ark_peer::Event>>>,
    pub sent: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    pub wake: std::sync::Arc<std::sync::Mutex<Option<crate::client::Wake>>>,
}

impl Queues {
    /// Something the connection says, and the wake rung.
    pub fn push(&self, e: ark_peer::Event) {
        self.said.lock().unwrap().push(e);
        if let Some(w) = &*self.wake.lock().unwrap() {
            w();
        }
    }

    /// A dialer whose every connection is a fresh pair of queues, each
    /// kept in `made`.
    pub fn dialer(made: std::sync::Arc<std::sync::Mutex<Vec<Queues>>>) -> impl ark_peer::link::Dial {
        move |_url: &str, _token: &str, wake: Option<crate::client::Wake>| {
            let q = Queues::default();
            *q.wake.lock().unwrap() = wake;
            made.lock().unwrap().push(q.clone());
            Box::new(q) as Box<dyn ark_peer::link::Wire>
        }
    }
}

impl ark_peer::link::Wire for Queues {
    fn send(&mut self, frame: Vec<u8>) {
        self.sent.lock().unwrap().push(frame);
    }
    fn events(&mut self) -> Vec<ark_peer::Event> {
        std::mem::take(&mut *self.said.lock().unwrap())
    }
}

/// The demo, seeded once per test run and handed to each test as a replica
/// of its own.
///
/// Seeding is seconds in a debug build, and every context test starts from
/// the same library. So the first test to ask seeds it and keeps the
/// replica's records, and every test opens its own peer from those — the
/// same library, nothing shared — and hydrates its own views, which is what
/// a window opening does.
#[cfg(feature = "demo")]
pub fn demo_app() -> crate::App {
    use std::sync::OnceLock;

    static TEMPLATE: OnceLock<ark_peer::Memory> = OnceLock::new();
    let disk = TEMPLATE.get_or_init(|| crate::seed::seeded_disk(1)).clone();
    crate::App::with_peer(crate::peer::Peer::open(crate::seed::open(disk)), String::new(), None)
}
