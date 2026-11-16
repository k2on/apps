//! The tests' host, for any of harken's screens: the demo's library on a
//! peer of its own, the views a screen names held, and what it opened and
//! requested written down.

use ark_app::{App, Ask, Host, Inbox, Modules, Runtime, Ticket};
use ark_peer::store::Store;
use ark_peer::Peer;
use harken_domain::{HarkenChange, HarkenView};

use crate::{HarkenRequest, Route};

/// 2026-10-26, noon UTC.
pub const NOW: i64 = 1_792_929_600_000;

pub struct Demo<A: App> {
    pub peer: Peer,
    pub store: Store<HarkenView>,
    pub modules: Modules,
    pub inbox: Inbox<A::Msg>,
    pub opened: Vec<Route>,
    pub requests: Vec<HarkenRequest>,
}

/// The demo's library as a peer of its own, its authority the library's.
pub fn demo_peer() -> Peer {
    harken_seed::open(harken_seed::seeded_disk("2026-10-26", NOW))
}

impl<A: App<Key = HarkenView, Change = HarkenChange, Route = Route, Request = HarkenRequest>> Demo<A> {
    pub fn new() -> Demo<A> {
        let mut d = Demo {
            peer: demo_peer(),
            store: Store::new(),
            modules: Modules::new(),
            inbox: Inbox::default(),
            opened: vec![],
            requests: vec![],
        };
        d.hold(A::holds(&A::State::default()));
        d
    }

    /// The views `keys` held, and settled.
    pub fn hold(&mut self, keys: Vec<HarkenView>) {
        for k in keys {
            self.store.hold(&mut self.peer, k);
        }
        self.store.settle(&mut self.peer);
    }

    /// `msgs` taken in turn, the views the state names held after each.
    pub fn run(&mut self, rt: &mut Runtime<A>, msgs: impl IntoIterator<Item = A::Msg>) {
        for m in msgs {
            rt.dispatch(m, self);
            let inbox = self.inbox.clone();
            inbox.drain(rt, self);
            self.hold(rt.holds());
        }
    }

    /// The first song of the demo's library, its id's hex.
    pub fn first_song(&mut self) -> String {
        let all = HarkenView::Library { playlist: [0; 16] };
        self.hold(vec![all.clone()]);
        let rows: Vec<harken_domain::LibraryEntry> = self
            .store
            .rows::<Vec<u8>>(&all)
            .iter()
            .filter_map(|b| <harken_domain::LibraryEntry as ark::format::Encode>::from_bytes(b).ok())
            .collect();
        rows.iter().find(|e| e.kind != "episode").expect("a song").id.hex()
    }

    /// The screen's record as it is now.
    pub fn page(&mut self, rt: &mut Runtime<A>) -> A::Screen {
        self.hold(rt.holds());
        rt.screen(self)
    }
}

impl<A: App<Key = HarkenView, Change = HarkenChange, Route = Route, Request = HarkenRequest>> Host<A> for Demo<A> {
    fn read<R>(&self, f: impl FnOnce(&dyn ark::views::Views<HarkenView>) -> R) -> R {
        f(&self.store)
    }
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn send(&mut self, change: HarkenChange) -> Result<(), String> {
        let r = self.store.sent(self.peer.mutate_bytes(change.name(), change.input()));
        self.store.settle(&mut self.peer);
        r.map(|_| ())
    }
    fn open(&mut self, route: Route) {
        self.opened.push(route)
    }
    fn ask(&mut self, ticket: Ticket, ask: Ask<A::Msg>) {
        self.inbox.ask(&self.modules, ticket, ask)
    }
    fn request(&mut self, r: HarkenRequest) {
        self.requests.push(r)
    }
}
