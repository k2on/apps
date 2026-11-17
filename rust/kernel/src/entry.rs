//! The one message: an [`Entry`], its envelope shared by every machine and
//! its body each machine's own type.
//!
//! ```text
//! Entry { id: { origin, n }, space, actor, at: { wall, logical }, cause, version, seq, body }
//! ```
//!
//! An entry's id is where it was made and its count there, and is never
//! renumbered: the device's [`Origin`], drawn once and kept, and `n`, that
//! origin's count of the entries it made, from 1. Its [`Seq`] is set only
//! when the master log orders it. Its time is a hybrid logical clock
//! ([`Hlc`]), so entries are ordered across devices whose clocks disagree.
//! Ids and names are integers on the wire: an actor, a version, and (in
//! the body) a handler or a table are each a number the build gives them.

use alloc::vec::Vec;

use crate::codec::{Decode, Encode, Error, Reader};

/// Where entries are made: a number a device draws once, at its first
/// start, and keeps. Eight fixed bytes on the wire, being random.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin(pub u64);

/// An entry's id: its origin and its count there, from 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId {
    pub origin: Origin,
    pub n: u64,
}

/// Where an entry and its data live, and so where it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Space {
    /// A view's state: in memory, never travels.
    Local = 0,
    /// This device's settings: on its disk, never travels.
    Device = 1,
    /// Live state (a playback position, who is here): in memory, to the
    /// user's devices through the server, never stored.
    Session = 2,
    /// The app's data: the master log's, to the server and then to whoever
    /// may see it.
    Shared = 3,
}

impl Space {
    pub const ALL: [Space; 4] = [Space::Local, Space::Device, Space::Session, Space::Shared];
}

/// Who an entry is from: a number the server gives a user. [`Actor::NONE`]
/// is no one signed in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Actor(pub u64);

impl Actor {
    pub const NONE: Actor = Actor(0);
}

/// The build an entry was made by: its schema and API, frozen per release.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u32);

/// A position in the master log, from 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seq(pub u64);

/// A hybrid logical clock: the wall time in milliseconds a node last saw,
/// and a count of the events since at that time. It never goes back, and
/// an event after another, on any device, is later than it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hlc {
    pub wall: u64,
    pub logical: u32,
}

impl Hlc {
    /// The time of an event here at wall time `now`, after this one.
    pub fn tick(self, now: u64) -> Hlc {
        if now > self.wall {
            Hlc { wall: now, logical: 0 }
        } else {
            Hlc {
                wall: self.wall,
                logical: self.logical.saturating_add(1),
            }
        }
    }

    /// The time of taking in an event at `seen`, at wall time `now`: after
    /// this one and after `seen`.
    pub fn witness(self, seen: Hlc, now: u64) -> Hlc {
        let latest = self.max(seen);
        latest.tick(now)
    }
}

/// The one message. `B` is the machine's body, its own enum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry<B> {
    pub id: EntryId,
    pub space: Space,
    pub actor: Actor,
    pub at: Hlc,
    /// The entry this one answers or was made by.
    pub cause: Option<EntryId>,
    pub version: Version,
    /// Set when the master log orders it.
    pub seq: Option<Seq>,
    pub body: B,
}

impl Encode for Origin {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0.to_le_bytes());
    }
}

impl Decode for Origin {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        let mut b = [0; 8];
        b.copy_from_slice(r.take(8)?);
        Ok(Origin(u64::from_le_bytes(b)))
    }
}

impl Encode for EntryId {
    fn encode(&self, out: &mut Vec<u8>) {
        self.origin.encode(out);
        self.n.encode(out);
    }
}

impl Decode for EntryId {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(EntryId {
            origin: Origin::decode(r)?,
            n: u64::decode(r)?,
        })
    }
}

impl Encode for Space {
    fn encode(&self, out: &mut Vec<u8>) {
        out.push(*self as u8);
    }
}

impl Decode for Space {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Space::ALL.get(usize::from(r.byte()?)).copied().ok_or(Error::Tag)
    }
}

/// The integer newtypes, each its number as a varint.
macro_rules! number {
    ($($t:ident($n:ty)),*) => {$(
        impl Encode for $t {
            fn encode(&self, out: &mut Vec<u8>) {
                self.0.encode(out);
            }
        }

        impl Decode for $t {
            fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
                Ok($t(<$n>::decode(r)?))
            }
        }
    )*};
}

number!(Actor(u64), Version(u32), Seq(u64));

impl Encode for Hlc {
    fn encode(&self, out: &mut Vec<u8>) {
        self.wall.encode(out);
        self.logical.encode(out);
    }
}

