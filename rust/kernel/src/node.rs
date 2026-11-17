//! The loop: a [`Node`] takes one entry at a time ([`Machine::step`]),
//! runs its handler in a transaction on its space's store, and returns the
//! entries the handler made, which the host routes by their space
//! ([`Route`]): `local` and `device` entries are stepped here in turn,
//! `session` and `shared` ones go out through the [`Link`].
//!
//! A handler is found by the number its body names ([`Body::handler`]) in
//! the node's table of them, written by the build. It reads and writes
//! through [`Cx::db`], an [`Overlay`] on its space's store whose writes
//! are applied whole when it returns `Ok`, and dropped when it refuses.
//! A refusal is a code ([`Refusal`]), kept in the node's journal of
//! refusals for the host to show.
//!
//! What the node does with a shared entry here is run it on the shared
//! store when it is stepped. Keeping its own shared entries as pending
//! records on an overlay over confirmed rows, taking the server's facts in
//! place of running others' entries, and the rebase are the next steps
//! (docs/north-star.md, "Authority and Rebase").

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::entry::{Actor, Entry, EntryId, Hlc, Origin, Space, Version};
use crate::store::{Overlay, Store};

/// The machine every node is: one entry in, the entries it made out.
pub trait Machine {
    type Body;
    fn step(&mut self, e: Entry<Self::Body>) -> Vec<Entry<Self::Body>>;
}

/// A handler, by the number its build gives it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandlerId(pub u32);

/// A body names the handler that runs it.
pub trait Body {
    fn handler(&self) -> HandlerId;
}

/// Why an entry was not run: a code the host words. Codes below 256 are
/// the kernel's; an app's handlers use the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Refusal(pub u16);

impl Refusal {
    /// No handler has the number the body names.
    pub const NO_HANDLER: Refusal = Refusal(1);
}

/// What a handler runs with: the entry, its space's rows, and the entries
/// it makes.
pub struct Cx<'a, B> {
    pub entry: &'a Entry<B>,
    /// The entry's space's store, as this run has written it.
    pub db: &'a mut dyn Store,
    made: Vec<(Space, B)>,
}

impl<B> Cx<'_, B> {
    /// Makes an entry in `space`, caused by this one, for the host to route
    /// once this run is kept.
    pub fn emit(&mut self, space: Space, body: B) {
        self.made.push((space, body));
    }
}

/// A handler: reads and writes `cx.db`, emits entries, or refuses.
pub type Handler<B> = fn(&mut Cx<'_, B>) -> Result<(), Refusal>;

/// One store for each space.
pub struct Spaces<S> {
    pub local: S,
    pub device: S,
    pub session: S,
    pub shared: S,
}

impl<S> Spaces<S> {
    pub fn get(&self, space: Space) -> &S {
        match space {
            Space::Local => &self.local,
            Space::Device => &self.device,
            Space::Session => &self.session,
            Space::Shared => &self.shared,
        }
    }

    pub fn get_mut(&mut self, space: Space) -> &mut S {
        match space {
            Space::Local => &mut self.local,
            Space::Device => &mut self.device,
            Space::Session => &mut self.session,
            Space::Shared => &mut self.shared,
        }
    }
}

/// Where the host sends an entry, by its space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Stepped on this node: `local` and `device`.
    Step,
    /// Sent through the link: `session` and `shared`.
    Send,
}

impl Route {
    pub fn of(space: Space) -> Route {
        match space {
            Space::Local | Space::Device => Route::Step,
            Space::Session | Space::Shared => Route::Send,
        }
    }
}

/// Bytes' other end, as entries: a socket, a phone relayed to, a test.
pub trait Link<B> {
    fn send(&mut self, e: Entry<B>);
}

/// The outgoing entries, held: a link a host or a test drains.
impl<B> Link<B> for Vec<Entry<B>> {
    fn send(&mut self, e: Entry<B>) {
        self.push(e);
    }
}

/// One process running the loop.
pub struct Node<B: 'static, S> {
    origin: Origin,
    made: u64,
    actor: Actor,
    version: Version,
    clock: Hlc,
    handlers: &'static [Handler<B>],
    stores: Spaces<S>,
    refused: Vec<(EntryId, Refusal)>,
}

