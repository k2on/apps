//! The demo's domain, standing in for harken's now playing (step 5 of
//! `docs/north-star.md`): what the watch and its upstream must agree on,
//! so both link it.
//!
//! - **The queue**, a shared table ([`QUEUE`]): the tracks to come, keyed
//!   by their place (four bytes big-endian, so a scan reads them in order).
//!   [`Msg::Add`] puts one at the end, [`Msg::Pop`] takes one off when it
//!   starts playing, and refuses ([`GONE`]) when another device took it
//!   first.
//! - **Playback**, a session row ([`PLAYBACK`]) the player owns and sets
//!   whole ([`Msg::Play`]): the track and an [`Anchor`] every view
//!   extrapolates, so nothing streams while it plays. Its key is the
//!   player's origin, so the kernel knows the row is the player's: another
//!   device's entry for it is a control, refused while that device has no
//!   link ([`ark_kernel::Refusal::OFFLINE`]).
//! - **The query** a watch subscribes to ([`NEXT`]): the queue's first
//!   [`HELD`] rows, of which its screen shows the first few.
//! - **The screen's record** ([`face`]): what the watch's `#[view]` reads.

#![no_std]

extern crate alloc;

pub mod face;

use alloc::vec::Vec;
use core::ops::Bound;

use ark_kernel::codec::{from_slice, to_vec, Error, Reader};
use ark_kernel::{scan, Anchor, Body, Cx, Decode, Encode, Handler, HandlerId, Origin, Plan, Query, QueryId, Read, Refusal, Space, TableId, Version};

/// The build's version.
pub const VERSION: Version = Version(1);
/// The queue: a track per place.
pub const QUEUE: TableId = TableId(1);
/// The session rows of playback: one per player, keyed by its origin
/// ([`Origin::key`]).
pub const PLAYBACK: TableId = TableId(2);
/// The queue's first rows.
pub const NEXT: QueryId = QueryId(0);
/// How many of the queue's rows a watch holds.
pub const HELD: usize = 5;
/// What `Pop` refuses with: the track is no longer queued.
pub const GONE: Refusal = Refusal(256);

/// A track: what a row of the queue holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    pub title: Vec<u8>,
    pub artist: Vec<u8>,
    /// Its length in milliseconds.
    pub ms: u32,
}

/// What is playing: the track and where it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Playing {
    pub track: Track,
    pub anchor: Anchor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Queues a track at a place.
    Add { at: u32, track: Track },
    /// Takes the track at a place off the queue.
    Pop { at: u32 },
    /// Sets a player's playback whole: its own, or a control of it.
    Play { player: Origin, playing: Playing },
}

impl Body for Msg {
    fn handler(&self) -> HandlerId {
        HandlerId(match self {
            Msg::Add { .. } => 0,
            Msg::Pop { .. } => 1,
            Msg::Play { .. } => 2,
        })
    }

    fn row(&self) -> Option<(TableId, Vec<u8>)> {
        match self {
            Msg::Play { player, .. } => Some((PLAYBACK, player.key().to_vec())),
            _ => None,
        }
    }
}

/// The handlers, by the numbers [`Body::handler`] gives.
pub static HANDLERS: [Handler<Msg>; 3] = [add, pop, play];

fn add(cx: &mut Cx<'_, Msg>) -> Result<(), Refusal> {
    let Msg::Add { at, track } = &cx.entry.body else {
        return Err(Refusal::NO_HANDLER);
    };
    cx.db.put(QUEUE, &at.to_be_bytes(), &to_vec(track));
    Ok(())
}

fn pop(cx: &mut Cx<'_, Msg>) -> Result<(), Refusal> {
    let Msg::Pop { at } = cx.entry.body else {
        return Err(Refusal::NO_HANDLER);
    };
    let key = at.to_be_bytes();
    if cx.db.get(QUEUE, &key).is_none() {
        return Err(GONE);
    }
    cx.db.delete(QUEUE, &key);
    Ok(())
}

fn play(cx: &mut Cx<'_, Msg>) -> Result<(), Refusal> {
    let Msg::Play { player, playing } = &cx.entry.body else {
        return Err(Refusal::NO_HANDLER);
    };
    if cx.entry.space != Space::Session {
        return Err(Refusal::NO_HANDLER);
    }
    cx.db.put(PLAYBACK, &player.key(), &to_vec(playing));
    Ok(())
}

/// The queue's first rows, as an upstream runs it for a watch.
fn next(_args: &[u8]) -> Plan {
    scan(QUEUE, Bound::Unbounded, Bound::Unbounded).take(HELD, false)
}

/// The queries, by their numbers.
pub static QUERIES: [Query; 1] = [next];

/// The queue's first `n` rows over a node's view: what a screen shows.
pub fn upcoming(n: usize) -> Plan {
    scan(QUEUE, Bound::Unbounded, Bound::Unbounded).take(n, false)
}

/// A queue row's place, from its key.
pub fn place(key: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(key.try_into().ok()?))
}

/// What is playing, from a session space: the first player's, with whose
/// it is.
pub fn playing(db: &dyn Read) -> Option<(Origin, Playing)> {
    let mut first = None;
    db.scan(PLAYBACK, ark_kernel::store::Range::all(), &mut |k, v| {
        let player = k.try_into().ok().map(|k| Origin(u64::from_be_bytes(k)));
        first = player.zip(from_slice(v).ok());
        false
    });
    first
}

impl Encode for Track {
    fn encode(&self, out: &mut Vec<u8>) {
        self.title.encode(out);
        self.artist.encode(out);
        self.ms.encode(out);
    }
}