impl Decode for Hlc {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Hlc {
            wall: u64::decode(r)?,
            logical: u32::decode(r)?,
        })
    }
}

impl<B: Encode> Encode for Entry<B> {
    fn encode(&self, out: &mut Vec<u8>) {
        self.id.encode(out);
        self.space.encode(out);
        self.actor.encode(out);
        self.at.encode(out);
        self.cause.encode(out);
        self.version.encode(out);
        self.seq.encode(out);
        self.body.encode(out);
    }
}

impl<B: Decode> Decode for Entry<B> {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Entry {
            id: EntryId::decode(r)?,
            space: Space::decode(r)?,
            actor: Actor::decode(r)?,
            at: Hlc::decode(r)?,
            cause: Option::decode(r)?,
            version: Version::decode(r)?,
            seq: Option::decode(r)?,
            body: B::decode(r)?,
        })
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::codec::{from_slice, put_frame, to_vec, Frames};
    use crate::Rng;
    use alloc::vec;

    fn random(rng: &mut Rng) -> Entry<Vec<u8>> {
        let id = |rng: &mut Rng| EntryId {
            origin: Origin(rng.next()),
            n: rng.any(),
        };
        Entry {
            id: id(rng),
            space: Space::ALL[rng.below(4) as usize],
            actor: Actor(rng.any()),
            at: Hlc {
                wall: rng.any(),
                logical: rng.any() as u32,
            },
            cause: (rng.below(2) == 0).then(|| id(rng)),
            version: Version(rng.any() as u32),
            seq: (rng.below(2) == 0).then(|| Seq(rng.any())),
            body: rng.bytes(64),
        }
    }

    #[cfg_attr(test, test)]
    fn an_entry_is_compact() {
        let e = Entry {
            id: EntryId { origin: Origin(7), n: 1 },
            space: Space::Shared,
            actor: Actor(3),
            at: Hlc {
                wall: 1_700_000_000_000,
                logical: 0,
            },
            cause: None,
            version: Version(1),
            seq: None,
            body: vec![4u8, 2],
        };
        let bytes = to_vec(&e);
        // origin 8, n 1, space 1, actor 1, wall 6, logical 1, cause 1,
        // version 1, seq 1, body 3.
        assert_eq!(bytes.len(), 24);
        assert_eq!(from_slice(&bytes), Ok(e));
    }

    #[cfg_attr(test, test)]
    fn a_bad_space_is_refused() {
        assert_eq!(from_slice::<Space>(&[4]), Err(Error::Tag));
        assert_eq!(from_slice::<Space>(&[3]), Ok(Space::Shared));
    }

    /// Random entries read back as themselves, singly, and as frames
    /// delivered a few bytes at a time; and encode again to the same bytes.
    #[cfg_attr(test, test)]
    fn random_entries_round_trip() {
        let mut rng = Rng::new(2);
        for _ in 0..50 {
            let entries: Vec<_> = (0..1 + rng.below(6)).map(|_| random(&mut rng)).collect();
            let mut wire = Vec::new();
            for e in &entries {
                let bytes = to_vec(e);
                let back: Entry<Vec<u8>> = from_slice(&bytes).unwrap();
                assert_eq!(&back, e);
                assert_eq!(to_vec(&back), bytes);
                put_frame(&mut wire, e);
            }
            let mut frames = Frames::new(1 << 12);
            let mut read = Vec::new();
            for part in wire.chunks(1 + rng.below(9) as usize) {
                frames.push(part);
                while let Some(e) = frames.read::<Entry<Vec<u8>>>().unwrap() {
                    read.push(e);
                }
            }
            assert_eq!(read, entries);
        }
    }

    #[cfg_attr(test, test)]
    fn the_clock_never_goes_back() {
        let t = Hlc { wall: 100, logical: 0 };
        assert_eq!(t.tick(200), Hlc { wall: 200, logical: 0 });
        // A wall clock that went back still moves the clock on.
        assert_eq!(t.tick(50), Hlc { wall: 100, logical: 1 });
        // An event seen from a device ahead is followed, not preceded.
        let seen = Hlc { wall: 500, logical: 4 };
        assert_eq!(t.witness(seen, 120), Hlc { wall: 500, logical: 5 });
        assert!(t.witness(seen, 120) > seen);
        assert_eq!(t.witness(seen, 600), Hlc { wall: 600, logical: 0 });
    }

    crate::listed!(
        an_entry_is_compact,
        a_bad_space_is_refused,
        random_entries_round_trip,
        the_clock_never_goes_back
    );
}
