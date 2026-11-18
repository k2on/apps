//! Serving downstream nodes: what any upstream keeps of the nodes that
//! connect to it and answers them with. The server's [`crate::Authority`]
//! serves its devices with it, and a phone's [`crate::Relay`] its watch,
//! over the phone's own view. A watch never links it.
//!
//! **Connections and records.** The host opens a connection as the actor
//! it signed in, and names the client it is ([`Serve::open`]), by the
//! origin it knows the device by. Of a client that subscribes, the record
//! of what it holds outlives the connection: kept for a while once it
//! closes ([`Serve::set_ttl`]), and taken up again when the client
//! connects again, so its hello is answered with only what changed since
//! its cursor, as Zero's per-client records do, in place of every row.
//!
//! A record keeps the client's subscriptions, each a [`Live`] answer over
//! the upstream's rows, kept current while it is away, and the rows it
//! holds: for each, how many of its subscriptions read it and the version
//! at which it was last sent, or that it is still to send. A row no
//! subscription reads any longer stays as a removal to send until the
//! client is known to have it. Never the rows' contents.
//!
//! **Versions.** The upstream's link is ordered, and a client's cursor
//! moves only with the log's entries and a fill's end. So a row sent with
//! the entry at `seq` `s` (version `2s`), or in a fill to `s` from a
//! cursor below it (also `2s`), has reached a client at a cursor of `s` or
//! more; one sent outside the log at head `h`, a move or a fill from `h`
//! itself (version `2h + 1`), only one past `h`, as the client cannot tell
//! it from what it had at `h`.
//!
//! **A hello.** A client whose record was kept, which has had the last
//! fill of every row the record sent it (its cursor says so, as for any
//! row) and is not past the head, is sent the rows still to send and those
//! sent at a version above twice its cursor, each whole or as a removal,
//! as a fill that empties nothing ([`Down::Fill`] with no tables); any
//! other is sent every row it holds, as a fill that empties its queries'
//! tables. Then `Got` for each subscription it said.
//!
//! **A query let go** stays held for the query TTL: its rows stay, it
//! is kept current, and the client subscribing again is told `Got` at
//! once. A subscription the client does not say again at its next
//! connection is let go the same way. When the TTL ends ([`Serve::tick`])
//! its rows that no other query reads move out.
//!
//! **Session entries.** Of an entry that sets a row whole
//! ([`crate::Body::row`]) the latest is kept per actor, by its time: one
//! older than it is dropped, and a hello is answered with its actor's
//! latest (not those of the client's own origin). Each goes to its actor's
//! other connections through each one's [`Feed`], at the rate it asked.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::entry::{Actor, Entry, Origin, Seq};
use crate::node::{Body, Refusal};
use crate::plan::{Changes, Diff, Live, Plan};
use crate::store::{Read, TableId, Write, Writes};
use crate::sync::{Down, Feed, Logged, QueryId};

/// A connection, by the host's number for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Conn(pub u32);

/// A frame for a connection.
pub type Out<B> = (Conn, Down<B>);

/// A query's plan for its arguments, over one table's rows, which it keeps
/// whole (a scan, its filters and its take): what a node that subscribes
/// holds. The node runs its own maps, sorts and groups over them.
pub type Query = fn(args: &[u8]) -> (TableId, Plan);

/// A row of the shared space.
pub(crate) type At = (TableId, Vec<u8>);

/// A row's `sent` while it is still to send.
const UNSENT: u64 = u64::MAX;

/// The version of a row sent with the log's entry at `seq`.
fn logged(seq: Seq) -> u64 {
    2 * seq.0
}

/// The version of a row sent outside the log at head `head`.
fn moved(head: Seq) -> u64 {
    2 * head.0 + 1
}
/// A subscription's `until` while the client has not said it again at
/// this connection.
const UNSAID: u64 = u64::MAX;

/// A subscription: its query and arguments, its answer kept live, and
/// when it is let go, once the client has let go of it.
struct Sub {
    query: QueryId,
    args: Vec<u8>,
    table: TableId,
    live: Live,
    until: Option<u64>,
}

