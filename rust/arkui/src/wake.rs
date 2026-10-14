//! What wakes a window, for an app with no loop (`docs/plan-reactive.md`).
//!
//! A window used to be driven by a 50 ms clock: everything that might have
//! happened — a frame on the socket, the back button, a media key, the end
//! of a track — was looked for twenty times a second, whether it had or
//! not. Here each of those *says so* instead, and a window at rest runs
//! nothing.
//!
//! - A [`Bell`] is a subscription anything can ring from anywhere: a
//!   socket's reader thread, a `WebSocket` callback, a lock-screen button's
//!   handler. Rings that land before the app has answered are one message.
//! - [`at`] is a one-shot timer for a deadline — the redial after a close,
//!   a second of quiet — keyed by the deadline itself, so the same deadline
//!   asked for on every `subscription()` call is one timer, and a moved one
//!   replaces it.
//! - [`page_changes`] calls back on the browser's `popstate` and
//!   `hashchange`: the address bar read when it moves rather than polled.
//!
//! A clock that is genuinely needed — an animation, a scrubber while audio
//! plays — is still `iced::time::every`, subscribed only while it is.

use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::StreamExt;
use iced::Subscription;

/// A subscription that yields `()` whenever it has been rung since it last
/// yielded. Cheap to clone; every clone rings the same one.
#[derive(Clone)]
pub struct Bell(Arc<Inner>);

struct Inner {
    /// Which bell, so two windows' subscriptions are two.
    id: u64,
    /// The running subscription's end of the channel, once it runs.
    tx: Mutex<Option<mpsc::UnboundedSender<()>>>,
    /// Rung with nobody listening yet: said as the subscription starts.
    missed: AtomicBool,
    /// Every ring, for a test or a debug screen.
    rung: AtomicU64,
}

impl Default for Bell {
    fn default() -> Self {
        Bell::new()
    }
}

impl std::fmt::Debug for Bell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bell").field("id", &self.0.id).field("rung", &self.rung()).finish()
    }
}

impl Bell {
    pub fn new() -> Bell {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Bell(Arc::new(Inner {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            tx: Mutex::new(None),
            missed: AtomicBool::new(false),
            rung: AtomicU64::new(0),
        }))
    }

    /// Ask for one message. From any thread, any number of times.
    pub fn ring(&self) {
        self.0.rung.fetch_add(1, Ordering::Relaxed);
        let tx = self.0.tx.lock().unwrap_or_else(|e| e.into_inner());
        match tx.as_ref() {
            Some(tx) if tx.unbounded_send(()).is_ok() => {}
            _ => self.0.missed.store(true, Ordering::SeqCst),
        }
    }

    /// How many times it has been rung.
    pub fn rung(&self) -> u64 {
        self.0.rung.load(Ordering::Relaxed)
    }

    /// The messages: one per burst of rings that landed before the app took
    /// the last one.
    pub fn subscription(&self) -> Subscription<()> {
        Subscription::run_with(Keyed(self.clone()), |k: &Keyed| {
            let (tx, rx) = mpsc::unbounded();
            let inner = &(k.0).0;
            if inner.missed.swap(false, Ordering::SeqCst) {
                let _ = tx.unbounded_send(());
            }
            *inner.tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
            rx.ready_chunks(64).map(|_| ())
        })
    }
}

/// A bell as a subscription's identity: its id, and nothing else.
struct Keyed(Bell);

impl Hash for Keyed {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "arkui::wake::Bell".hash(state);
        (self.0).0.id.hash(state);
    }
}

/// One `()` at the deadline `key` names, `delay` from when this is first
/// asked for. Keyed by `key` alone: asked for again with the same key — on
/// every `subscription()`, with a `delay` that has shrunk since — it is the
/// timer already running; with another key it is a new one, and the old is
/// dropped. So a host keyed by a deadline arms exactly one timer for it.
pub fn at(key: u64, delay: Duration) -> Subscription<()> {
    Subscription::run_with(Due { key, delay }, |d: &Due| iced::futures::stream::once(sleep(d.delay)))
}

struct Due {
    key: u64,
    delay: Duration,
}

