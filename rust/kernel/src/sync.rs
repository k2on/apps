//! What a node and its upstream say to each other about the shared space,
//! and the records each keeps of it.
//!
//! ```text
//! Up     Hello { cursor, time } | Push(Entry) | Live(Entry) | Rate { table, every }
//!        | Subscribe { query, args } | Unsubscribe { query, args }
//! Down   Logged { entry, facts } | Verdict { id, verdict } | Live(Entry)
//!        | Fill { seq, tables, parts } | Moved(writes) | Got { query, args }
//!        | Welcome { sent, time }
//! Verdict  At(seq) | Refused(code)
//! Pending  { entry, writes, judged }
//! ```
//!
//! A node says `Hello` with the last `seq` it has applied, then pushes its
//! pending entries, oldest first, each as it is made and all of them again
//! at every connection. The upstream answers the hello with the log after
//! that cursor, an entry and its facts at a time, and each push with a
//! verdict: refused, or `At(seq)`, its log's head when it answered: the
//! entry, and every entry of its origin pushed before it, is in the log at
//! or below `seq`, or was refused (whose verdict came first). So a push
//! that arrives twice is judged once.
//!
//! **A fill.** An upstream keeps only the end of the log. A node whose
//! cursor is below what it keeps is answered with a `Fill`: the confirmed
//! rows at `seq`, in `parts` frames of `Moved` after it (so each frame stays
//! within what a watch reads at once), and then the log after `seq` as
//! usual. The node empties the `tables` it names (every table the log has
//! written) at the first part and takes `seq` as its cursor at the last.
//!
//! **Subscriptions.** A node that holds only what its queries read (a
//! watch) subscribes to each by its number and arguments, never by sending
//! a query; it says its subscriptions at each connection before its hello.
//! The upstream runs each and sends the rows its plan keeps of its table,
//! whole: its hello is answered with a fill of them, or, when the
//! upstream kept its record of what the node holds, with a fill that
//! empties nothing and carries only what changed since its cursor
//! ([`crate::Serve`]); a subscription after
//! it with the rows the node did not hold, in `Moved` parts, then `Got`,
//! which says the query's rows are all there. After that each logged entry
//! comes with the facts of the rows the node holds, and the rows that
//! moved into or out of its queries (a move-out is a removal), so a batch
//! lands at its one `seq`. An unsubscription is answered with the
//! move-outs of the rows no other query of the node holds. A node that
//! subscribes to nothing holds every row, and is sent every fact. A query
//! the upstream does not have is got at once, with no rows.
//!
//! **Atomic.** A node holds what moves, and a fill's parts, apart until
//! every query it wants is got (a fill: until its last part), then takes
//! them at once, so a view never shows half a subscription. An upstream
//! sends a subscription's `Moved` parts and its `Got` together, nothing
//! between them.
//!
//! **Session entries.** `Live` carries a session entry, which is forwarded
//! to its actor's nodes (the one that sent it too) and never logged, and
//! only while the link is up: live controls don't queue. A node states
//! with `Rate` how often it wants a session table's rows: between its
//! sends, only the latest entry for each row is held ([`Feed`]). A table
//! it names no rate for is sent every entry at once. A hello is answered
//! with the latest entry of every session row the node may see, its own
//! included, and the answer ends with `Welcome`.
//!
//! **Time.** A hello says the node's clock (`time`); `Welcome` says it
//! back (`sent`) with the tree's time at the upstream when it answered
//! (the root's clock, as the upstream estimates it), from which the node
//! takes its offset ([`crate::Node::offset`]).
//!
//! Ported from `rust/ark`'s `frames.rs` and `peer.rs` as far as pending
//! entries, facts and the rebase need: schemas, calls and files are for
//! later rounds.

use alloc::vec::Vec;

use crate::codec::{Decode, Encode, Error, Reader};
use crate::entry::{Entry, EntryId, Seq};
#[cfg(feature = "upstream")]
use crate::node::Body;
use crate::node::Refusal;
use crate::plan::Plan;
use crate::store::{TableId, Writes};

/// A query, by the number its build gives it: what a node subscribes to,
/// with its arguments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueryId(pub u32);