/// A row a client holds: how many of its subscriptions read it (none: a
/// removal still to be known), and the version at which it was last sent.
#[derive(Clone, Copy, Debug)]
struct Held {
    count: u32,
    sent: u64,
}

/// What an upstream keeps of a client: its subscriptions, the rows it
/// holds (`None` for every row), and the version of the last fill that
/// sent it every row, which a delta needs it to have had.
struct Record {
    subs: Vec<Sub>,
    held: Option<BTreeMap<At, Held>>,
    filled: u64,
}

impl Record {
    /// One more subscription reads each of `rows`: those that come into
    /// the client's hold, sent now at version `send`, or marked to send.
    fn hold<'r>(&mut self, table: TableId, rows: impl Iterator<Item = &'r Vec<u8>>, db: &dyn Read, send: Option<u64>) -> Vec<Write> {
        let mut out = Vec::new();
        let Some(held) = &mut self.held else { return out };
        for key in rows {
            let h = held.entry((table, key.clone())).or_insert(Held { count: 0, sent: UNSENT });
            h.count += 1;
            if h.count == 1 {
                h.sent = send.unwrap_or(UNSENT);
                if send.is_some() {
                    out.push(Write {
                        table,
                        key: key.clone(),
                        value: db.get(table, key),
                    });
                }
            }
        }
        out
    }

    /// One fewer subscription reads each of a subscription's rows: those
    /// that leave the client's hold move out.
    fn release(&mut self, sub: &Sub, send: Option<u64>) -> Vec<Write> {
        let mut out = Vec::new();
        let Some(held) = &mut self.held else { return out };
        for (key, _) in sub.live.rows() {
            let Some(h) = held.get_mut(&(sub.table, key.clone())) else { continue };
            h.count -= 1;
            if h.count == 0 {
                h.sent = send.unwrap_or(UNSENT);
                if send.is_some() {
                    out.push(Write {
                        table: sub.table,
                        key: key.clone(),
                        value: None,
                    });
                }
            }
        }
        out
    }

    /// Marks every row it holds to send: what it holds may all have
    /// changed.
    fn unsend(&mut self) {
        for h in self.held.iter_mut().flat_map(|h| h.values_mut()) {
            if h.count > 0 {
                h.sent = UNSENT;
            }
        }
    }

    /// The rows still to send, each whole or a removal, sent now at
    /// `version`.
    fn unsent(&mut self, db: &dyn Read, version: u64) -> Vec<Write> {
        let mut out = Vec::new();
        for ((table, key), h) in self.held.iter_mut().flat_map(|h| h.iter_mut()) {
            if h.sent == UNSENT {
                h.sent = version;
                let value = if h.count > 0 { db.get(*table, key) } else { None };
                out.push(Write {
                    table: *table,
                    key: key.clone(),
                    value,
                });
            }
        }
        out
    }

    /// Lets go of the subscriptions whose time has come by `now`.
    fn expire(&mut self, now: u64, send: Option<u64>) -> Vec<Write> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.subs.len() {
            if self.subs[i].until.is_some_and(|t| t <= now) {
                let sub = self.subs.remove(i);
                out.extend(self.release(&sub, send));
            } else {
                i += 1;
            }
        }
        out
    }

    /// Takes one settle of the upstream's rows (`db` after it): counts
    /// the rows that moved into or out of its subscriptions, and answers
    /// the rows held before or now that changed or moved, each whole or a
    /// removal, sent now at version `send`, or marked to send.
    fn settle(&mut self, db: &dyn Read, changes: &Changes, send: Option<u64>) -> Vec<Write> {
        let mut out = Vec::new();
        // Whether each row was held before the settle.
        let mut was: BTreeMap<At, bool> = BTreeMap::new();
        for sub in &mut self.subs {
            for Diff { key, old, new } in sub.live.diffs(db, changes) {
                let (Some(held), true) = (&mut self.held, old.is_some() != new.is_some()) else {
                    continue;
                };
                let at = (sub.table, key);
                was.entry(at.clone()).or_insert_with(|| held.get(&at).is_some_and(|h| h.count > 0));
                let h = held.entry(at).or_insert(Held { count: 0, sent: UNSENT });
                if new.is_some() {
                    h.count += 1;
                } else {
                    h.count -= 1;
                }
            }
        }
        let Some(held) = &mut self.held else { return out };
        for (table, d) in &changes.rows {
            let at = (*table, d.key.clone());
            if let Some(h) = held.get(&at) {
                let before = h.count > 0;
                was.entry(at).or_insert(before);
            }
        }
        for ((table, key), before) in was {
            let Some(h) = held.get_mut(&(table, key.clone())) else { continue };
            let now = h.count > 0;
            if !now && !before {
                continue;
            }
            h.sent = send.unwrap_or(UNSENT);
            if send.is_some() {
                let value = if now { db.get(table, &key) } else { None };
                out.push(Write { table, key, value });
            }
        }
        out
    }
}