impl Hash for Due {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "arkui::wake::at".hash(state);
        self.key.hash(state);
    }
}

/// A future that is ready `d` from now: a thread natively, `setTimeout` in
/// a browser — no runtime of its own either way.
fn sleep(d: Duration) -> impl std::future::Future<Output = ()> {
    let (tx, rx) = iced::futures::channel::oneshot::channel::<()>();
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::spawn(move || {
            std::thread::sleep(d);
            let _ = tx.send(());
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let fire = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = tx.send(());
        });
        if let Some(w) = web_sys::window() {
            let ms = d.as_millis().min(i32::MAX as u128) as i32;
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(fire.unchecked_ref(), ms);
        }
    }
    async move {
        let _ = rx.await;
    }
}

/// Call `f` whenever the browser moves the address bar itself — the back
/// button (`popstate`), a fragment typed or followed (`hashchange`) — for
/// the life of the page. Nothing on the desktop, which has no bar. Writing
/// the bar through the history fires neither, which is why
/// [`crate::route::Router::sync`] does.
pub fn page_changes(f: impl Fn() + 'static) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let Some(w) = web_sys::window() else { return };
        let cb = wasm_bindgen::closure::Closure::<dyn Fn()>::new(f).into_js_value();
        for event in ["popstate", "hashchange"] {
            let _ = w.add_event_listener_with_callback(event, cb.unchecked_ref());
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = f;
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::subscription::{into_recipes, EventStream};
    use iced::futures::executor::block_on;
    use iced::futures::stream::{self, BoxStream};
    use std::time::Instant;

    /// The next item, or `None` after a second of nothing.
    fn next(s: &mut BoxStream<'static, ()>) -> Option<()> {
        let (tx, rx) = iced::futures::channel::oneshot::channel::<()>();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(1));
            let _ = tx.send(());
        });
        block_on(async {
            match iced::futures::future::select(s.next(), rx).await {
                iced::futures::future::Either::Left((got, _)) => got,
                iced::futures::future::Either::Right(_) => None,
            }
        })
    }

    fn stream_of(s: Subscription<()>) -> BoxStream<'static, ()> {
        let mut recipes = into_recipes(s);
        assert_eq!(recipes.len(), 1);
        let quiet: EventStream = stream::pending().boxed();
        recipes.remove(0).stream(quiet)
    }

    /// Rung before the app listens is still said once it does, a burst of
    /// rings is one message, and a ring after is another. Falsified by not
    /// keeping `missed`: the first message never comes; and by yielding each
    /// ring rather than each ready chunk: fifty rings are fifty messages.
    #[test]
    fn a_bell_says_each_burst_once_and_nothing_is_lost_before_it_runs() {
        let bell = Bell::new();
        bell.ring();
        let mut s = stream_of(bell.subscription());
        assert_eq!(next(&mut s), Some(()), "the ring before anyone listened");
        for _ in 0..50 {
            bell.ring();
        }
        assert_eq!(next(&mut s), Some(()));
        assert_eq!(next(&mut s), None, "fifty rings, one message");
        let other = bell.clone();
        std::thread::spawn(move || other.ring());
        assert_eq!(next(&mut s), Some(()), "from another thread");
        assert_eq!(bell.rung(), 52);
    }

    /// The deadline fires once, after its delay, and is keyed by the
    /// deadline: the same key twice is one subscription's identity.
    /// Falsified by hashing the delay too: the two hash apart.
    #[test]
    fn a_deadline_fires_once_and_is_one_timer_however_often_asked_for() {
        let t = Instant::now();
        let mut s = stream_of(at(7, Duration::from_millis(60)));
        assert_eq!(next(&mut s), Some(()));
        assert!(t.elapsed() >= Duration::from_millis(60));
        assert_eq!(next(&mut s), None, "once");
        let key = |s: Subscription<()>| {
            let mut h = iced::advanced::subscription::Hasher::default();
            into_recipes(s)[0].hash(&mut h);
            h.finish()
        };
        assert_eq!(key(at(7, Duration::from_millis(60))), key(at(7, Duration::from_millis(10))));
        assert_ne!(key(at(7, Duration::from_millis(60))), key(at(8, Duration::from_millis(60))));
    }
}