/// A query's plan for its arguments, over one table's rows, which it keeps
/// whole (a scan, its filters and its take): what a node that subscribes
/// holds, and its upstream runs. The node runs its own maps, sorts and
/// groups over them.
pub type Query = fn(args: &[u8]) -> (TableId, Plan);

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
    /// It and its origin's entries before it are judged, those not
    /// refused in the log at or below this `seq`.
    At(Seq),
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
    /// At each connection: the last `seq` this node has applied, and its
    /// clock's time, for the answer's [`Down::Welcome`] to say back.
    Hello {
        cursor: Seq,
        time: u64,
    },
    /// A pending entry, for the authority to judge.
    Push(Entry<B>),
    /// A session entry, forwarded and never logged.
    Live(Entry<B>),
    /// The least time in milliseconds this node wants between the session
    /// entries it is sent for `table`'s rows; 0 for each at once.
    Rate {
        table: TableId,
        every: u32,
    },
    /// Hold the rows this query reads, with these arguments.
    Subscribe {
        query: QueryId,
        args: Vec<u8>,
    },
    Unsubscribe {
        query: QueryId,
        args: Vec<u8>,
    },
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
    /// The confirmed rows at `seq` follow, in the next `parts` frames,
    /// each `Moved`, in place of the log up to it: the node's cursor is
    /// below what its upstream keeps. `tables` are emptied first.
    Fill { seq: Seq, tables: Vec<TableId>, parts: u32 },
    /// A fill's part; else rows that came into the node's queries, or moved
    /// out of them (a removal), at its cursor.
    Moved(Writes),
    /// A subscription's rows are all sent.
    Got { query: QueryId, args: Vec<u8> },
    /// The end of the answer to a hello: the time the hello said, and the
    /// tree's time at the upstream when it answered (the root's clock, as
    /// the upstream estimates it), for the node to measure its offset.
    Welcome { sent: u64, time: u64 },
}

#[cfg(feature = "upstream")]
/// What one node is sent of the session space: every entry at once, but
/// in a table it asked a rate for, only the latest entry for each row
/// ([`Body::row`]) is held between the table's turns, which come at most
/// once each `every` milliseconds. Time comes in as an argument.
pub struct Feed<B> {
    /// Per table: its rate, and when its next turn may come.
    rates: Vec<(TableId, u32, u64)>,
    /// At most one entry per row, in the order their rows were first held.
    held: Vec<Entry<B>>,
}

#[cfg(feature = "upstream")]
impl<B> Default for Feed<B> {
    fn default() -> Feed<B> {
        Feed {
            rates: Vec::new(),
            held: Vec::new(),
        }
    }
}

#[cfg(feature = "upstream")]
impl<B: Body> Feed<B> {
    /// Sets `table`'s rate: at most one turn each `every` milliseconds.
    pub fn rate(&mut self, table: TableId, every: u32) {
        self.rates.retain(|r| r.0 != table);
        if every > 0 {
            self.rates.push((table, every, 0));
        }
    }

    /// Takes an entry at `now`: what to send now, of it and of what was
    /// held.
    pub fn offer(&mut self, e: Entry<B>, now: u64) -> Vec<Entry<B>> {
        let row = e.body.row();
        let Some(row) = row.filter(|(table, _)| self.rates.iter().any(|r| r.0 == *table)) else {
            let mut out = self.due(now);
            out.push(e);
            return out;
        };
        match self.held.iter_mut().find(|h| h.body.row().as_ref() == Some(&row)) {
            Some(h) => *h = e,
            None => self.held.push(e),
        }
        self.due(now)
    }

    /// What is held of the tables whose turn has come by `now`.
    pub fn due(&mut self, now: u64) -> Vec<Entry<B>> {
        let mut out = Vec::new();
        for (table, every, next) in &mut self.rates {
            if *next > now || !self.held.iter().any(|h| h.body.row().is_some_and(|r| r.0 == *table)) {
                continue;
            }
            *next = now + u64::from(*every);
            let mut i = 0;
            while i < self.held.len() {
                if self.held[i].body.row().is_some_and(|r| r.0 == *table) {
                    out.push(self.held.remove(i));
                } else {
                    i += 1;
                }
            }
        }
        out
    }

    /// When the next held entry's turn comes, if one is held: for the host
    /// to call [`Feed::due`] then.
    pub fn next(&self) -> Option<u64> {
        self.rates
            .iter()
            .filter(|(table, ..)| self.held.iter().any(|h| h.body.row().is_some_and(|r| r.0 == *table)))
            .map(|r| r.2)
            .min()
    }
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
            Verdict::At(seq) => {
                0u32.encode(out);
                seq.encode(out);
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
            0 => Ok(Verdict::At(Seq::decode(r)?)),
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
            Up::Hello { cursor, time } => {
                0u32.encode(out);
                cursor.encode(out);
                time.encode(out);
            }
            Up::Push(e) => {
                1u32.encode(out);
                e.encode(out);
            }
            Up::Live(e) => {
                2u32.encode(out);
                e.encode(out);
            }
            Up::Rate { table, every } => {
                3u32.encode(out);
                table.encode(out);
                every.encode(out);
            }
            Up::Subscribe { query, args } => {
                4u32.encode(out);
                query.0.encode(out);
                args.encode(out);
            }
            Up::Unsubscribe { query, args } => {
                5u32.encode(out);
                query.0.encode(out);
                args.encode(out);
            }
        }
    }
}