/// What an upstream keeps of a connection.
struct Peer<B> {
    actor: Actor,
    client: Option<Origin>,
    feed: Feed<B>,
    hello: bool,
    /// The subscriptions it said before its hello, each told `Got` then.
    said: Vec<(QueryId, Vec<u8>)>,
    record: Record,
}

/// The connections an upstream serves, the records of what each client
/// holds, and the session entries it passes between them.
pub struct Serve<B> {
    conns: BTreeMap<Conn, Peer<B>>,
    /// The records of clients not connected, each with when it is let go.
    kept: BTreeMap<Origin, (Record, u64)>,
    /// The latest session entry for each row, by its actor.
    latest: BTreeMap<(Actor, At), Entry<B>>,
    queries: &'static [Query],
    /// The most rows a part carries.
    part: usize,
    /// How long a query let go stays held, and a record of a client gone.
    ttl: u64,
    keep: u64,
    /// Whether a connection that subscribes to nothing holds every row.
    all: bool,
}

impl<B: Body + Clone> Serve<B> {
    /// Serves no one yet. With `all`, a connection that says no
    /// subscription before its hello holds every row (the authority's,
    /// which answers it from its log); without, it holds none.
    pub fn new(all: bool) -> Serve<B> {
        Serve {
            conns: BTreeMap::new(),
            kept: BTreeMap::new(),
            latest: BTreeMap::new(),
            queries: &[],
            part: 32,
            ttl: 0,
            keep: 0,
            all,
        }
    }

    /// Sets the queries nodes subscribe to, by their numbers.
    pub fn set_queries(&mut self, queries: &'static [Query]) {
        self.queries = queries;
    }

    /// Sets the most rows a part carries, so that each frame fits what
    /// the node reads at once.
    pub fn set_part(&mut self, rows: usize) {
        self.part = rows.max(1);
    }

    /// The most rows a part carries.
    pub fn part(&self) -> usize {
        self.part
    }

    /// Sets how long, in milliseconds, a query let go stays held, and how
    /// long the record of a client gone is kept. Both 0 (the default):
    /// let go at once.
    pub fn set_ttl(&mut self, query: u64, record: u64) {
        self.ttl = query;
        self.keep = record;
    }

    /// A connection opened, signed in as `actor`, from `client` if the
    /// host knows which it is: the client's record, if kept, is taken up,
    /// from a connection of the same client still open (which is closed)
    /// or from those kept.
    pub fn open(&mut self, conn: Conn, actor: Actor, client: Option<Origin>) {
        let mut record = None;
        if let Some(c) = client {
            let old = self.conn(c).and_then(|old| self.conns.remove(&old));
            record = old.map(|p| p.record).filter(|r| r.held.is_some());
            record = record.or_else(|| self.kept.remove(&c).map(|(r, _)| r));
        }
        let mut record = record.unwrap_or(Record {
            subs: Vec::new(),
            held: Some(BTreeMap::new()),
            filled: UNSENT,
        });
        for sub in &mut record.subs {
            sub.until.get_or_insert(UNSAID);
        }
        let peer = Peer {
            actor,
            client,
            feed: Feed::default(),
            hello: false,
            said: Vec::new(),
            record,
        };
        self.conns.insert(conn, peer);
    }

    /// A connection closed at `now`: what was held for it is dropped, and
    /// its client's record kept, if it subscribes, for the record TTL.
    pub fn close(&mut self, conn: Conn, now: u64) {
        let Some(peer) = self.conns.remove(&conn) else { return };
        if let (Some(client), Some(_), true) = (peer.client, &peer.record.held, self.keep > 0) {
            self.kept.insert(client, (peer.record, now.saturating_add(self.keep)));
        }
    }

