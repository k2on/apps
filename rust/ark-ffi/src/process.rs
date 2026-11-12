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
//! **Online while wanted.** The live connection is open while a UI is
//! attached or background work runs ([`ArkProcess::work`]: the media
//! service playing, a woken task) and closed when neither is, as it was
//! when the activity went; it redials when a UI attaches or work begins.
//! The replica, its session and the state stay in memory either way.
//!
//! An app reaches its runtime by [`ArkProcess::open`] with how to open it
//! (its domain's config and disk, in its own crate: money's
//! `money_process_open`); a second open is the first's peer. Sign-out puts
//! it away ([`ArkProcess::sign_out`]) and the next open makes a fresh one.

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
    /// Background work running now (a player playing, a woken task).
    working: u32,
    /// Whether the live connection is wanted open, as last told.
    online: bool,
    /// Where online and offline are said: the peer's link, or a test's.
    link: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    /// How many times the runtime was made: one per process, again only
    /// after a sign-out.
    opened: u32,
    /// The app's device modules (app.toml's), registered once by the app:
    /// every screen's runtime asks through them. The device's, not the
    /// session's, so a sign-out keeps them.
    modules: ark_app::Modules,
}

/// Background work that keeps the runtime online while it runs, with no
/// UI attached: the media service playing, a woken task. Ended with
/// [`BackgroundWork::end`], or when it is dropped.
#[derive(uniffi::Object)]
pub struct BackgroundWork {
    process: Arc<ArkProcess>,
    done: Mutex<bool>,
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

    /// Online while a UI is attached or work runs, offline when neither:
    /// told to the link only when it changes.
    fn follow(&self) {
        let (link, online) = {
            let mut i = self.lock();
            let want = i.attached > 0 || i.working > 0;
            if want == i.online {
                return;
            }
            i.online = want;
            let link = i.link.clone().or_else(|| {
                i.peer
                    .clone()
                    .map(|p| Arc::new(move |on| p.set_online(on)) as Arc<dyn Fn(bool) + Send + Sync>)
            });
            (link, want)
        };
        if let Some(link) = link {
            link(online)
        }
    }

    /// Online and offline said to `link` instead of the peer: a test's host.
    pub fn with_link(&self, link: impl Fn(bool) + Send + Sync + 'static) {
        self.lock().link = Some(Arc::new(link));
    }

    /// The process's runtime as `key` (its disk: one server's replica),
    /// its peer opened by `open` the first time and the same peer every
    /// time after, until [`ArkProcess::sign_out`]; asked as another key (the
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
        // Opened with nothing to show it or work for it: offline until there is.
        let wanted = i.attached > 0 || i.working > 0;
        i.online = wanted;
        if !wanted {
            let link = i.link.clone();
            drop(i);
            match link {
                Some(link) => link(false),
                None => p.set_online(false),
            }
        }
        Ok(p)
    }

    /// The app's device modules registered: the process's from now on, for
    /// every screen of every UI attached (a second registration replaces
    /// the first).
    pub fn register_modules(&self, modules: ark_app::Modules) {
        self.lock().modules = modules;
    }

    /// The device modules registered; none before the app registers them.
    pub fn modules(&self) -> ark_app::Modules {
        self.lock().modules.clone()
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
        self.follow();
        Arc::new(Attached {
            process: self,
            done: Mutex::new(false),
        })
    }

    /// Background work begun (the media service playing, a woken task):
    /// online while it runs, whether or not a UI is attached.
    pub fn work(self: Arc<Self>) -> Arc<BackgroundWork> {
        self.lock().working += 1;
        self.follow();
        Arc::new(BackgroundWork {
            process: self,
            done: Mutex::new(false),
        })
    }

    /// Whether the live connection is open: a UI attached or work running.
    pub fn online(&self) -> bool {
        self.lock().online
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
    pub fn sign_out(&self) {
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
            drop(i);
            self.process.follow();
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

#[uniffi::export]
impl BackgroundWork {
    /// The work done: offline again if no UI is attached and nothing else
    /// runs. Twice is once.
    pub fn end(&self) {
        let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        if !*done {
            *done = true;
            let mut i = self.process.lock();
            i.working = i.working.saturating_sub(1);
            drop(i);
            self.process.follow();
        }
    }
}

impl Drop for BackgroundWork {
    fn drop(&mut self) {
        self.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With a fake host for the link: attached, online; detached and idle,
    /// offline; a wake-up's work online again, then offline once it ends;
    /// a UI and work together stay online until both are gone. Falsified
    /// by going offline when the last UI goes while work runs.
    #[test]
    fn online_while_a_ui_or_work_wants_it() {
        let said = Arc::new(Mutex::new(vec![]));
        let p = ArkProcess::apart();
        let s = said.clone();
        p.with_link(move |on| s.lock().unwrap().push(on));
        let ui = p.clone().attach();
        assert!(p.online());
        ui.detach();
        assert!(!p.online());
        let woken = p.clone().work();
        assert!(p.online(), "a wake-up connects");
        let ui = p.clone().attach();
        woken.end();
        assert!(p.online(), "the UI still wants it");
        let playing = p.clone().work();
        drop(ui);
        assert!(p.online(), "the player still plays");
        drop(playing);
        assert!(!p.online(), "idle: offline");
        assert_eq!(*said.lock().unwrap(), [true, false, true, false]);
    }

    /// The device modules are the process's: registered once, every
    /// runtime's, and kept across a sign-out (the device's, not the
    /// session's); registered again, replaced. Falsified by letting them go
    /// with the session's parts.
    #[test]
    fn the_modules_are_the_processs() {
        trait Buzz: Send + Sync {}
        struct Phone;
        impl Buzz for Phone {}
        let p = ArkProcess::apart();
        assert!(p.modules().get::<dyn Buzz>().is_none());
        let mut m = ark_app::Modules::new();
        m.set(Some(Arc::new(Phone) as Arc<dyn Buzz>));
        p.register_modules(m);
        assert!(p.modules().get::<dyn Buzz>().is_some());
        p.sign_out();
        assert!(p.modules().get::<dyn Buzz>().is_some(), "kept across a sign-out");
        p.register_modules(ark_app::Modules::new());
        assert!(p.modules().get::<dyn Buzz>().is_none(), "replaced");
    }
}
