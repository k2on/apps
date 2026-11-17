//! What a node and its upstream say to each other about the shared space,
//! and the records each keeps of it.
//!
//! ```text
//! Up     Hello { cursor } | Push(Entry) | Live(Entry)
//! Down   Logged { entry, facts } | Verdict { id, verdict } | Live(Entry)
//! Verdict  At { seq, last } | Refused(code)
//! Pending  { entry, writes, judged }
//! ```
//!
//! A node says `Hello` with the last `seq` it has applied, then pushes its
//! pending entries, oldest first, each as it is made and all of them again
//! at every connection. The upstream answers the hello with the log after
//! that cursor, an entry and its facts at a time, and each push with a
//! verdict: refused, or where the entry's origin stands, `At { seq, last }`:
//! the last count of that origin the authority has judged, and its log's
//! head when it answered. An entry of that origin at or below `last` is in
//! the log at or below `seq`, or was refused; one past it was not judged.
//! So a push that arrives twice is judged once.
//!
//! `Live` carries a session entry, which is forwarded and never logged.
//! Ported from `rust/ark`'s `frames.rs` and `peer.rs` as far as pending
//! entries, facts and the rebase need: snapshots, fills, schemas, calls and
//! files are for later rounds.

use alloc::vec::Vec;

use crate::codec::{Decode, Encode, Error, Reader};
use crate::entry::{Entry, EntryId, Seq};
use crate::node::Refusal;
use crate::store::Writes;

/// An entry as the master log holds it: ordered (`entry.seq` is set) and
/// with the rows its run wrote, which every other node applies in place of
/// running it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Logged<B> {
    pub entry: Entry<B>,
    pub facts: Writes,
}

/// The authority's answer to one pushed entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Its origin's entries up to `last` are judged, those not refused in
    /// the log at or below `seq`.
    At {
        seq: Seq,
        last: u64,
    },
    Refused(Refusal),
}

/// An entry this node made in the shared space and the server has not
/// confirmed, kept as a record: it survives a restart and is put back with
/// its writes, not run again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending<B> {
    pub entry: Entry<B>,
    /// What its last run here wrote over the confirmed rows and the
    /// pending entries before it; none if that run refused.
    pub writes: Writes,
    /// Judged by the authority at or below this `seq`, by an `At` verdict:
    /// it is let go once this node has applied the log that far.
    pub judged: Option<Seq>,
}

/// From a node to its upstream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Up<B> {
    /// At each connection: the last `seq` this node has applied.
    Hello { cursor: Seq },
    /// A pending entry, for the authority to judge.
    Push(Entry<B>),
    /// A session entry, forwarded and never logged.
    Live(Entry<B>),
}

/// From an upstream to a node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Down<B> {
    /// The next entry of the master log.
    Logged(Logged<B>),
    /// The answer to a push.
    Verdict { id: EntryId, verdict: Verdict },
    /// A session entry from another of the user's nodes.
    Live(Entry<B>),
}

impl Encode for Refusal {
    fn encode(&self, out: &mut Vec<u8>) {
        self.0.encode(out);
    }
}

impl Decode for Refusal {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Refusal(u16::decode(r)?))
    }
}

impl<B: Encode> Encode for Logged<B> {
    fn encode(&self, out: &mut Vec<u8>) {
        self.entry.encode(out);
        self.facts.encode(out);
    }
}

impl<B: Decode> Decode for Logged<B> {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Logged {
            entry: Entry::decode(r)?,
            facts: Writes::decode(r)?,
        })
    }
}

impl Encode for Verdict {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Verdict::At { seq, last } => {
                0u32.encode(out);
                seq.encode(out);
                last.encode(out);
            }
            Verdict::Refused(why) => {
                1u32.encode(out);
                why.encode(out);
            }
        }
    }
}

impl Decode for Verdict {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        match u32::decode(r)? {
            0 => Ok(Verdict::At {
                seq: Seq::decode(r)?,
                last: u64::decode(r)?,
            }),
            1 => Ok(Verdict::Refused(Refusal::decode(r)?)),
            _ => Err(Error::Tag),
        }
    }
}

