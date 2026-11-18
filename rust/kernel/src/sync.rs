//! What a node and its upstream say to each other about the shared space,
//! and the records each keeps of it.
//!
//! ```text
//! Up     Hello { cursor, time } | Push(Entry) | Live(Entry) | Rate { table, every }
//!        | Subscribe { query, args } | Unsubscribe { query, args }
//! Down   Logged { entry, facts } | Live(Entry)
//!        | Rows { rows, end } | Welcome { sent, time }
//! End    More | Moved | Fill { seq, whole, judged } | Got { query, args }
//! Pending  { entry, writes }
//! ```
//!
//! A node says `Hello` with the last `seq` it has applied, then pushes its
//! pending entries, oldest first, each as it is made and all of them again
//! at every connection. The upstream answers the hello with the log after
//! that cursor, an entry and its facts at a time. A push is not answered:
//! **its judgment is the log's.** Every entry judged is logged at its
//! `seq`, kept or refused, so each origin's entries come down in the order
//! of their count and a node lets go of each as it comes: its count, the
//! origin's **last judged**, is carried by every entry of the log, and by
//! a fill, which says it for each origin of the node's (`judged`). A push
//! that arrives twice is judged once, and a repeat is not answered.
//!
//! **Refusals are rows.** A refused entry is logged with one fact: a row
//! of [`REFUSALS`], keyed by the entry's id, saying why ([`Refused`]). It
//! goes only to the connection of the entry's origin, or of the node it
//! came through ([`Refused::via`]: a phone, for its watch), with the log
//! entry or in a fill, as every row does, so a dropped connection cannot
//! lose it: what a node missed comes with its next hello. A node takes its
//! own refusal rows as it applies them, journaling those of its pending
//! entries, and never keeps them; a node serving others keeps theirs. An
//! upstream lets go of a refusal row once its owner (or the node it came
//! through) says hello at or past its `seq`: it has applied it.
//!
//! **Rows.** Every change that is rows and not a logged entry's facts (a
//! fill, a subscription's rows, rows that moved) is one frame, `Rows`, cut
//! into parts so each frame stays within what a watch reads at once. Each
//! part but the last says `More`; the last says what they were, and until
//! it comes the node holds them apart.
//!
//! **A fill.** An upstream keeps only the end of the log. A node whose
//! cursor is below what it keeps is answered with a fill: the confirmed
//! rows at `seq`, ending `Fill { seq, whole }`, and then the log after
//! `seq` as usual. At it a whole fill replaces every row the node holds,
//! and the node takes `seq` as its cursor. An upstream sends a fill's
//! parts together, nothing between them.
//!
//! **Subscriptions.** A node that holds only what its queries read (a
//! watch) subscribes to each by its number and arguments, never by sending
//! a query; it says its subscriptions at each connection before its hello.
//! The upstream runs each and sends the rows its plan keeps of its table,
//! whole: its hello is answered with a fill of them, or, when the
//! upstream kept its record of what the node holds, with a fill that
//! empties nothing and carries only what changed since its cursor
//! ([`crate::Serve`]); a subscription after
//! it with the rows the node did not hold, ending `Got`, which says the
//! query's rows are all there. After that each logged entry
//! comes with the facts of the rows the node holds, and the rows that
//! moved into or out of its queries (a move-out is a removal), so a batch
//! lands at its one `seq`. An unsubscription is answered with the
//! move-outs of the rows no other query of the node holds. A node that
//! subscribes to nothing holds every row, and is sent every fact. A query
//! the upstream does not have is got at once, with no rows.
//!
//! **Atomic.** A node holds what moves, and a fill's parts, apart until
//! every query it wants is got (a fill: until its last part), then takes
//! them at once, so a view never shows half a subscription.
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
use crate::entry::{Entry, EntryId, Origin, Seq};
#[cfg(feature = "upstream")]
use crate::node::Body;
use crate::node::Refusal;
use crate::plan::Plan;
use crate::store::{TableId, Writes};

/// A query, by the number its build gives it: what a node subscribes to,
/// with its arguments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueryId(pub u32);

/// A query's plan for its arguments: what a node that subscribes holds the
/// base rows of ([`crate::Live::base`]), and its upstream runs. A scan, its
/// filters, maps and takes; the node runs its own sorts and groups.
pub type Query = fn(args: &[u8]) -> Plan;

/// An entry as the master log holds it: ordered (`entry.seq` is set) and
/// with the rows its run wrote, which every other node applies in place of
/// running it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Logged<B> {
    pub entry: Entry<B>,
    pub facts: Writes,
}

/// The table of refusals: a row per refused entry, keyed by its id
/// ([`Refused::key`]), until its owner has seen it.
pub const REFUSALS: TableId = TableId(u32::MAX - 1);

/// A refused entry's row ([`REFUSALS`]): why, whose, through which node,
/// and the `seq` its refusal was logged at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused {
    pub why: Refusal,
    /// The actor whose connection pushed it.
    pub actor: crate::entry::Actor,
    /// The node it was pushed through: its origin's, or a relay's.
    pub via: crate::entry::Origin,
    pub seq: Seq,
}

impl Refused {
    /// The row's key: the entry's origin and count, big-endian.
    pub fn key(id: EntryId) -> [u8; 16] {
        let mut k = [0; 16];
        k[..8].copy_from_slice(&id.origin.0.to_be_bytes());
        k[8..].copy_from_slice(&id.n.to_be_bytes());
        k
    }