    /// The actor a connection is signed in as, if it is open.
    pub fn actor(&self, conn: Conn) -> Option<Actor> {
        self.conns.get(&conn).map(|p| p.actor)
    }

    /// The open connection of a client.
    pub fn conn(&self, client: Origin) -> Option<Conn> {
        self.conns.iter().find(|(_, p)| p.client == Some(client)).map(|(&c, _)| c)
    }

    /// Rows, in `Moved` parts.
    fn parts(&self, rows: Vec<Write>) -> Vec<Down<B>> {
        rows.chunks(self.part).map(|c| Down::Moved(Writes(c.to_vec()))).collect()
    }

    /// `conn` subscribes to a query over `db`, the upstream's rows at
    /// `head`: after its hello, it is sent the rows it did not hold, then
    /// `Got`. One the build does not have, or one it let go and still
    /// held, is got at once.
    pub fn subscribe(&mut self, conn: Conn, query: QueryId, args: Vec<u8>, db: &dyn Read, head: Seq) -> Vec<Down<B>> {
        let Some(peer) = self.conns.get_mut(&conn) else {
            return Vec::new();
        };
        let hello = peer.hello;
        if !hello && !peer.said.iter().any(|s| s.0 == query && s.1 == args) {
            peer.said.push((query, args.clone()));
        }
        let got = |rows: Vec<Down<B>>| {
            let mut out = rows;
            if hello {
                out.push(Down::Got { query, args: args.clone() });
            }
            out
        };
        if let Some(sub) = peer.record.subs.iter_mut().find(|s| s.query == query && s.args == args) {
            sub.until = None;
            return got(Vec::new());
        }
        let Some(make) = self.queries.get(query.0 as usize) else {
            return got(Vec::new());
        };
        let (table, plan) = make(&args);
        let live = plan.live(db);
        let came = peer
            .record
            .hold(table, live.rows().iter().map(|(k, _)| k), db, hello.then(|| moved(head)));
        peer.record.subs.push(Sub {
            query,
            args: args.clone(),
            table,
            live,
            until: None,
        });
        let rows = self.parts(came);
        got(rows)
    }

    /// `conn` lets go of a subscription at `now`: held for the query TTL,
    /// or its rows no other query reads moved out at once.
    pub fn unsubscribe(&mut self, conn: Conn, query: QueryId, args: Vec<u8>, head: Seq, now: u64) -> Vec<Down<B>> {
        let Some(peer) = self.conns.get_mut(&conn) else {
            return Vec::new();
        };
        peer.said.retain(|s| s.0 != query || s.1 != args);
        let Some(sub) = peer.record.subs.iter_mut().find(|s| s.query == query && s.args == args) else {
            return Vec::new();
        };
        sub.until = Some(now.saturating_add(self.ttl));
        let gone = peer.record.expire(now, peer.hello.then(|| moved(head)));
        self.parts(gone)
    }

    /// `conn` states a session table's rate.
    pub fn rate(&mut self, conn: Conn, table: TableId, every: u32) {
        if let Some(peer) = self.conns.get_mut(&conn) {
            peer.feed.rate(table, every);
        }
    }