impl<B: Encode> Encode for Pending<B> {
    fn encode(&self, out: &mut Vec<u8>) {
        self.entry.encode(out);
        self.writes.encode(out);
        self.judged.encode(out);
    }
}

impl<B: Decode> Decode for Pending<B> {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Pending {
            entry: Entry::decode(r)?,
            writes: Writes::decode(r)?,
            judged: Option::decode(r)?,
        })
    }
}

impl<B: Encode> Encode for Up<B> {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Up::Hello { cursor } => {
                0u32.encode(out);
                cursor.encode(out);
            }
            Up::Push(e) => {
                1u32.encode(out);
                e.encode(out);
            }
            Up::Live(e) => {
                2u32.encode(out);
                e.encode(out);
            }
        }
    }
}

impl<B: Decode> Decode for Up<B> {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        match u32::decode(r)? {
            0 => Ok(Up::Hello { cursor: Seq::decode(r)? }),
            1 => Ok(Up::Push(Entry::decode(r)?)),
            2 => Ok(Up::Live(Entry::decode(r)?)),
            _ => Err(Error::Tag),
        }
    }
}

impl<B: Encode> Encode for Down<B> {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Down::Logged(l) => {
                0u32.encode(out);
                l.encode(out);
            }
            Down::Verdict { id, verdict } => {
                1u32.encode(out);
                id.encode(out);
                verdict.encode(out);
            }
            Down::Live(e) => {
                2u32.encode(out);
                e.encode(out);
            }
        }
    }
}

impl<B: Decode> Decode for Down<B> {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        match u32::decode(r)? {
            0 => Ok(Down::Logged(Logged::decode(r)?)),
            1 => Ok(Down::Verdict {
                id: EntryId::decode(r)?,
                verdict: Verdict::decode(r)?,
            }),
            2 => Ok(Down::Live(Entry::decode(r)?)),
            _ => Err(Error::Tag),
        }
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::codec::{from_slice, to_vec};
    use crate::entry::{Actor, Hlc, Origin, Space, Version};
    use crate::store::{TableId, Write};
    use alloc::vec;

    fn entry(n: u64, seq: Option<Seq>) -> Entry<Vec<u8>> {
        Entry {
            id: EntryId { origin: Origin(9), n },
            space: Space::Shared,
            actor: Actor(2),
            at: Hlc { wall: 5, logical: 1 },
            cause: None,
            version: Version(1),
            seq,
            body: vec![1, 2],
        }
    }

    #[cfg_attr(test, test)]
    fn frames_round_trip() {
        let facts = Writes(vec![
            Write {
                table: TableId(3),
                key: vec![1],
                value: Some(vec![7]),
            },
            Write {
                table: TableId(3),
                key: vec![2],
                value: None,
            },
        ]);
        let ups = [Up::Hello { cursor: Seq(4) }, Up::Push(entry(1, None)), Up::Live(entry(2, None))];
        for up in ups {
            assert_eq!(from_slice::<Up<Vec<u8>>>(&to_vec(&up)), Ok(up));
        }
        let downs = [
            Down::Logged(Logged {
                entry: entry(1, Some(Seq(5))),
                facts: facts.clone(),
            }),
            Down::Verdict {
                id: entry(1, None).id,
                verdict: Verdict::At { seq: Seq(5), last: 1 },
            },
            Down::Verdict {
                id: entry(2, None).id,
                verdict: Verdict::Refused(Refusal(300)),
            },
            Down::Live(entry(3, None)),
        ];
        for down in downs {
            assert_eq!(from_slice::<Down<Vec<u8>>>(&to_vec(&down)), Ok(down));
        }
        let p = Pending {
            entry: entry(4, None),
            writes: facts,
            judged: Some(Seq(2)),
        };
        assert_eq!(from_slice::<Pending<Vec<u8>>>(&to_vec(&p)), Ok(p));
        assert_eq!(from_slice::<Verdict>(&[2]), Err(Error::Tag));
    }

    crate::listed!(frames_round_trip);
}
