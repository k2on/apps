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
//! - **Session entries** pass between every side: the phone steps a
//!   watch's (its player is the owner of its playback), and passes it up
//!   and to its other downstream nodes; its own and its upstream's go
//!   down. Of each row only the newest by its time passes, so one the
//!   server says back late never undoes a newer one.
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
                    Verdict::At { seq, .. } => {
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
                let Some(passed) = self.serve.live(None, e.actor, e.clone(), now) else {
                    return Served {
                        out,
                        changes: Changes::default(),
                    };
                };
                out = passed;
                let mut said = Vec::new();
                node.receive(Down::Live(e), &mut said);
                out.extend(self.pass(said, link, now));
            }
            down => {
                if let Down::Logged(l) = &down {
                    logged = Some(Logged {
                        entry: l.entry.clone(),
                        facts: Default::default(),
                    });
                }
                let mut said = Vec::new();
                node.receive(down, &mut said);
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
        let head = node.cursor();
        let view = node.view(Space::Shared);
        let to = |frames: Vec<Down<B>>| frames.into_iter().map(|d| (conn, d)).collect();
        match up {
            Up::Hello { cursor } => served.out = to(self.serve.hello(conn, cursor, &view, head, now).1),
            Up::Subscribe { query, args } => served.out = to(self.serve.subscribe(conn, query, args, &view, head)),
            Up::Unsubscribe { query, args } => served.out = to(self.serve.unsubscribe(conn, query, args, head, now)),
            Up::Rate { table, every } => self.serve.rate(conn, table, every),
            Up::Push(e) => {
                let id = e.id;
                let answer = |verdict| Vec::from([(conn, Down::Verdict { id, verdict })]);
                if let Some(&why) = self.refused.get(&id) {
                    served.out = answer(Verdict::Refused(why));
                } else if let Some(&(seq, last)) = self.judged.get(&id.origin).filter(|j| id.n <= j.1) {
                    served.out = answer(Verdict::At { seq, last });
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
                let Some(passed) = self.serve.live(Some(conn), actor, e.clone(), now) else {
                    return served;
                };
                let before = node.cursor();
                let mut said = Vec::new();
                node.receive(Down::Live(e.clone()), &mut said);
                link.send(Up::Live(e));
                served.out = passed;
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
                out.extend(self.serve.live(None, e.actor, e.clone(), now).unwrap_or_default());
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
    use crate::store::Memory;
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
        watch.connect(&mut up);
        let deliver = |out: Vec<Out<Msg>>, watch: &mut Node<Msg, Memory>| {
            for (c, down) in out {
                assert_eq!(c, Conn(9));
                watch.receive(down, &mut Vec::new());
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
        phone.connect(&mut to_server);
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

    crate::listed!(a_phone_serves_its_watch);
}
