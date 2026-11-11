//! The tests' host, for any of money's screens: the demo's household on a
//! peer on 2026-10-26, the views a screen names held, the device modules'
//! fakes registered, and what it opened and requested written down. A
//! request to keep a photo is carried out as the phones' host does it: the
//! file put on the peer and `AddPhoto` sent.

use ark_app::{App, Ask, Host, Inbox, Modules, Runtime, Ticket};
use ark_peer::store::Store;
use ark_peer::Peer;
use money_core::{AddPhoto, MoneyChange, MoneyView};

use crate::{MoneyRequest, Route};

/// 2026-10-26, noon UTC.
pub const NOW: i64 = 1_792_929_600_000;

pub struct Demo<A: App> {
    pub peer: Peer,
    pub store: Store<MoneyView>,
    pub haptics: ark_haptics::fake::Fake,
    pub scanner: ark_scanner::fake::Fake,
    pub modules: Modules,
    pub inbox: Inbox<A::Msg>,
    pub opened: Vec<Route>,
    pub requests: Vec<MoneyRequest>,
}

impl<A: App<Key = MoneyView, Change = MoneyChange, Route = Route, Request = MoneyRequest>> Demo<A> {
    pub fn new() -> Demo<A> {
        let peer = money_seed::open(money_seed::seeded_disk("2026-10-26", NOW));
        let haptics = ark_haptics::fake::Fake::new();
        let clipboard = ark_clipboard::fake::Fake::new();
        let scanner = ark_scanner::fake::Fake::new();
        let mut modules = Modules::new();
        haptics.register(&mut modules);
        clipboard.register(&mut modules);
        scanner.register(&mut modules);
        let d = Demo {
            peer,
            store: Store::new(),
            haptics,
            scanner,
            modules,
            inbox: Inbox::default(),
            opened: vec![],
            requests: vec![],
        };
        let mut d = d;
        d.hold(A::holds(&A::State::default()));
        d
    }

    /// The views `keys` held, and settled.
    pub fn hold(&mut self, keys: Vec<MoneyView>) {
        for k in keys {
            self.store.hold(&mut self.peer, k);
        }
        self.store.settle(&mut self.peer);
    }

    /// `msgs` taken in turn, then whatever modules answered since.
    pub fn run(&mut self, rt: &mut Runtime<A>, msgs: impl IntoIterator<Item = A::Msg>) {
        for m in msgs {
            rt.dispatch(m, self);
            self.drain(rt);
        }
    }

    /// What modules answered since, run; the views the state names held.
    pub fn drain(&mut self, rt: &mut Runtime<A>) {
        let inbox = self.inbox.clone();
        inbox.drain(rt, self);
        self.hold(rt.holds());
    }

    pub fn rows<T: 'static>(&self, key: &MoneyView) -> &[T] {
        self.store.rows(key)
    }
}

impl<A: App<Key = MoneyView, Change = MoneyChange, Route = Route, Request = MoneyRequest>> Host<A> for Demo<A> {
    fn read<R>(&self, f: impl FnOnce(&dyn ark::views::Views<MoneyView>) -> R) -> R {
        f(&self.store)
    }
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn send(&mut self, change: MoneyChange) -> Result<(), String> {
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
    fn request(&mut self, r: MoneyRequest) {
        if let MoneyRequest::KeepPhoto { jpeg, txn } = &r {
            if let Ok(file) = self.peer.put_file(jpeg, "image/jpeg") {
                let txn_id = txn.as_deref().and_then(crate::id_of);
                let _ = Host::<A>::send(self, MoneyChange::AddPhoto(AddPhoto { file, txn_id }));
            }
        }
        self.requests.push(r)
    }
}
