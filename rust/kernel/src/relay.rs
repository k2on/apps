//! Any upstream serves a downstream: a node that is itself downstream of
//! another (a phone, of the server) serving its own downstream nodes (its
//! watch) exactly as the server serves it. Host code: a phone links it
//! beside its [`Node`], a watch never does.
//!
//! ```text
//! server  <-  phone (Node + Relay)  <-  watch
//! ```
//!
//! - **Subscriptions** are answered from the phone's own view, its
//!   confirmed rows with its pending entries over them, through the same
//!   [`Serve`] the server runs, records and TTLs and all: so a watch and
//!   its phone always agree, even when neither reaches the server. Each
//!   entry of the log the phone applies goes down with the facts of the
//!   rows the watch holds, as the phone's view has them, so the watch's
//!   cursor moves with the phone's; what the phone's own entries do to its
//!   view goes down as rows moved; a fill of the phone's own is a fill of
//!   the watch's held rows.
//! - **Pending entries.** A watch's push keeps its origin: the relay holds
//!   it and passes it upstream, now and at each of the phone's
//!   connections, until the upstream judges it. The phone never runs it
//!   and never judges it: the upstream's verdict goes down to the watch's
//!   connection that pushed it, and its confirmation with the log, so
//!   "confirmed" still means the server's. A verdict covers the entries of
//!   its origin pushed before it, so it must not reach a watch before
//!   theirs: the relay keeps the upstream's refusals of its watch's
//!   entries and, per origin, the last entry an `At` answered, and answers
//!   a push again from them at once; any other awaits the upstream's
//!   verdict, which, the upstream's link being ordered and the held pushes
//!   sent again oldest first, comes after those before it.
//! - **Session entries** pass between every side, as the phone's node
//!   takes them ([`Node`]'s rules: a row it owns takes only a newer entry,
//!   any other is its upstream's): a watch's, which the phone takes (its
//!   player is the owner of its playback) and passes up and to every
//!   downstream connection, the watch's own too; its own and its
//!   upstream's go down. What the relay greets a watch with is the phone's
//!   session rows. The phone answers for the rows its downstream nodes own
//!   ([`Node::answer_for`]), as for its own: so what a watch set while the
//!   phone was away from the server reaches it when the phone connects,
//!   and what the server kept for them meanwhile is not taken.
//!
//! The host routes the phone's own entries through [`Relay::route`], what
//! its upstream sends through [`Relay::down`], and what a downstream sends
//! through [`Relay::up`], and sends the frames each answers ([`Served`]);
//! the shared view's changes come back with them, as the relay has taken
//! them from the node for its downstream, for the phone's own queries.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::entry::{Entry, EntryId, Origin, Seq, Space};
use crate::node::{Body, Link, Node, Refusal};
use crate::plan::Changes;
use crate::serve::{stamped, Conn, Out, Serve};
use crate::store::Store;
use crate::sync::{Down, Logged, Up, Verdict};

/// What a call answers: the frames for downstream connections, and what
/// it changed in the node's shared view, for the host's own queries.
pub struct Served<B> {
    pub out: Vec<Out<B>>,
    pub changes: Changes,
}

/// A node's downstream: whom it serves, and their pushes it holds until
/// its upstream judges them.
pub struct Relay<B> {
    serve: Serve<B>,
    /// Downstream pushes not yet judged upstream, oldest first.
    forwarded: Vec<Entry<B>>,
    /// Downstream pushes awaiting the upstream's verdict, by the
    /// connection that pushed each.
    awaiting: Vec<(Conn, EntryId)>,
    /// The upstream's refusals of downstream entries, and per origin the
    /// last entry an `At` answered, with that answer's `seq`.
    refused: BTreeMap<EntryId, Refusal>,
    judged: BTreeMap<Origin, (Seq, u64)>,
}

impl<B: Body + Clone + 'static> Default for Relay<B> {
    fn default() -> Relay<B> {
        Relay {
            serve: Serve::new(false),
            forwarded: Vec::new(),
            awaiting: Vec::new(),
            refused: BTreeMap::new(),
            judged: BTreeMap::new(),
        }
    }
}