    /// The entry a row's key names.
    pub fn id(key: &[u8]) -> Option<EntryId> {
        let (o, n) = (key.get(..8)?, key.get(8..16)?);
        Some(EntryId {
            origin: crate::entry::Origin(u64::from_be_bytes(o.try_into().ok()?)),
            n: u64::from_be_bytes(n.try_into().ok()?),
        })
    }
}

impl Encode for Refused {
    fn encode(&self, out: &mut Vec<u8>) {
        self.why.encode(out);
        self.actor.0.encode(out);
        self.via.0.encode(out);
        self.seq.encode(out);
    }
}

impl Decode for Refused {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Refused {
            why: Refusal::decode(r)?,
            actor: crate::entry::Actor(u64::decode(r)?),
            via: crate::entry::Origin(u64::decode(r)?),
            seq: Seq::decode(r)?,
        })
    }
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
    /// The next entry of the master log: kept, with its facts, or refused,
    /// with its refusal's row.
    Logged(Logged<B>),
    /// A session entry from another of the user's nodes.
    Live(Entry<B>),
    /// Rows, held by the node until `end` says what they were: one
    /// frame's worth of every change that is rows and not a logged entry's
    /// facts (a fill, a subscription's rows, rows that moved).
    Rows { rows: Writes, end: End },
    /// The end of the answer to a hello: the time the hello said, and the
    /// tree's time at the upstream when it answered (the root's clock, as
    /// the upstream estimates it), for the node to measure its offset.
    Welcome { sent: u64, time: u64 },
}

/// What a run of [`Down::Rows`] frames was, said by its last: until then
/// each says `More`, and its rows are held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// More of the same follow.
    More,
    /// Rows that came into the node's queries, or moved out of them (a
    /// removal), at its cursor.
    Moved,
    /// The confirmed rows at `seq`, in place of the log up to it: the
    /// node's cursor was below what its upstream keeps. When `whole`, they
    /// replace every row the node holds. With them, the last count judged
    /// of each origin the node pushes for: its entries up to it are in
    /// these rows, or refused (their rows among them).
    Fill { seq: Seq, whole: bool, judged: Vec<(Origin, u64)> },
    /// A subscription's rows, all sent.
    Got { query: QueryId, args: Vec<u8> },
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

impl<B: Encode> Encode for Pending<B> {
    fn encode(&self, out: &mut Vec<u8>) {
        self.entry.encode(out);
        self.writes.encode(out);
    }
}

impl<B: Decode> Decode for Pending<B> {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Pending {
            entry: Entry::decode(r)?,
            writes: Writes::decode(r)?,
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
            Down::Live(e) => {
                1u32.encode(out);
                e.encode(out);
            }
            Down::Rows { rows, end } => {
                2u32.encode(out);
                rows.encode(out);
                match end {
                    End::More => 0u32.encode(out),
                    End::Moved => 1u32.encode(out),
                    End::Fill { seq, whole, judged } => {
                        2u32.encode(out);
                        seq.encode(out);
                        whole.encode(out);
                        crate::codec::put_varint(out, judged.len() as u64);
                        for (o, n) in judged {
                            o.0.encode(out);
                            n.encode(out);
                        }
                    }
                    End::Got { query, args } => {
                        3u32.encode(out);
                        query.0.encode(out);
                        args.encode(out);
                    }
                }
            }
            Down::Welcome { sent, time } => {
                3u32.encode(out);
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
            1 => Ok(Down::Live(Entry::decode(r)?)),
            2 => Ok(Down::Rows {
                rows: Writes::decode(r)?,
                end: match u32::decode(r)? {
                    0 => End::More,
                    1 => End::Moved,
                    2 => End::Fill {
                        seq: Seq::decode(r)?,
                        whole: bool::decode(r)?,
                        judged: {
                            let n = r.count()?;
                            let mut judged = Vec::new();
                            for _ in 0..n {
                                judged.push((Origin(u64::decode(r)?), u64::decode(r)?));
                            }
                            judged
                        },
                    },
                    3 => End::Got {
                        query: QueryId(u32::decode(r)?),
                        args: Vec::decode(r)?,
                    },
                    _ => return Err(Error::Tag),
                },
            }),
            3 => Ok(Down::Welcome {
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
            Down::Live(entry(3, None)),
            Down::Rows {
                rows: facts.clone(),
                end: End::More,
            },
            Down::Rows {
                rows: facts.clone(),
                end: End::Moved,
            },
            Down::Rows {
                rows: Writes::default(),
                end: End::Fill {
                    seq: Seq(9),
                    whole: true,
                    judged: vec![(Origin(9), 4), (Origin(12), 1)],
                },
            },
            Down::Rows {
                rows: facts.clone(),
                end: End::Got {
                    query: QueryId(7),
                    args: vec![9],
                },
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
        };
        assert_eq!(from_slice::<Pending<Vec<u8>>>(&to_vec(&p)), Ok(p));
        let r = Refused {
            why: Refusal(300),
            actor: Actor(2),
            via: Origin(7),
            seq: Seq(3),
        };
        assert_eq!(from_slice::<Refused>(&to_vec(&r)), Ok(r));
        let id = entry(4, None).id;
        assert_eq!(Refused::id(&Refused::key(id)), Some(id));
        assert_eq!(from_slice::<Down<Vec<u8>>>(&[4]), Err(Error::Tag));
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
