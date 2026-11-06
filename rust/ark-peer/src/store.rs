//! The views an app holds, kept in one place (§1.10): what every shell
//! wrote by hand before — a view opened by name on its input, read once,
//! its patches spliced as it moves, held while a screen shows it and let go
//! after the last one leaves, and a change sent with its refusal kept.
//!
//! An app names every view it opens as one value of its own key type
//! ([`ViewKey`]): money's `MoneyView`, harken's `HarkenView`. A key is one
//! view of the domain ([`ViewKey::open`]: its function, its input and the
//! type its rows decode as), or a page made of several ([`ViewKey::parts`]):
//! money's budget for a month is its lines, its summary, its spending and
//! its formulas, wherever it is drawn. The core says what a page needs, so
//! every shell opens the same views for it.
//!
//! [`Store::hold`] holds a key once more (each of its parts opened the
//! first time, read whole), [`Store::release`] once less (each closed when
//! nothing holds it). [`Store::settle`] takes what the peer did since into
//! every view held and answers the keys that moved — each view that did,
//! and each page held that is made of one — and [`Store::version`] is a
//! number that grows whenever a key's rows moved, which a UI compares to
//! know what to read again. The rows are the app's own type, decoded once
//! as they arrive ([`Store::rows`]).
//!
//! It is UI-free: the iced windows hold one directly, and the phones' one
//! behind the bridge (`ark_ffi::Session`), settled by the bridge's thread.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Debug;

use ark::format::Encode;
use ark::hash::StateHash;
use ark::log::EntryId;

use crate::{Peer, Update, Watched};

/// A key of an app's views: one view of the domain, or a page made of
/// several.
pub trait ViewKey: Clone + Ord + Debug + Send + 'static {
    /// The views this key is made of: itself when it is one view, or every
    /// view a page needs — each of them a key whose [`ViewKey::open`] says
    /// how it is opened.
    fn parts(&self) -> Vec<Self> {
        vec![self.clone()]
    }

    /// How the view this key names is opened; none for a page, which is
    /// only its parts.
    fn open(&self) -> Option<Opening>;
}

/// How one view is opened: its function by name, its input's bytes, and
/// the type its rows are read as.
pub struct Opening {
    name: String,
    input: Vec<u8>,
    make: fn(&mut Peer, &str, Vec<u8>) -> Box<dyn Part>,
}

impl Opening {
    /// The view of `name` on `input`, its rows read as `T` (a row that does
    /// not decode is a domain this build does not match: not a state to
    /// draw).
    pub fn of<T: Encode + Send + 'static>(name: &str, input: Vec<u8>) -> Opening {
        Opening {
            name: name.into(),
            input,
            make: |peer, name, input| Box::new(Watched::open(peer, name, input, decode::<T>)),
        }
    }

    /// The function the view is of.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its input's bytes.
    pub fn input(&self) -> &[u8] {
        &self.input
    }
}

fn decode<T: Encode>(b: &[u8]) -> T {
    T::from_bytes(b).unwrap_or_else(|e| panic!("a view's row: {e}"))
}

/// One view held, its rows of whatever type it was opened as.
trait Part: Send {
    fn settle(&mut self, peer: &mut Peer, moved: bool) -> Update;
    fn close(self: Box<Self>, peer: &mut Peer);
    fn rows(&self) -> &dyn Any;
    fn name(&self) -> &str;
    fn view(&self) -> Option<usize>;
}

impl<T: Send + 'static> Part for Watched<T> {
    fn settle(&mut self, peer: &mut Peer, moved: bool) -> Update {
        Watched::settle(self, peer, moved)
    }

    fn close(self: Box<Self>, peer: &mut Peer) {
        Watched::close(*self, peer)
    }

    fn rows(&self) -> &dyn Any {
        &self.rows
    }

    fn name(&self) -> &str {
        Watched::name(self)
    }

    fn view(&self) -> Option<usize> {
        Watched::view(self)
    }
}

struct Held {
    part: Box<dyn Part>,
    holds: usize,
    /// The store's tick when its rows last moved.
    version: u64,
    /// What it did at the last settle that moved it.
    last: Update,
}

/// An app's views, held by key.
pub struct Store<K: ViewKey> {
    views: BTreeMap<K, Held>,
    /// The pages held, each how many times.
    pages: BTreeMap<K, usize>,
    /// Grows at every move: what each view's version is taken from.
    tick: u64,
    /// The peer's state at the last settle: a view whose query refused is
    /// asked again only when it moved.
    seen: Option<StateHash>,
    refused: Option<String>,
}

impl<K: ViewKey> Default for Store<K> {
    fn default() -> Self {
        Store {
            views: BTreeMap::new(),
            pages: BTreeMap::new(),
            tick: 0,
            seen: None,
            refused: None,
        }
    }
}

impl<K: ViewKey> Store<K> {
    pub fn new() -> Store<K> {
        Store::default()
    }