impl<B: Decode> Decode for Up<B> {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        match u32::decode(r)? {
            0 => Ok(Up::Hello {
                cursor: Seq::decode(r)?,
                time: u64::decode(r)?,
            }),
            1 => Ok(Up::Push(Entry::decode(r)?)),
            2 => Ok(Up::Live(Entry::decode(r)?)),
            3 => Ok(Up::Rate {
                table: TableId::decode(r)?,
                every: u32::decode(r)?,
            }),
            4 => Ok(Up::Subscribe {
                query: QueryId(u32::decode(r)?),
                args: Vec::decode(r)?,
            }),
            5 => Ok(Up::Unsubscribe {
                query: QueryId(u32::decode(r)?),
                args: Vec::decode(r)?,
            }),
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
            Down::Fill { seq, tables, parts } => {
                3u32.encode(out);
                seq.encode(out);
                tables.encode(out);
                parts.encode(out);
            }
            Down::Moved(rows) => {
                4u32.encode(out);
                rows.encode(out);
            }
            Down::Got { query, args } => {
                5u32.encode(out);
                query.0.encode(out);
                args.encode(out);
            }
            Down::Welcome { sent, time } => {
                6u32.encode(out);
                sent.encode(out);
                time.encode(out);
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
            3 => Ok(Down::Fill {
                seq: Seq::decode(r)?,
                tables: Vec::decode(r)?,
                parts: u32::decode(r)?,
            }),
            4 => Ok(Down::Moved(Writes::decode(r)?)),
            5 => Ok(Down::Got {
                query: QueryId(u32::decode(r)?),
                args: Vec::decode(r)?,
            }),
            6 => Ok(Down::Welcome {
                sent: u64::decode(r)?,
                time: u64::decode(r)?,
            }),
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
        let ups = [
            Up::Hello {
                cursor: Seq(4),
                time: 1_700_000_000_123,
            },
            Up::Push(entry(1, None)),
            Up::Live(entry(2, None)),
            Up::Rate {
                table: TableId(2),
                every: 250,
            },
            Up::Subscribe {
                query: QueryId(3),
                args: vec![1, 2],
            },
            Up::Unsubscribe {
                query: QueryId(3),
                args: vec![],
            },
        ];
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
                verdict: Verdict::At(Seq(5)),
            },
            Down::Verdict {
                id: entry(2, None).id,
                verdict: Verdict::Refused(Refusal(300)),
            },
            Down::Live(entry(3, None)),
            Down::Fill {
                seq: Seq(9),
                tables: vec![TableId(1), TableId(3)],
                parts: 2,
            },
            Down::Moved(facts.clone()),
            Down::Got {
                query: QueryId(7),
                args: vec![9],
            },
            Down::Welcome {
                sent: 5,
                time: 1_700_000_000_456,
            },
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

    #[cfg(feature = "upstream")]
    /// A session table with a rate is sent at most once a turn, the
    /// latest entry for each row; other tables' entries go at once.
    #[cfg_attr(test, test)]
    fn a_feed_keeps_the_latest_per_row_between_turns() {
        use crate::node::app::{node, Msg, POSITIONS};
        let mut n = node(1);
        let mut set = |key, value| n.make(Space::Session, Msg::Set { key, value }, 0);
        let mut feed = Feed::default();
        let a1 = set(1, 10);
        assert_eq!(feed.offer(a1.clone(), 0), [a1]);
        feed.rate(POSITIONS, 100);
        // The first goes at once; the next two for row 1 within the turn
        // leave the latest; row 2's is held beside it.
        let (a2, a3, b1, a4) = (set(1, 11), set(1, 12), set(2, 20), set(1, 13));
        assert_eq!(feed.offer(a2.clone(), 5), [a2]);
        assert_eq!(feed.offer(a3, 20), []);
        assert_eq!(feed.offer(b1.clone(), 30), []);
        assert_eq!(feed.offer(a4.clone(), 40), []);
        assert_eq!(feed.next(), Some(105));
        assert_eq!(feed.due(104), []);
        assert_eq!(feed.due(105), [a4, b1]);
        assert_eq!(feed.next(), None);
        // An entry naming no row is never held.
        let tap = n.make(Space::Session, Msg::Seen, 0);
        assert_eq!(feed.offer(tap.clone(), 106), [tap]);
        // A rate of 0 sends each at once again.
        feed.rate(POSITIONS, 0);
        let a5 = n.make(Space::Session, Msg::Set { key: 1, value: 14 }, 0);
        assert_eq!(feed.offer(a5.clone(), 107), [a5]);
    }

    #[cfg(not(feature = "upstream"))]
    crate::listed!(frames_round_trip);
    #[cfg(feature = "upstream")]
    crate::listed!(frames_round_trip, a_feed_keeps_the_latest_per_row_between_turns);
}