impl<B: Body + 'static, S: Store> Node<B, S> {
    /// A node of `origin`, which has made `made` entries before (the host
    /// keeps the count), running `handlers` over `stores`.
    pub fn new(origin: Origin, made: u64, version: Version, handlers: &'static [Handler<B>], stores: Spaces<S>) -> Node<B, S> {
        Node {
            origin,
            made,
            actor: Actor::NONE,
            version,
            clock: Hlc::default(),
            handlers,
            stores,
            refused: Vec::new(),
        }
    }

    /// Who the entries this node makes are from.
    pub fn set_actor(&mut self, actor: Actor) {
        self.actor = actor;
    }

    /// How many entries this node's origin has made: for the host to keep.
    pub fn made(&self) -> u64 {
        self.made
    }

    /// The node's clock: the latest time it has made or seen.
    pub fn clock(&self) -> Hlc {
        self.clock
    }

    pub fn store(&self, space: Space) -> &S {
        self.stores.get(space)
    }

    /// The entries refused since the last call, oldest first.
    pub fn take_refused(&mut self) -> Vec<(EntryId, Refusal)> {
        core::mem::take(&mut self.refused)
    }

    /// A new entry of this node's, at wall time `now`: a tap, a timer.
    pub fn make(&mut self, space: Space, body: B, now: u64) -> Entry<B> {
        self.clock = self.clock.tick(now);
        self.made += 1;
        Entry {
            id: EntryId {
                origin: self.origin,
                n: self.made,
            },
            space,
            actor: self.actor,
            at: self.clock,
            cause: None,
            version: self.version,
            seq: None,
            body,
        }
    }

    /// Runs `e`'s handler in a transaction on its space's store, and
    /// returns the entries it made; a refusal is journaled and returns
    /// none.
    pub fn step(&mut self, e: Entry<B>) -> Vec<Entry<B>> {
        // The node has no clock of its own: an entry's time is when it is
        // taken in.
        self.clock = self.clock.witness(e.at, e.at.wall);
        let Some(handler) = self.handlers.get(e.body.handler().0 as usize) else {
            self.refused.push((e.id, Refusal::NO_HANDLER));
            return Vec::new();
        };
        let mut db = Overlay::new(self.stores.get(e.space));
        let mut cx = Cx {
            entry: &e,
            db: &mut db,
            made: Vec::new(),
        };
        let ran = handler(&mut cx);
        let made = cx.made;
        if let Err(why) = ran {
            self.refused.push((e.id, why));
            return Vec::new();
        }
        let writes = db.into_writes();
        self.stores.get_mut(e.space).apply(&writes);
        made.into_iter()
            .map(|(space, body)| {
                self.clock = self.clock.tick(self.clock.wall);
                self.made += 1;
                Entry {
                    id: EntryId {
                        origin: self.origin,
                        n: self.made,
                    },
                    space,
                    actor: e.actor,
                    at: self.clock,
                    cause: Some(e.id),
                    version: self.version,
                    seq: None,
                    body,
                }
            })
            .collect()
    }

    /// Routes an entry this node made, and what stepping it makes, until
    /// nothing is left to step: in order, each `local` and `device` entry
    /// stepped here and each `session` and `shared` one sent.
    pub fn route(&mut self, e: Entry<B>, link: &mut dyn Link<B>) {
        self.run(VecDeque::from([e]), link);
    }

    /// Steps an entry that arrived through the link, and routes what it
    /// makes.
    pub fn receive(&mut self, e: Entry<B>, link: &mut dyn Link<B>) {
        let made = self.step(e);
        self.run(made.into(), link);
    }

    fn run(&mut self, mut queue: VecDeque<Entry<B>>, link: &mut dyn Link<B>) {
        while let Some(e) = queue.pop_front() {
            match Route::of(e.space) {
                Route::Step => queue.extend(self.step(e)),
                Route::Send => link.send(e),
            }
        }
    }
}