    /// `conn` says hello at `cursor`, over `db`, the upstream's rows at
    /// `head`: whether it holds every row (and the upstream answers it
    /// from its log), and the frames that answer it besides: a fill of the
    /// rows it holds and `Got` for each subscription it said, unless it
    /// holds every row; then its actor's latest session entries.
    pub fn hello(&mut self, conn: Conn, cursor: Seq, db: &dyn Read, head: Seq, now: u64) -> (bool, Vec<Down<B>>) {
        let Some(peer) = self.conns.get_mut(&conn) else {
            return (false, Vec::new());
        };
        peer.hello = true;
        let record = &mut peer.record;
        // What it did not say again is let go as if it had said so now.
        for sub in &mut record.subs {
            if sub.until == Some(UNSAID) {
                sub.until = Some(now.saturating_add(self.ttl));
            }
        }
        record.expire(now, None);
        let all = self.all && peer.said.is_empty() && record.subs.is_empty() && record.held.as_ref().is_none_or(|h| h.is_empty());
        let mut out = Vec::new();
        if all {
            record.held = None;
        } else {
            let held = record.held.get_or_insert_with(BTreeMap::new);
            let delta = cursor <= head && record.filled <= logged(cursor);
            let version = if cursor < head { logged(head) } else { moved(head) };
            let mut rows = Vec::new();
            held.retain(|(table, key), h| {
                let send = if delta { h.sent > logged(cursor) } else { h.count > 0 };
                if send {
                    let value = if h.count > 0 { db.get(*table, key) } else { None };
                    rows.push(Write {
                        table: *table,
                        key: key.clone(),
                        value,
                    });
                    h.sent = version;
                }
                h.count > 0 || (delta && send)
            });
            if !delta {
                record.filled = version;
            }
            let mut tables: Vec<TableId> = if delta {
                Vec::new()
            } else {
                record.subs.iter().map(|s| s.table).collect()
            };
            tables.sort();
            tables.dedup();
            let parts: Vec<Down<B>> = rows.chunks(self.part).map(|c| Down::Rows(Writes(c.to_vec()))).collect();
            out.push(Down::Fill {
                seq: head,
                tables,
                parts: parts.len() as u32,
            });
            out.extend(parts);
            out.extend(peer.said.iter().map(|(query, args)| Down::Got {
                query: *query,
                args: args.clone(),
            }));
        }
        let (actor, client) = (peer.actor, peer.client);
        let live = self.latest.iter().filter(|((a, _), e)| *a == actor && Some(e.id.origin) != client);
        out.extend(live.map(|(_, e)| Down::Live(e.clone())));
        (all, out)
    }

    /// One settle of the upstream's rows (`db` after it, at `head`), and
    /// the logged entry it was, if one: to each connection that has said
    /// hello, that entry with the facts of the rows it holds and the rows
    /// that moved into or out of its queries (every fact, to one that
    /// holds every row); with no entry, those rows as `Moved`. The records
    /// of clients away, and of connections yet to say hello (which their
    /// hello answers), mark them to send.
    pub fn feed(&mut self, db: &dyn Read, changes: &Changes, entry: Option<&Logged<B>>, head: Seq) -> Vec<Out<B>> {
        let mut out = Vec::new();
        let part = self.part;
        for (&c, peer) in &mut self.conns {
            let version = if entry.is_some() { logged(head) } else { moved(head) };
            let rows = peer.record.settle(db, changes, peer.hello.then_some(version));
            if !peer.hello {
                continue;
            }
            match entry {
                Some(l) => {
                    let facts = if peer.record.held.is_none() { l.facts.clone() } else { Writes(rows) };
                    let entry = l.entry.clone();
                    out.push((c, Down::Logged(Logged { entry, facts })));
                }
                None => out.extend(rows.chunks(part).map(|r| (c, Down::Moved(Writes(r.to_vec()))))),
            }
        }
        for (record, _) in self.kept.values_mut() {
            record.settle(db, changes, None);
        }
        out
    }

    /// The upstream's rows changed whole (`changes` is a reset: a fill of
    /// its own), from `from` to `head`: each connection that has said
    /// hello is sent every row it holds and the removals of those it no
    /// longer does, as a fill to `head` that empties nothing; the records
    /// of the rest mark them to send.
    pub fn refill(&mut self, db: &dyn Read, changes: &Changes, from: Seq, head: Seq) -> Vec<Out<B>> {
        let version = if from < head { logged(head) } else { moved(head) };
        let mut out = Vec::new();
        let part = self.part;
        for (&c, peer) in &mut self.conns {
            peer.record.settle(db, changes, None);
            peer.record.unsend();
            if !peer.hello {
                continue;
            }
            let rows = peer.record.unsent(db, version);
            let parts: Vec<Writes> = rows.chunks(part).map(|r| Writes(r.to_vec())).collect();
            out.push((
                c,
                Down::Fill {
                    seq: head,
                    tables: Vec::new(),
                    parts: parts.len() as u32,
                },
            ));
            out.extend(parts.into_iter().map(|r| (c, Down::Rows(r))));
        }
        for (record, _) in self.kept.values_mut() {
            record.settle(db, changes, None);
            record.unsend();
        }
        out
    }