impl<B: Body + Clone + 'static> Relay<B> {
    /// Its connections and records, to set its queries and TTLs and to
    /// open connections ([`Serve`]); [`Relay::close`] closes them. A
    /// downstream that says no subscription holds nothing.
    pub fn serve(&mut self) -> &mut Serve<B> {
        &mut self.serve
    }

    /// A downstream connection closed at `now` ([`Serve::close`]): what
    /// awaited a verdict for it is let go.
    pub fn close(&mut self, conn: Conn, now: u64) {
        self.serve.close(conn, now);
        self.awaiting.retain(|a| a.0 != conn);
    }

    /// The downstream pushes it holds, oldest first.
    pub fn forwarded(&self) -> &[Entry<B>] {
        &self.forwarded
    }

    /// What the node says at each connection to its upstream, after its
    /// own ([`Node::connect`]): the downstream pushes not yet judged.
    pub fn connect(&self, link: &mut dyn Link<B>) {
        for e in &self.forwarded {
            link.send(Up::Push(e.clone()));
        }
    }

    /// Routes the node's own entry ([`Node::route`]) at `now`, its session
    /// entries passed down as well as up.
    pub fn route<S: Store>(&mut self, node: &mut Node<B, S>, e: Entry<B>, link: &mut dyn Link<B>, now: u64) -> Served<B> {
        let before = node.cursor();
        let mut said = Vec::new();
        node.route(e, &mut said);
        let mut out = self.pass(said, link, now);
        let served = self.settle(node, None, before);
        out.extend(served.out);
        Served { out, ..served }
    }

    /// Takes what the node's upstream sent at `now`: the node takes it,
    /// and what is the downstream's goes down.
    pub fn down<S: Store>(&mut self, node: &mut Node<B, S>, down: Down<B>, link: &mut dyn Link<B>, now: u64) -> Served<B> {
        let before = node.cursor();
        let mut out = Vec::new();
        let mut logged = None;
        match down {
            // The upstream judged a downstream push: it goes to the
            // connections awaiting it, and it and those before it of its
            // origin, whose verdicts came before it, are held no longer.
            Down::Verdict { id, verdict } if id.origin != node.origin() => {
                match verdict {
                    Verdict::Refused(why) => {
                        self.refused.insert(id, why);
                    }
                    Verdict::At(seq) => {
                        let judged = self.judged.entry(id.origin).or_insert((seq, id.n));
                        *judged = (seq.max(judged.0), id.n.max(judged.1));
                    }
                }
                self.forwarded.retain(|e| e.id.origin != id.origin || e.id.n > id.n);
                let serve = &self.serve;
                self.awaiting.retain(|&(c, a)| {
                    if a == id && serve.actor(c).is_some() {
                        out.push((c, Down::Verdict { id, verdict }));
                    }
                    a != id
                });
                return Served {
                    out,
                    changes: Changes::default(),
                };
            }
            Down::Live(e) => {
                let mut said = Vec::new();
                if node.take_live(e.clone(), true, &mut said) {
                    out = self.serve.pass(e.actor, e, now);
                }
                out.extend(self.pass(said, link, now));
            }
            down => {
                let welcome = matches!(down, Down::Welcome { .. });
                if let Down::Logged(l) = &down {
                    logged = Some(Logged {
                        entry: l.entry.clone(),
                        facts: Default::default(),
                    });
                }
                let mut said = Vec::new();
                node.receive(down, &mut said, now);
                // The rows the node let go at its hello's end go here too.
                if welcome {
                    self.serve.retain_latest(|l| node.latest().any(|k| k.id == l.id));
                }
                out.extend(self.pass(said, link, now));
            }
        }
        // The entry went down only if the node took it.
        let logged = logged.filter(|l| l.entry.seq == Some(node.cursor()) && node.cursor() != before);
        let served = self.settle(node, logged.as_ref(), before);
        out.extend(served.out);
        Served { out, ..served }
    }

    /// Takes what downstream connection `conn` sent at `now`, and answers
    /// it from the node's view.
    pub fn up<S: Store>(&mut self, node: &mut Node<B, S>, conn: Conn, up: Up<B>, link: &mut dyn Link<B>, now: u64) -> Served<B> {
        let mut served = Served {
            out: Vec::new(),
            changes: Changes::default(),
        };
        let Some(actor) = self.serve.actor(conn) else { return served };
        // The phone answers for the rows its downstream node owns.
        if let (Up::Hello { .. }, Some(client)) = (&up, self.serve.client(conn)) {
            node.answer_for(client);
        }
        let head = node.cursor();
        let view = node.view(Space::Shared);
        let to = |frames: Vec<Down<B>>| frames.into_iter().map(|d| (conn, d)).collect();
        match up {
            Up::Hello { cursor, time } => {
                served.out = to(self.serve.hello(conn, cursor, &view, head, now).1);
                served.out.push((
                    conn,
                    Down::Welcome {
                        sent: time,
                        time: node.time(now),
                    },
                ));
            }
            Up::Subscribe { query, args } => served.out = to(self.serve.subscribe(conn, query, args, &view, head)),
            Up::Unsubscribe { query, args } => served.out = to(self.serve.unsubscribe(conn, query, args, head, now)),
            Up::Rate { table, every } => self.serve.rate(conn, table, every),
            Up::Push(e) => {
                let id = e.id;
                let answer = |verdict| Vec::from([(conn, Down::Verdict { id, verdict })]);
                if let Some(&why) = self.refused.get(&id) {
                    served.out = answer(Verdict::Refused(why));
                } else if let Some(&(seq, _)) = self.judged.get(&id.origin).filter(|j| id.n <= j.1) {
                    served.out = answer(Verdict::At(seq));
                } else {
                    if !self.forwarded.iter().any(|f| f.id == id) {
                        self.forwarded.push(e.clone());
                    }
                    if !self.awaiting.contains(&(conn, id)) {
                        self.awaiting.push((conn, id));
                    }
                    link.send(Up::Push(e));
                }
            }
            Up::Live(e) => {
                let Some(e) = stamped(actor, e).ok().filter(|e| e.space == Space::Session) else {
                    return served;
                };
                let before = node.cursor();
                let mut said = Vec::new();
                if !node.take_live(e.clone(), false, &mut said) {
                    return served;
                }
                served.out = self.serve.pass(actor, e.clone(), now);
                link.send(Up::Live(e));
                served.out.extend(self.pass(said, link, now));
                let settled = self.settle(node, None, before);
                served.out.extend(settled.out);
                served.changes = settled.changes;
            }
        }
        served
    }

    /// What is due by `now`: the downstream's session entries whose turn
    /// has come, and the move-outs of queries whose TTL has ended.
    pub fn tick<S: Store>(&mut self, node: &Node<B, S>, now: u64) -> Vec<Out<B>> {
        self.serve.tick(now, node.cursor())
    }

    /// When [`Relay::tick`] next has something to do, if ever.
    pub fn next_tick(&self) -> Option<u64> {
        self.serve.next_tick()
    }

    /// Passes down what the node's view did since the relay last looked:
    /// for a host that changed the node itself (a resume).
    pub fn changed<S: Store>(&mut self, node: &mut Node<B, S>) -> Served<B> {
        let before = node.cursor();
        self.settle(node, None, before)
    }

    /// Sends upstream what the node said, its session entries passed down
    /// as well.
    fn pass(&mut self, said: Vec<Up<B>>, link: &mut dyn Link<B>, now: u64) -> Vec<Out<B>> {
        let mut out = Vec::new();
        for up in said {
            if let Up::Live(e) = &up {
                out.extend(self.serve.pass(e.actor, e.clone(), now));
            }
            link.send(up);
        }
        out
    }

    /// Takes the node's shared changes and passes them down: with the log's
    /// entry the node took, if one; as a fill, if its view changed whole.
    fn settle<S: Store>(&mut self, node: &mut Node<B, S>, logged: Option<&Logged<B>>, before: Seq) -> Served<B> {
        let changes = node.take_changes(Space::Shared);
        let head = node.cursor();
        let view = node.view(Space::Shared);
        let out = if changes.reset {
            self.serve.refill(&view, &changes, before, head)
        } else if logged.is_some() || !changes.rows.is_empty() {
            self.serve.feed(&view, &changes, logged, head)
        } else {
            Vec::new()
        };
        Served { out, changes }
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::authority::tests::{ME, QUERIES};
    use crate::authority::Authority;
    use crate::entry::Origin;
    use crate::node::app::*;
    use crate::node::Refusal;
    use crate::store::{Memory, Read};
    use crate::sync::QueryId;
    use alloc::vec;

    /// A phone serves its watch from its own view: the phone's pending
    /// write is in the watch's rows at once; the watch's write, made while
    /// the phone is offline, waits at the phone, keeps its origin, and is
    /// confirmed only when the server logs it, which reaches the watch
    /// with the log; a refusal reaches it as the server's verdict.
    #[cfg_attr(test, test)]
    fn a_phone_serves_its_watch() {
        let mut server = Authority::new(&HANDLERS, Memory::new());
        server.open(Conn(1), ME, Some(Origin(1)));
        let mut phone = node(1);
        phone.set_actor(ME);
        let mut relay: Relay<Msg> = Relay::default();
        relay.serve().set_queries(&QUERIES);
        relay.serve().open(Conn(9), ME, Some(Origin(2)));
        let mut watch = node(2);
        watch.set_actor(ME);
        watch.subscribe(QueryId(1), vec![], &mut Vec::new());
        // The phone offline: its write is pending, and the watch, served
        // from the phone's view, holds it.
        let mut to_server = Vec::new();
        let tap = phone.make(Space::Shared, Msg::Add { key: 1, by: 2 }, 10);
        relay.route(&mut phone, tap, &mut to_server, 10);
        let mut up = Vec::new();
        watch.connect(&mut up, 0);
        let deliver = |out: Vec<Out<Msg>>, watch: &mut Node<Msg, Memory>| {
            for (c, down) in out {
                assert_eq!(c, Conn(9));
                watch.receive(down, &mut Vec::new(), 0);
            }
        };
        for u in core::mem::take(&mut up) {
            let served = relay.up(&mut phone, Conn(9), u, &mut to_server, 10);
            deliver(served.out, &mut watch);
        }
        assert!(watch.complete(QueryId(1), &[]));
        assert_eq!(count(watch.store(Space::Shared), 1), 2);
        // The watch writes; the phone holds its push, unrun.
        let take = watch.make(Space::Shared, Msg::Take { key: 1 }, 20);
        let refused = watch.make(Space::Shared, Msg::Take { key: 2 }, 20);
        watch.route(take.clone(), &mut up);
        watch.route(refused.clone(), &mut up);
        assert_eq!(count(&watch.view(Space::Shared), 1), 1);
        for u in core::mem::take(&mut up) {
            relay.up(&mut phone, Conn(9), u, &mut to_server, 20);
        }
        assert_eq!(relay.forwarded().len(), 2);
        assert_eq!(count(&phone.view(Space::Shared), 1), 2);
        // The phone connects: its own push, then the watch's, go up with
        // their origins.
        to_server.clear();
        phone.connect(&mut to_server, 0);
        relay.connect(&mut to_server);
        let pushed: Vec<_> = to_server
            .iter()
            .filter_map(|u| if let Up::Push(e) = u { Some(e.id.origin) } else { None })
            .collect();
        assert_eq!(pushed, [Origin(1), Origin(2), Origin(2)]);
        let mut frames = Vec::new();
        for u in core::mem::take(&mut to_server) {
            frames.extend(server.receive(Conn(1), u, 30));
        }
        for (_, down) in frames {
            let served = relay.down(&mut phone, down, &mut to_server, 30);
            deliver(served.out, &mut watch);
        }
        assert_eq!(server.head(), Seq(2));
        assert_eq!((watch.pending(), watch.cursor()), (&[][..], Seq(2)));
        assert_eq!(watch.take_refused(), [(refused.id, NONE_LEFT)]);
        assert_eq!(count(watch.store(Space::Shared), 1), 1);
        assert_eq!((relay.forwarded(), phone.pending()), (&[][..], &[][..]));
        // Pushed again, the watch's entries are answered by the phone from
        // what the server said: no frame goes up.
        for e in [take, refused] {
            let served = relay.up(&mut phone, Conn(9), Up::Push(e), &mut to_server, 40);
            assert!(matches!(served.out[..], [(_, Down::Verdict { .. })]));
        }
        assert_eq!(to_server, []);
    }

    /// The session through a phone: the watch's clock composes the
    /// phone's offset to the server with its own to the phone; the watch's
    /// pause reaches the phone's player with the server away, and is
    /// refused with the watch away; and when the phone connects, the
    /// server's stale entry for the phone's row is not taken: the phone
    /// says its own again, and the server and the third device take it.
    #[cfg_attr(test, test)]
    fn the_session_through_a_phone() {
        let value = |n: &Node<Msg, Memory>, key| n.store(Space::Session).get(POSITIONS, &at(key)).map(|v| v[0]);
        let mut server = Authority::new(&HANDLERS, Memory::new());
        let mut phone = node(1);
        phone.set_actor(ME);
        let mut laptop = node(3);
        laptop.set_actor(ME);
        let mut relay: Relay<Msg> = Relay::default();
        let mut watch = node(2);
        watch.set_actor(ME);
        // Each frame delivered at once: to the server at `now` on its
        // clock, a watch's to the relay at the phone's.
        let mut to_server = Vec::new();
        let up = |server: &mut Authority<Msg, Memory>, conn, said: &mut Vec<Up<Msg>>, now| {
            let mut out = Vec::new();
            for u in core::mem::take(said) {
                out.extend(server.receive(Conn(conn), u, now));
            }
            out
        };
        // The laptop on the server, its clock 1000 behind: offset 1000.
        server.open(Conn(3), ME, Some(Origin(3)));
        let mut said = Vec::new();
        laptop.connect(&mut said, 0);
        for (_, d) in up(&mut server, 3, &mut said, 1000) {
            laptop.receive(d, &mut said, 0);
        }
        assert_eq!(laptop.offset(), 1000);
        // The phone, 500 behind the server, with a 20 ms round trip.
        server.open(Conn(1), ME, Some(Origin(1)));
        phone.connect(&mut to_server, 100);
        for (_, d) in up(&mut server, 1, &mut to_server, 610) {
            relay.down(&mut phone, d, &mut to_server, 120);
        }
        assert_eq!(phone.offset(), 500);
        // Its player plays: row 1 is its own, and goes up and down.
        let play = phone.make(Space::Session, Msg::Set { key: 1, value: 1 }, 130);
        relay.route(&mut phone, play, &mut to_server, 130);
        for (_, d) in up(&mut server, 1, &mut to_server, 630) {
            relay.down(&mut phone, d, &mut to_server, 130);
        }
        // The watch, 200 ahead of the phone: answered with the phone's
        // estimate of the server's time, so 300 behind the server.
        relay.serve().open(Conn(9), ME, Some(Origin(2)));
        let mut from_watch = Vec::new();
        watch.connect(&mut from_watch, 340);
        let mut to_watch = Vec::new();
        for u in core::mem::take(&mut from_watch) {
            to_watch.extend(relay.up(&mut phone, Conn(9), u, &mut to_server, 140).out);
        }
        for (_, d) in core::mem::take(&mut to_watch) {
            watch.receive(d, &mut from_watch, 340);
        }
        assert_eq!((watch.offset(), watch.time(340), value(&watch, 1)), (300, 640, Some(1)));
        // The phone loses the server; the watch pauses, which reaches the
        // player at once.
        phone.disconnect();
        server.close(Conn(1), 700);
        to_server.clear();
        let pause = watch.make(Space::Session, Msg::Set { key: 1, value: 2 }, 400);
        watch.route(pause, &mut from_watch);
        for u in core::mem::take(&mut from_watch) {
            to_watch.extend(relay.up(&mut phone, Conn(9), u, &mut to_server, 200).out);
        }
        assert_eq!((value(&phone, 1), value(&watch, 1)), (Some(2), Some(2)));
        // Meanwhile the laptop, linked, plays on the server: kept there.
        let stale = laptop.make(Space::Session, Msg::Set { key: 1, value: 3 }, 900);
        laptop.route(stale, &mut said);
        up(&mut server, 3, &mut said, 900);
        // The watch away: its pause is refused, the row as it was.
        watch.disconnect();
        let late = watch.make(Space::Session, Msg::Set { key: 1, value: 4 }, 500);
        watch.route(late.clone(), &mut from_watch);
        assert_eq!((from_watch.len(), value(&watch, 1)), (0, Some(2)));
        assert_eq!(watch.take_refused(), [(late.id, Refusal::OFFLINE)]);
        // The phone connects: the laptop's entry is not taken, and the
        // phone says its pause again, which the server and the laptop take.
        server.open(Conn(1), ME, Some(Origin(1)));
        phone.connect(&mut to_server, 300);
        relay.connect(&mut to_server);
        let mut to_laptop = Vec::new();
        while !to_server.is_empty() {
            for (c, d) in up(&mut server, 1, &mut to_server, 810) {
                if c == Conn(3) {
                    to_laptop.push(d);
                } else {
                    relay.down(&mut phone, d, &mut to_server, 310);
                }
            }
        }
        for d in to_laptop {
            laptop.receive(d, &mut said, 1000);
        }
        assert_eq!((value(&phone, 1), value(&laptop, 1)), (Some(2), Some(2)));
        let kept = server.latest().find(|e| e.body.row() == Some((POSITIONS, at(1))));
        assert_eq!(
            kept.map(|e| (e.id.origin, e.body.clone())),
            Some((Origin(1), Msg::Set { key: 1, value: 2 }))
        );
    }

    crate::listed!(a_phone_serves_its_watch, the_session_through_a_phone);
}