impl<B: Body + 'static, S: Store> Machine for Node<B, S> {
    type Body = B;

    fn step(&mut self, e: Entry<B>) -> Vec<Entry<B>> {
        Node::step(self, e)
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::entry::Seq;
    use crate::store::{Memory, Range, Read, TableId};
    use alloc::vec;

    const COUNTS: TableId = TableId(1);

    /// A counter app: `Add` adds to a count, `Tap` asks for a shared `Add`
    /// and a local `Seen`, `Seen` counts taps, `Refuse` writes and refuses.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Msg {
        Add { key: u8, by: u64 },
        Tap { key: u8 },
        Seen,
        Refuse,
        Unknown,
    }

    impl Body for Msg {
        fn handler(&self) -> HandlerId {
            HandlerId(match self {
                Msg::Add { .. } => 0,
                Msg::Tap { .. } => 1,
                Msg::Seen => 2,
                Msg::Refuse => 3,
                Msg::Unknown => 9,
            })
        }
    }

    fn count(db: &dyn Read, key: u8) -> u64 {
        db.get(COUNTS, &[key]).map_or(0, |v| u64::from_le_bytes(v.try_into().unwrap()))
    }

    fn add(cx: &mut Cx<'_, Msg>, key: u8, by: u64) {
        let n = count(cx.db, key) + by;
        cx.db.put(COUNTS, &[key], &n.to_le_bytes());
    }

    static HANDLERS: [Handler<Msg>; 4] = [
        |cx| {
            let Msg::Add { key, by } = cx.entry.body else { unreachable!() };
            add(cx, key, by);
            Ok(())
        },
        |cx| {
            let Msg::Tap { key } = cx.entry.body else { unreachable!() };
            cx.emit(Space::Shared, Msg::Add { key, by: 1 });
            cx.emit(Space::Local, Msg::Seen);
            Ok(())
        },
        |cx| {
            add(cx, 0, 1);
            Ok(())
        },
        |cx| {
            add(cx, 0, 100);
            cx.emit(Space::Shared, Msg::Seen);
            Err(Refusal(300))
        },
    ];

    fn node(origin: u64) -> Node<Msg, Memory> {
        let stores = Spaces {
            local: Memory::new(),
            device: Memory::new(),
            session: Memory::new(),
            shared: Memory::new(),
        };
        Node::new(Origin(origin), 0, Version(1), &HANDLERS, stores)
    }

    fn rows(n: &Node<Msg, Memory>, space: Space) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        n.store(space).scan(COUNTS, Range::all(), &mut |k, v| {
            out.push((k.to_vec(), v.to_vec()));
            true
        });
        out
    }

    #[cfg_attr(test, test)]
    fn a_step_runs_on_its_space() {
        let mut n = node(1);
        let e = n.make(Space::Device, Msg::Add { key: 4, by: 2 }, 1000);
        assert_eq!(e.id, EntryId { origin: Origin(1), n: 1 });
        assert_eq!(n.step(e.clone()), []);
        assert_eq!(n.step(e), []);
        assert_eq!(count(n.store(Space::Device), 4), 4);
        assert_eq!(count(n.store(Space::Local), 4), 0);
        assert_eq!(n.made(), 1);
    }

    #[cfg_attr(test, test)]
    fn what_a_step_makes_is_routed_by_space() {
        let mut n = node(1);
        n.set_actor(Actor(5));
        let tap = n.make(Space::Local, Msg::Tap { key: 7 }, 1000);
        let mut sent = Vec::new();
        n.route(tap.clone(), &mut sent);
        // The local `Seen` was stepped here; the shared `Add` was sent,
        // caused by the tap, from the same actor, after it.
        assert_eq!(count(n.store(Space::Local), 0), 1);
        assert_eq!(count(n.store(Space::Shared), 7), 0);
        let [add] = &sent[..] else { panic!("one entry sent: {sent:?}") };
        assert_eq!(add.body, Msg::Add { key: 7, by: 1 });
        assert_eq!(add.space, Space::Shared);
        assert_eq!(add.cause, Some(tap.id));
        assert_eq!(add.actor, Actor(5));
        assert_eq!(add.id, EntryId { origin: Origin(1), n: 2 });
        assert!(add.at > tap.at);
        assert_eq!(add.seq, None);

        // It comes back ordered by the master log and is stepped.
        let confirmed = Entry {
            seq: Some(Seq(1)),
            ..add.clone()
        };
        n.receive(confirmed, &mut sent);
        assert_eq!(count(n.store(Space::Shared), 7), 1);
        assert_eq!(sent.len(), 1);
    }

    #[cfg_attr(test, test)]
    fn a_refusal_writes_nothing_and_is_journaled() {
        let mut n = node(1);
        let e = n.make(Space::Shared, Msg::Refuse, 1000);
        let unknown = n.make(Space::Shared, Msg::Unknown, 1000);
        assert_eq!(n.step(e.clone()), []);
        assert_eq!(n.step(unknown.clone()), []);
        assert_eq!(rows(&n, Space::Shared), []);
        assert_eq!(n.take_refused(), [(e.id, Refusal(300)), (unknown.id, Refusal::NO_HANDLER)]);
        assert_eq!(n.take_refused(), []);
    }

    #[cfg_attr(test, test)]
    fn the_same_entries_make_the_same_rows() {
        let mut a = node(1);
        let mut b = node(2);
        let mut sent = Vec::new();
        for (i, key) in [3u8, 1, 3, 2].into_iter().enumerate() {
            let tap = a.make(Space::Local, Msg::Tap { key }, 1000 + i as u64);
            a.route(tap, &mut sent);
        }
        let last = sent.last().unwrap().at;
        for (seq, e) in sent.drain(..).enumerate() {
            let e = Entry {
                seq: Some(Seq(seq as u64 + 1)),
                ..e
            };
            a.receive(e.clone(), &mut vec![]);
            b.receive(e, &mut vec![]);
        }
        assert_eq!(rows(&a, Space::Shared), rows(&b, Space::Shared));
        assert_eq!(count(a.store(Space::Shared), 3), 2);
        // b's clock is past every entry it took in.
        assert!(b.clock() > last);
    }

    #[cfg_attr(test, test)]
    fn routes() {
        assert_eq!(Space::ALL.map(Route::of), [Route::Step, Route::Step, Route::Send, Route::Send]);
    }

    crate::listed!(
        a_step_runs_on_its_space,
        what_a_step_makes_is_routed_by_space,
        a_refusal_writes_nothing_and_is_journaled,
        the_same_entries_make_the_same_rows,
        routes,
    );
}