    /// A session entry `actor` sent from `from` (or the upstream's own,
    /// or its upstream's: `None`) at `now`: what goes to the actor's other
    /// connections, or `None` if it is older than the latest kept for its
    /// row, and dropped.
    pub fn live(&mut self, from: Option<Conn>, actor: Actor, e: Entry<B>, now: u64) -> Option<Vec<Out<B>>> {
        if let Some(row) = e.body.row() {
            let key = (actor, row);
            if self.latest.get(&key).is_some_and(|l| (l.at, l.id) >= (e.at, e.id)) {
                return None;
            }
            self.latest.insert(key, e.clone());
        }
        let mut out = Vec::new();
        for (&c, peer) in &mut self.conns {
            if Some(c) != from && peer.actor == actor {
                out.extend(peer.feed.offer(e.clone(), now).into_iter().map(|e| (c, Down::Live(e))));
            }
        }
        Some(out)
    }

    /// The latest session entry of each row, as kept.
    pub fn latest(&self) -> impl Iterator<Item = &Entry<B>> {
        self.latest.values()
    }

    /// What is due by `now`, the upstream at `head`: the session entries
    /// whose turn has come, and the move-outs of queries whose TTL has
    /// ended; records kept past theirs are let go.
    pub fn tick(&mut self, now: u64, head: Seq) -> Vec<Out<B>> {
        let mut out = Vec::new();
        let part = self.part;
        for (&c, peer) in &mut self.conns {
            let gone = peer.record.expire(now, peer.hello.then(|| moved(head)));
            out.extend(gone.chunks(part).map(|r| (c, Down::Moved(Writes(r.to_vec())))));
            out.extend(peer.feed.due(now).into_iter().map(|e| (c, Down::Live(e))));
        }
        self.kept.retain(|_, (record, until)| {
            record.expire(now, None);
            *until > now
        });
        out
    }

    /// When [`Serve::tick`] next has something to do, if ever.
    pub fn next_tick(&self) -> Option<u64> {
        let until = |r: &Record| r.subs.iter().filter_map(|s| s.until.filter(|&t| t != UNSAID)).min();
        let conns = self.conns.values().flat_map(|p| [p.feed.next(), until(&p.record)]);
        let kept = self.kept.values().flat_map(|(r, t)| [Some(*t), until(r)]);
        conns.chain(kept).flatten().min()
    }

    /// How many records of clients away are kept: for the host to see.
    pub fn kept(&self) -> usize {
        self.kept.len()
    }
}