    /// `key` held once more: each of its views opened and read whole the
    /// first time anything holds it.
    pub fn hold(&mut self, peer: &mut Peer, key: K) {
        let parts = key.parts();
        if parts.len() != 1 || parts[0] != key {
            *self.pages.entry(key).or_default() += 1;
        }
        for p in parts {
            if let Some(h) = self.views.get_mut(&p) {
                h.holds += 1;
                continue;
            }
            let Some(o) = p.open() else { continue };
            let part = (o.make)(peer, &o.name, o.input);
            // What the first read had is not news.
            if let Some(v) = part.view() {
                peer.update(v);
            }
            self.tick += 1;
            let held = Held {
                part,
                holds: 1,
                version: self.tick,
                last: Update::Reset,
            };
            self.views.insert(p, held);
        }
    }

    /// `key` held once less: each of its views closed when nothing holds
    /// it any more.
    pub fn release(&mut self, peer: &mut Peer, key: &K) {
        let parts = key.parts();
        if parts.len() != 1 || parts[0] != *key {
            match self.pages.get_mut(key) {
                Some(n) if *n > 1 => *n -= 1,
                Some(_) => drop(self.pages.remove(key)),
                None => return,
            }
        }
        for p in parts {
            let Some(h) = self.views.get_mut(&p) else { continue };
            h.holds -= 1;
            if h.holds == 0 {
                if let Some(h) = self.views.remove(&p) {
                    h.part.close(peer);
                }
            }
        }
    }

    /// Whether anything holds `key`.
    pub fn holds(&self, key: &K) -> bool {
        self.views.contains_key(key) || self.pages.contains_key(key)
    }

    /// Every key held: each view, and each page.
    pub fn held(&self) -> Vec<K> {
        self.views.keys().chain(self.pages.keys()).cloned().collect()
    }

    /// The rows of the view `key`, as the type it was opened as; none while
    /// it is not held, or when they are not `T`.
    pub fn rows<T: 'static>(&self, key: &K) -> &[T] {
        self.views
            .get(key)
            .and_then(|h| h.part.rows().downcast_ref::<Vec<T>>())
            .map_or(&[], |r| &r[..])
    }

    /// The first of a view's rows: a view of one row (a month's summary).
    pub fn first<T: 'static>(&self, key: &K) -> Option<&T> {
        self.rows(key).first()
    }

    /// A number that grows whenever `key`'s rows moved — a page's, whenever
    /// any of its views' did; zero while nothing holds it.
    pub fn version(&self, key: &K) -> u64 {
        if let Some(h) = self.views.get(key) {
            return h.version;
        }
        key.parts().iter().filter_map(|p| self.views.get(p)).map(|h| h.version).max().unwrap_or(0)
    }

    /// What the view `key` did at the settle that last moved it: a reset
    /// when it was first read.
    pub fn last(&self, key: &K) -> Option<&Update> {
        self.views.get(key).map(|h| &h.last)
    }

    /// The function the view `key` is of, while it is held.
    pub fn name(&self, key: &K) -> Option<&str> {
        self.views.get(key).map(|h| h.part.name())
    }

    /// What the peer did since the last settle, taken into every view held:
    /// the keys that moved — each view, then each page held made of one.
    pub fn settle(&mut self, peer: &mut Peer) -> Vec<K> {
        let state = peer.state();
        let moved = self.seen != Some(state);
        self.seen = Some(state);
        let mut out = vec![];
        for (k, h) in self.views.iter_mut() {
            let u = h.part.settle(peer, moved);
            if u == Update::Unchanged {
                continue;
            }
            self.tick += 1;
            h.version = self.tick;
            h.last = u;
            out.push(k.clone());
        }
        if !out.is_empty() {
            let pages = self.pages.keys().filter(|p| p.parts().iter().any(|v| out.contains(v)));
            let pages: Vec<K> = pages.cloned().collect();
            out.extend(pages);
        }
        out
    }

    /// Every view let go: the store as it was made.
    pub fn close(&mut self, peer: &mut Peer) {
        for (_, h) in std::mem::take(&mut self.views) {
            h.part.close(peer);
        }
        self.pages.clear();
    }

    /// A change: the function `name` on `input`, its intent's id, or the
    /// domain's sentence for why not — which is kept ([`Store::refusal`])
    /// until the next change is taken.
    pub fn send(&mut self, peer: &mut Peer, name: &str, input: &impl Encode) -> Result<EntryId, String> {
        self.sent(peer.mutate(name, input))
    }

    /// What became of a change sent some other way, kept as
    /// [`Store::send`] keeps it.
    pub fn sent<T>(&mut self, r: Result<T, String>) -> Result<T, String> {
        self.refused = r.as_ref().err().cloned();
        r
    }

    /// Why the last change sent was refused, while it is the last.
    pub fn refusal(&self) -> Option<&str> {
        self.refused.as_deref()
    }
}