impl Decode for Track {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Track {
            title: Vec::decode(r)?,
            artist: Vec::decode(r)?,
            ms: u32::decode(r)?,
        })
    }
}

impl Encode for Playing {
    fn encode(&self, out: &mut Vec<u8>) {
        self.track.encode(out);
        self.anchor.encode(out);
    }
}

impl Decode for Playing {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Playing {
            track: Track::decode(r)?,
            anchor: Anchor::decode(r)?,
        })
    }
}

impl Encode for Msg {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Msg::Add { at, track } => {
                0u32.encode(out);
                at.encode(out);
                track.encode(out);
            }
            Msg::Pop { at } => {
                1u32.encode(out);
                at.encode(out);
            }
            Msg::Play { player, playing } => {
                2u32.encode(out);
                player.encode(out);
                playing.encode(out);
            }
        }
    }
}

impl Decode for Msg {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        match u32::decode(r)? {
            0 => Ok(Msg::Add {
                at: u32::decode(r)?,
                track: Track::decode(r)?,
            }),
            1 => Ok(Msg::Pop { at: u32::decode(r)? }),
            2 => Ok(Msg::Play {
                player: Origin::decode(r)?,
                playing: Playing::decode(r)?,
            }),
            _ => Err(Error::Tag),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_kernel::{Actor, Authority, Memory, Node, Origin, Spaces};

    fn spaces() -> Spaces<Memory> {
        Spaces {
            local: Memory::new(),
            device: Memory::new(),
            session: Memory::new(),
            shared: Memory::new(),
        }
    }

    fn track(t: &str) -> Track {
        Track {
            title: t.into(),
            artist: b"x".to_vec(),
            ms: 1000,
        }
    }

    #[test]
    fn bodies_round_trip() {
        for m in [
            Msg::Add { at: 7, track: track("a") },
            Msg::Pop { at: 9 },
            Msg::Play {
                player: Origin(0x9a7e),
                playing: Playing {
                    track: track("b"),
                    anchor: Anchor {
                        at: 1,
                        wall: 2,
                        rate: 1000,
                        playing: true,
                    },
                },
            },
        ] {
            assert_eq!(from_slice::<Msg>(&to_vec(&m)), Ok(m));
        }
    }

    /// The player's row is its own: set offline it is applied; a watch's
    /// pause of it is refused offline and applied linked; the watch reads
    /// whose playback it is.
    #[test]
    fn playback_is_the_players() {
        let mut player = Node::new(Origin(0x9a7e), 0, VERSION, &HANDLERS, spaces());
        let mut watch = Node::new(Origin(0x3a7c), 0, VERSION, &HANDLERS, spaces());
        let playing = Playing {
            track: track("a"),
            anchor: Anchor::default(),
        };
        let play = |n: &mut Node<Msg, Memory>, playing: &Playing, at| {
            let e = n.make(
                Space::Session,
                Msg::Play {
                    player: Origin(0x9a7e),
                    playing: playing.clone(),
                },
                at,
            );
            let mut up = alloc::vec::Vec::new();
            n.route(e.clone(), &mut up);
            (e, up)
        };
        let (e, up) = play(&mut player, &playing, 10);
        assert_eq!((up.len(), player.take_refused()), (1, alloc::vec![]));
        let (pause, up) = play(&mut watch, &playing, 20);
        assert_eq!((up.len(), watch.take_refused()), (0, alloc::vec![(pause.id, Refusal::OFFLINE)]));
        watch.connect(&mut alloc::vec::Vec::new(), 30);
        watch.receive(ark_kernel::Down::Live(e), &mut alloc::vec::Vec::new(), 30);
        assert_eq!(super::playing(&watch.view(Space::Session)), Some((Origin(0x9a7e), playing.clone())));
        let (_, up) = play(&mut watch, &playing, 40);
        assert_eq!(up.len(), 1);
    }

    /// Two devices pop the same track: the second is refused, and the
    /// queue holds the rest.
    #[test]
    fn a_pop_is_judged_once() {
        let mut server = Authority::new(&HANDLERS, Memory::new());
        server.set_queries(&QUERIES);
        let mut a = Node::new(Origin(1), 0, VERSION, &HANDLERS, spaces());
        a.set_actor(Actor(1));
        for at in 0..3 {
            let e = a.make(Space::Shared, Msg::Add { at, track: track("t") }, 10);
            let mut up = alloc::vec::Vec::new();
            a.route(e, &mut up);
        }
        server.open(ark_kernel::Conn(0), Actor(1), None);
        let mut up = alloc::vec::Vec::new();
        a.connect(&mut up, 15);
        let pop = a.make(Space::Shared, Msg::Pop { at: 0 }, 20);
        a.route(pop.clone(), &mut up);
        let again = Node::<Msg, Memory>::new(Origin(2), 0, VERSION, &HANDLERS, spaces()).make(Space::Shared, Msg::Pop { at: 0 }, 30);
        for u in up {
            server.receive(ark_kernel::Conn(0), u, 40);
        }
        server.open(ark_kernel::Conn(1), Actor(1), None);
        let out = server.receive(ark_kernel::Conn(1), ark_kernel::Up::Push(again.clone()), 50);
        assert!(out.iter().any(|(_, d)| matches!(
            d,
            ark_kernel::Down::Verdict {
                verdict: ark_kernel::Verdict::Refused(GONE),
                ..
            }
        )));
        assert_eq!(server.store().len(QUEUE), 2);
    }
}
