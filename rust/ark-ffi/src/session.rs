//! An app's views over the bridge (`ark_peer::store`): one [`Store`] of the
//! app's keys, kept beside the peer and settled by the peer's own thread
//! whenever anything moved, and the app told which keys did.
//!
//! An app's crate makes its `Session` UniFFI object of this — money's in
//! `money/ffi`, harken's in `harken/ffi` — with its own key type (its
//! `MoneyView`, its `HarkenView`) and typed readers named after its views;
//! this is the part every app's is made of. A phone holds a key while a
//! screen shows it ([`Session::hold`], released when the [`Hold`] is),
//! reads its rows when told it moved ([`Session::read`]), and sends its
//! changes through it ([`Session::send`]), the last refusal kept.

use std::any::Any;
use std::sync::{Arc, Mutex};

use ark_peer::store::{Store, ViewKey};

use crate::{Peer, Refusal, Shared, State};

/// Told the keys that moved, on the peer's thread (or whichever thread
/// moved them).
pub type Moved<K> = Arc<dyn Fn(Vec<K>) + Send + Sync>;

/// One app's store as the peer's state keeps it.
pub(crate) trait Attached: Send {
    /// What the peer did since, taken in: whether any key moved.
    fn settle(&mut self, peer: &mut ark_peer::Peer) -> bool;
    /// The telling of what moved, to run once the peer is let go.
    fn told(&mut self) -> Option<Box<dyn FnOnce() + Send>>;
    fn any(&mut self) -> &mut dyn Any;
    fn close(&mut self, peer: &mut ark_peer::Peer);
}

struct Slot<K: ViewKey> {
    store: Store<K>,
    moved: Vec<K>,
    listener: Option<Moved<K>>,
}

impl<K: ViewKey> Attached for Slot<K> {
    fn settle(&mut self, peer: &mut ark_peer::Peer) -> bool {
        let moved = self.store.settle(peer);
        let any = !moved.is_empty();
        for k in moved {
            if !self.moved.contains(&k) {
                self.moved.push(k);
            }
        }
        any
    }

    fn told(&mut self) -> Option<Box<dyn FnOnce() + Send>> {
        let moved = std::mem::take(&mut self.moved);
        let l = self.listener.clone()?;
        if moved.is_empty() {
            return None;
        }
        Some(Box::new(move || l(moved)))
    }

    fn any(&mut self) -> &mut dyn Any {
        self
    }

    fn close(&mut self, peer: &mut ark_peer::Peer) {
        self.store.close(peer);
    }
}

/// An app's views over a peer: a [`Store`] of its keys the peer's thread
/// settles.
pub struct Session<K: ViewKey> {
    peer: Arc<Peer>,
    id: u64,
    _k: std::marker::PhantomData<fn() -> K>,
}

impl<K: ViewKey> Session<K> {
    /// A store of `K`s beside `peer`.
    pub fn new(peer: Arc<Peer>) -> Session<K> {
        let id = {
            let mut s = peer.shared.lock();
            let id = s.next;
            s.next += 1;
            let slot: Slot<K> = Slot {
                store: Store::new(),
                moved: vec![],
                listener: None,
            };
            s.stores.insert(id, Box::new(slot));
            id
        };
        Session {
            peer,
            id,
            _k: std::marker::PhantomData,
        }
    }

    /// The peer it is over.
    pub fn peer(&self) -> &Arc<Peer> {
        &self.peer
    }

    fn slot(id: u64, s: &mut State) -> &mut Slot<K> {
        s.stores
            .get_mut(&id)
            .and_then(|a| a.any().downcast_mut::<Slot<K>>())
            .expect("a session's store is in its peer's state while the session is")
    }

    /// `f` on the store and the peer, everything settled after and whoever
    /// listens told what moved.
    pub fn with<T>(&self, f: impl FnOnce(&mut Store<K>, &mut ark_peer::Peer) -> T) -> T {
        let id = self.id;
        self.peer.shared.act(|s| {
            let State { stores, peer, .. } = s;
            let slot = stores
                .get_mut(&id)
                .and_then(|a| a.any().downcast_mut::<Slot<K>>())
                .expect("a session's store is in its peer's state while the session is");
            f(&mut slot.store, peer)
        })
    }

    /// `f` on the store as it stands, and the peer: what a reader reads.
    pub fn read<T>(&self, f: impl FnOnce(&Store<K>, &ark_peer::Peer) -> T) -> T {
        let mut s = self.peer.shared.lock();
        let State { stores, peer, .. } = &mut *s;
        let slot = stores
            .get_mut(&self.id)
            .and_then(|a| a.any().downcast_mut::<Slot<K>>())
            .expect("a session's store is in its peer's state while the session is");
        f(&slot.store, peer)
    }

    /// `key` held once more, its views opened the first time; held until
    /// the hold answered is closed or dropped.
    pub fn hold(self: &Arc<Self>, key: K) -> Hold<K> {
        self.with(|st, p| st.hold(p, key.clone()));
        Hold {
            session: self.clone(),
            key: Mutex::new(Some(key)),
        }
    }

    /// A number that grows whenever `key`'s rows moved.
    pub fn version(&self, key: &K) -> u64 {
        self.read(|st, _| st.version(key))
    }

    /// Told the keys that moved, whenever any did; none tells nobody.
    pub fn set_listener(&self, listener: Option<Moved<K>>) {
        let mut s = self.peer.shared.lock();
        Session::<K>::slot(self.id, &mut s).listener = listener;
    }

    /// A change sent as `send` does it (`Peer::mutate`, its span and all),
    /// its refusal kept until the next change is taken.
    pub fn send(&self, send: impl FnOnce(&Peer) -> Result<String, Refusal>) -> Result<String, Refusal> {
        let r = send(&self.peer).map_err(|Refusal::Refused { why }| why);
        let mut s = self.peer.shared.lock();
        Session::<K>::slot(self.id, &mut s).store.sent(r).map_err(Refusal::from)
    }

    /// Why the last change sent was refused, while it is the last.
    pub fn refusal(&self) -> Option<String> {
        self.read(|st, _| st.refusal().map(String::from))
    }
}

impl<K: ViewKey> Drop for Session<K> {
    fn drop(&mut self) {
        let mut s = self.peer.shared.lock();
        if let Some(mut a) = s.stores.remove(&self.id) {
            a.close(&mut s.peer);
        }
    }
}

/// A key held: released when closed or dropped.
pub struct Hold<K: ViewKey> {
    session: Arc<Session<K>>,
    key: Mutex<Option<K>>,
}

impl<K: ViewKey> Hold<K> {
    /// The key held, while it is.
    pub fn key(&self) -> Option<K> {
        self.key.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Held no more; once.
    pub fn close(&self) {
        let key = self.key.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(k) = key {
            self.session.with(|st, p| st.release(p, &k));
        }
    }
}

impl<K: ViewKey> Drop for Hold<K> {
    fn drop(&mut self) {
        self.close();
    }
}

impl Shared {
    /// Whoever each session's listener is, told what moved: run once the
    /// state is let go, so a listener may read.
    pub(crate) fn tell_sessions(&self) {
        let calls: Vec<Box<dyn FnOnce() + Send>> = {
            let mut s = self.lock();
            s.stores.values_mut().filter_map(|a| a.told()).collect()
        };
        for c in calls {
            c();
        }
    }
}
