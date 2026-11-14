//! The OS wakes the process (ui/docs/app.md, Background And Notifications):
//! a periodic sync (Android's job, the iPhone's app refresh task), a push,
//! a button on a notice, the device started. Each is a message into the
//! process's one runtime — [`ark_app::Woken`], which the app's update takes
//! like any other — and never a second runtime of the platform's own.
//!
//! The platform's component that was woken (a `JobService`, a
//! `BroadcastReceiver`, a `BGAppRefreshTask`'s handler, the app delegate's
//! remote-notification callback) calls [`ArkProcess::wake`] with its time
//! budget and is told when to finish: once the app's runtime is idle —
//! its `Command::Sync` answered, every module's answer in, the notice
//! scheduled — or the budget is nearly spent. While it runs the wake is
//! work ([`ArkProcess::work`]): the runtime is online for it, dials, syncs,
//! and goes offline again when it is done and no UI is attached.
//!
//! The app's runtime is handed to the process as a [`Wakeful`] by the app's
//! own bridge (the process runtime's step owns it); until one is, a wake
//! has nothing to run and finishes at once. The notifications' events are
//! turned into wakes by [`ArkProcess::notices`].

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ark_app::Woken;
use ark_notifications::phone::NotificationListener;
use ark_notifications::NotificationEvent;

use crate::ArkProcess;

/// The app's runtime, as a wake reaches it.
pub trait Wakeful: Send + Sync {
    /// `reason` taken as the app's message ([`ark_app::Runtime::wake`]);
    /// whether the app had one.
    fn woken(&self, reason: Woken) -> bool;
    /// Whether nothing is left to come ([`ark_app::Runtime::is_idle`]).
    fn idle(&self) -> bool;
}

/// Why the OS woke the process, as the phones say it.
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum WakeReason {
    Sync,
    Push,
    Action { notice: String, action: String },
    Boot,
}

impl From<WakeReason> for Woken {
    fn from(r: WakeReason) -> Woken {
        match r {
            WakeReason::Sync => Woken::Sync,
            WakeReason::Push => Woken::Push,
            WakeReason::Action { notice, action } => Woken::Action { notice, action },
            WakeReason::Boot => Woken::Boot,
        }
    }
}

/// Where the woken component is told to finish.
#[uniffi::export(with_foreign)]
pub trait WakeDone: Send + Sync {
    /// The runtime is done with the wake (`idle`), or its time ran out.
    fn finished(&self, idle: bool);
}

/// How often a wake looks whether the runtime is idle.
const LOOK: Duration = Duration::from_millis(50);

/// The process's part that holds the app's runtime for a wake.
#[derive(Default)]
struct Slot(Mutex<Option<Arc<dyn Wakeful>>>);

impl ArkProcess {
    /// The app's runtime, as wakes reach it: what its bridge hands over once
    /// the runtime is made.
    pub fn set_wakeful(&self, w: Option<Arc<dyn Wakeful>>) {
        *self.part("ark.wakeful", Slot::default).0.lock().unwrap_or_else(|e| e.into_inner()) = w;
    }

    fn wakeful(&self) -> Option<Arc<dyn Wakeful>> {
        self.part("ark.wakeful", Slot::default)
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// `reason` into the app's runtime, the process online meanwhile, and
    /// `then` told on a thread of its own once the runtime is idle or
    /// `budget` has passed.
    pub fn woken(self: Arc<Self>, reason: Option<Woken>, budget: Duration, then: impl FnOnce(bool) + Send + 'static) {
        let work = self.clone().work();
        let app = self.wakeful();
        if let (Some(app), Some(reason)) = (&app, reason) {
            app.woken(reason);
        }
        std::thread::spawn(move || {
            let until = Instant::now() + budget;
            let idle = loop {
                let idle = app.as_ref().is_none_or(|a| a.idle());
                if idle || Instant::now() >= until {
                    break idle;
                }
                std::thread::sleep(LOOK);
            };
            work.end();
            then(idle)
        });
    }
}

#[uniffi::export]
impl ArkProcess {
    /// The OS woke the process for `reason`, with `budget_ms` to spend:
    /// the app's runtime told, online meanwhile, and `done` told once it is
    /// idle or the time is nearly up. What a `JobService`, a receiver or a
    /// background task's handler calls.
    pub fn wake(self: Arc<Self>, reason: WakeReason, budget_ms: u64, done: Arc<dyn WakeDone>) {
        self.woken(Some(reason.into()), Duration::from_millis(budget_ms), move |idle| done.finished(idle))
    }

    /// `done` told once the app's runtime is idle or `budget_ms` has
    /// passed, online meanwhile: what a component holds on to after its
    /// event was told some other way (a notice's button, a push, through
    /// [`ArkProcess::notices`]).
    pub fn settle(self: Arc<Self>, budget_ms: u64, done: Arc<dyn WakeDone>) {
        self.woken(None, Duration::from_millis(budget_ms), move |idle| done.finished(idle))
    }

    /// The device's push endpoint (`ark_push::Endpoint`'s JSON, as the
    /// notifications handed it) written as this session's push row
    /// (`session.push`) over the runtime's own connection; empty, taken
    /// away. Nothing while the runtime is not open.
    pub fn push_endpoint(&self, endpoint: String) -> Result<(), crate::Refusal> {
        let Some(peer) = self.peer() else { return Ok(()) };
        let input = serde_json::json!({ "endpoint": endpoint }).to_string();
        peer.mutate(ark::auth::PUSH.into(), input).map(drop)
    }

    /// What the notifications are to tell the process: a push or a
    /// notice's button a wake of the app's runtime, an endpoint handed out
    /// the session's push row, an unregistration that row taken away. Registered with the phone's notifications
    /// once, at launch.
    pub fn notices(self: Arc<Self>) -> Arc<dyn NotificationListener> {
        Arc::new(Notices(self))
    }
}

/// The notifications' events, into the process.
struct Notices(Arc<ArkProcess>);

impl NotificationListener for Notices {
    fn heard(&self, event: NotificationEvent) {
        let reason = match event {
            NotificationEvent::Pushed => Woken::Push,
            NotificationEvent::Action { notice, action } => Woken::Action { notice, action },
            NotificationEvent::Registered { endpoint } => {
                let e = ark_push::Endpoint::from(endpoint).to_json();
                if let Err(why) = self.0.push_endpoint(e) {
                    tracing::warn!(why = ?why, "the push endpoint was not written");
                }
                return;
            }
            // Turned off: the session's push row taken away.
            NotificationEvent::Unregistered => {
                if let Err(why) = self.0.push_endpoint(String::new()) {
                    tracing::warn!(why = ?why, "the push endpoint was not taken away");
                }
                return;
            }
            // A tap opened its route through the app's links; a failure to
            // register leaves the device unwoken and is said where it is.
            NotificationEvent::Opened { .. } | NotificationEvent::Failed { .. } => return,
        };
        // The component that told it holds on through `settle`; this only
        // hands the runtime its message and keeps it online meanwhile.
        self.0.clone().woken(Some(reason), Duration::from_secs(25), |_| {})
    }
}