/// `e` as from `actor`, its connection's: stamped if it names no one,
/// refused if it names another.
pub(crate) fn stamped<B>(actor: Actor, mut e: Entry<B>) -> Result<Entry<B>, Refusal> {
    if e.actor == Actor::NONE {
        e.actor = actor;
    }
    if e.actor == actor {
        Ok(e)
    } else {
        Err(Refusal::NOT_YOURS)
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::authority::tests::{ME, QUERIES};
    use crate::authority::Authority;
    use crate::entry::Space;
    use crate::node::app::*;
    use crate::node::Node;
    use crate::store::Memory;
    use crate::sync::Up;
    use alloc::vec;

    type Server = Authority<Msg, Memory>;

    /// A server with rows 1 to 4 written from a connection that holds
    /// every row, and a watch subscribed to the odd counts, filled.
    fn served(ttl: u64, keep: u64) -> (Server, Node<Msg, Memory>, Node<Msg, Memory>) {
        let mut a = Authority::new(&HANDLERS, Memory::new());
        a.set_queries(&QUERIES);
        a.set_ttl(ttl, keep);
        a.open(Conn(1), ME, Some(Origin(1)));
        a.receive(Conn(1), Up::Hello { cursor: Seq(0) }, 0);
        let mut writer = node(1);
        for (key, by) in [(1, 1), (2, 2), (3, 3), (4, 5)] {
            push(&mut a, &mut writer, key, by);
        }
        let mut watch = node(2);
        watch.subscribe(QueryId(0), vec![1], &mut Vec::new());
        connect(&mut a, &mut watch, 2, 0);
        (a, writer, watch)
    }

    fn push(a: &mut Server, writer: &mut Node<Msg, Memory>, key: u8, by: u64) -> Vec<Out<Msg>> {
        let e = writer.make(Space::Shared, Msg::Add { key, by }, 10);
        a.receive(Conn(1), Up::Push(e), 0)
    }

    /// Opens the watch's connection `conn` and delivers what its hello is
    /// answered with: those frames.
    fn connect(a: &mut Server, watch: &mut Node<Msg, Memory>, conn: u32, now: u64) -> Vec<Down<Msg>> {
        a.open(Conn(conn), ME, Some(Origin(2)));
        let mut up = Vec::new();
        watch.connect(&mut up);
        let mut sent = Vec::new();
        for u in up {
            for (c, down) in a.receive(Conn(conn), u, now) {
                assert_eq!(c, Conn(conn));
                sent.push(down.clone());
                watch.receive(down, &mut Vec::new());
            }
        }
        sent
    }

    fn deliver(out: Vec<Out<Msg>>, conn: u32, watch: &mut Node<Msg, Memory>) {
        for (c, down) in out {
            if c == Conn(conn) {
                watch.receive(down, &mut Vec::new());
            }
        }
    }

    fn odd(db: &dyn Read) -> Vec<(Vec<u8>, Vec<u8>)> {
        rows(db).into_iter().filter(|(_, v)| v[0] % 2 == 1).collect()
    }

    fn count_is(key: u8, n: u64) -> Write {
        Write {
            table: COUNTS,
            key: vec![key],
            value: Some(n.to_le_bytes().to_vec()),
        }
    }

    /// The rows a fill carries, and the tables it empties.
    fn filled(sent: &[Down<Msg>]) -> (Vec<TableId>, Vec<Write>) {
        let Some(Down::Fill { tables, .. }) = sent.first() else {
            panic!("a fill: {sent:?}")
        };
        let rows = sent.iter().filter_map(|d| if let Down::Rows(w) = d { Some(w.0.clone()) } else { None });
        (tables.clone(), rows.flatten().collect())
    }

    /// A client that connects again while its record is kept is sent only
    /// what changed since its cursor: a row it holds that changed, one that
    /// moved in, one that moved out; and one sent as the connection was
    /// lost. Once the record's time is up it is filled whole.
    #[cfg_attr(test, test)]
    fn a_client_that_connects_again_is_sent_what_changed() {
        let (mut a, mut writer, mut watch) = served(0, 1000);
        assert_eq!(rows(watch.store(Space::Shared)), odd(a.store()));
        a.close(Conn(2), 100);
        assert_eq!(a.serve().kept(), 1);
        // Held and changed; moved in; moved out; not held.
        for (key, by) in [(1, 2), (2, 1), (3, 1), (6, 2)] {
            push(&mut a, &mut writer, key, by);
        }
        let sent = connect(&mut a, &mut watch, 3, 200);
        let (tables, rows_sent) = filled(&sent);
        let gone = Write {
            table: COUNTS,
            key: vec![3],
            value: None,
        };
        assert_eq!((tables, rows_sent), (vec![], vec![count_is(1, 3), count_is(2, 3), gone]));
        assert!(matches!(sent.last(), Some(Down::Got { .. })));
        assert_eq!(rows(watch.store(Space::Shared)), odd(a.store()));
        assert!(watch.complete(QueryId(0), &[1]) && watch.cursor() == a.head());

        // A change sent as the connection is lost is sent again.
        let lost = push(&mut a, &mut writer, 1, 2);
        assert_eq!(lost.iter().filter(|(c, _)| *c == Conn(3)).count(), 1);
        a.close(Conn(3), 300);
        let sent = connect(&mut a, &mut watch, 4, 400);
        assert_eq!(filled(&sent), (vec![], vec![count_is(1, 5)]));
        assert_eq!(rows(watch.store(Space::Shared)), odd(a.store()));
        // With nothing new, nothing: the fill that sent it reached its
        // cursor, so it had it.
        a.close(Conn(4), 500);
        let sent = connect(&mut a, &mut watch, 5, 600);
        assert_eq!(filled(&sent), (vec![], vec![]));
        // A row moved in outside the log, at the head, is sent again: the
        // client cannot tell it from what it had there.
        let mut up = Vec::new();
        watch.subscribe(QueryId(0), vec![0], &mut up);
        let out = a.receive(Conn(5), up.remove(0), 650);
        assert!(matches!(out[0].1, Down::Moved(_)));
        a.close(Conn(5), 660);
        let sent = connect(&mut a, &mut watch, 8, 670);
        let (_, again) = filled(&sent);
        assert_eq!(again.len(), 2);
        assert_eq!(rows(watch.store(Space::Shared)), rows(a.store()));
        watch.unsubscribe(QueryId(0), vec![0], &mut up);
        deliver(a.receive(Conn(8), up.remove(0), 680), 8, &mut watch);
        assert_eq!(rows(watch.store(Space::Shared)), odd(a.store()));
        a.close(Conn(8), 690);
        let sent = connect(&mut a, &mut watch, 5, 695);
        assert_eq!(filled(&sent).1.len(), 2, "the move-outs, sent outside the log, again");

        // A record past its time is let go: the next hello fills whole.
        a.close(Conn(5), 700);
        assert_eq!(a.next_tick(), Some(1700));
        a.tick(1700);
        assert_eq!(a.serve().kept(), 0);
        let sent = connect(&mut a, &mut watch, 6, 1800);
        assert_eq!(
            filled(&sent),
            (
                vec![COUNTS],
                odd(a.store())
                    .into_iter()
                    .map(|(k, v)| count_is(k[0], u64::from_le_bytes(v.try_into().unwrap())))
                    .collect()
            )
        );
        assert_eq!(rows(watch.store(Space::Shared)), odd(a.store()));
        // A client opened again while connected takes its record over.
        a.open(Conn(7), ME, Some(Origin(2)));
        assert!(a.receive(Conn(6), Up::Hello { cursor: Seq(0) }, 0).is_empty());
    }

    /// A query let go stays held for its TTL: nothing moves out, and
    /// taken up again it is got at once; then its rows no other query
    /// reads move out. One a client does not say again at its next
    /// connection is let go the same way.
    #[cfg_attr(test, test)]
    fn a_query_let_go_stays_held_for_its_ttl() {
        let (mut a, _, mut watch) = served(100, 1000);
        let mut up = Vec::new();
        watch.subscribe(QueryId(1), vec![], &mut up);
        for u in core::mem::take(&mut up) {
            deliver(a.receive(Conn(2), u, 0), 2, &mut watch);
        }
        let both = rows(watch.store(Space::Shared));
        watch.unsubscribe(QueryId(0), vec![1], &mut up);
        assert_eq!(a.receive(Conn(2), up.remove(0), 10), []);
        assert_eq!(a.next_tick(), Some(110));
        watch.subscribe(QueryId(0), vec![1], &mut up);
        assert_eq!(
            a.receive(Conn(2), up.remove(0), 50),
            [(
                Conn(2),
                Down::Got {
                    query: QueryId(0),
                    args: vec![1]
                }
            )]
        );
        assert_eq!(a.next_tick(), None);
        watch.unsubscribe(QueryId(0), vec![1], &mut up);
        a.receive(Conn(2), up.remove(0), 60);
        assert_eq!(a.tick(159), []);
        assert_eq!(rows(watch.store(Space::Shared)), both);
        deliver(a.tick(160), 2, &mut watch);
        let first_two: Vec<_> = rows(a.store()).into_iter().take(2).collect();
        assert_eq!(rows(watch.store(Space::Shared)), first_two);

        // Not said again at the next connection: held, then let go.
        a.close(Conn(2), 200);
        let mut again = node(2);
        let sent = connect(&mut a, &mut again, 3, 300);
        // A node with none of the rows the record sent is filled whole.
        assert_eq!(filled(&sent).0, vec![COUNTS]);
        assert_eq!(a.next_tick(), Some(400));
        assert!(a.tick(400).iter().any(|(_, d)| matches!(d, Down::Moved(_))));
    }

    crate::listed!(a_client_that_connects_again_is_sent_what_changed, a_query_let_go_stays_held_for_its_ttl);
}
