//! The process's one runtime: what an Ark app runs on for as long as its
//! process lives, whatever UI is showing. The replica (its [`Peer`]: the
//! disk, the sync, the session's link) and the device modules the app
//! registered are made on demand, once, and kept here; an Activity, a
//! Scene or a window only attaches a view to them ([`ArkProcess::attach`])
//! and lets go when it closes ([`Attached::detach`]) — closing the UI
//! never stops the runtime. The generated Android `Application` and iOS
//! `App` own [`ArkProcess::shared`]; it replaces a ViewModel or a model
//! object owning the peer per screen. ark-media-player's process player
//! (`ArkMedia`) is the same idea for sound.
//!
//! An app reaches its runtime by [`ArkProcess::open`] with how to open it
//! (its domain's config and disk, in its own crate: money's
//! `money_process_open`); a second open is the first's peer. Sign-out puts
//! it away ([`ArkProcess::close`]) and the next open makes a fresh one.

use std::any::Any;
use std::sync::{Arc, Mutex, OnceLock};

use crate::{Peer, Refusal};

/// The process's runtime.
#[derive(uniffi::Object, Default)]
pub struct ArkProcess {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    peer: Option<Arc<Peer>>,
    /// What the peer was opened as (its disk): another is a new runtime.
    key: String,
    /// The app's own process-wide parts (its device modules, a player),
    /// by name: made once, shared by every attachment.
    parts: Vec<(String, Arc<dyn Any + Send + Sync>)>,
    attached: u32,
    /// How many times the runtime was made: one per process, again only
    /// after a sign-out.
    opened: u32,
}

/// A UI attached to the runtime: an Activity's, a Scene's, a window's.
/// Let go of with [`Attached::detach`], or when it is dropped.
#[derive(uniffi::Object)]
pub struct Attached {
    process: Arc<ArkProcess>,
    done: Mutex<bool>,
}

static SHARED: OnceLock<Arc<ArkProcess>> = OnceLock::new();

impl ArkProcess {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The process's runtime as `key` (its disk: one server's replica),
    /// its peer opened by `open` the first time and the same peer every
    /// time after, until [`ArkProcess::close`]; asked as another key (the
    /// app moved to another server), the old one is let go first.
    pub fn open(&self, key: &str, open: impl FnOnce() -> Result<Arc<Peer>, Refusal>) -> Result<Arc<Peer>, Refusal> {
        let mut i = self.lock();
        if let Some(p) = i.peer.as_ref().filter(|_| i.key == key) {
            return Ok(p.clone());
        }
        let gone = (i.peer.take(), std::mem::take(&mut i.parts));
        drop(gone);
        let p = open()?;
        i.peer = Some(p.clone());
        i.key = key.into();
        i.opened += 1;
        Ok(p)
    }

    /// The app's process-wide part called `name`, made by `make` the first
    /// time it is asked for.
    pub fn part<T: Any + Send + Sync>(&self, name: &str, make: impl FnOnce() -> T) -> Arc<T> {
        let mut i = self.lock();
        if let Some(found) = i.parts.iter().find(|(n, _)| n == name).and_then(|(_, p)| p.clone().downcast::<T>().ok()) {
            return found;
        }
        let made = Arc::new(make());
        i.parts.retain(|(n, _)| n != name);
        i.parts.push((name.into(), made.clone()));
        made
    }
}

#[uniffi::export]
impl ArkProcess {
    /// The one runtime of this process.
    #[uniffi::constructor]
    pub fn shared() -> Arc<ArkProcess> {
        SHARED.get_or_init(|| Arc::new(ArkProcess::default())).clone()
    }

    /// A runtime of its own, apart from the process's: a test's.
    #[uniffi::constructor]
    pub fn apart() -> Arc<ArkProcess> {
        Arc::new(ArkProcess::default())
    }

    /// A UI attached: it draws over the runtime until it detaches.
    pub fn attach(self: Arc<Self>) -> Arc<Attached> {
        self.lock().attached += 1;
        Arc::new(Attached {
            process: self,
            done: Mutex::new(false),
        })
    }

    /// How many UIs are attached now.
    pub fn attached(&self) -> u32 {
        self.lock().attached
    }

    /// The runtime's peer, while it is open: whether or not a UI shows it.
    pub fn peer(&self) -> Option<Arc<Peer>> {
        self.lock().peer.clone()
    }

    /// Whether the runtime is open.
    pub fn running(&self) -> bool {
        self.lock().peer.is_some()
    }

    /// How many times it was made: once a process, again after a sign-out.
    pub fn opened(&self) -> u32 {
        self.lock().opened
    }

    /// Put away (signed out): its peer and its parts let go — closed once
    /// nothing else holds them — and the next open makes it again. The
    /// attached UIs stay attached.
    pub fn close(&self) {
        let gone = {
            let mut i = self.lock();
            (i.peer.take(), std::mem::take(&mut i.parts))
        };
        drop(gone);
    }
}

#[uniffi::export]
impl Attached {
    /// The UI let go of the runtime (its Activity or Scene closed): the
    /// runtime goes on. Twice is once.
    pub fn detach(&self) {
        let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        if !*done {
            *done = true;
            let mut i = self.process.lock();
            i.attached = i.attached.saturating_sub(1);
        }
    }

    /// The runtime it is attached to.
    pub fn process(&self) -> Arc<ArkProcess> {
        self.process.clone()
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        self.detach()
    }
}
