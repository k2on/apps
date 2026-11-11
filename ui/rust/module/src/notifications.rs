//! The notifications' port: a notice shown now or at a time, taken back,
//! the person asked once whether the app may show any, and the device
//! registered for push. Its implementations — `UNUserNotificationCenter`
//! and APNs on the iPhone, the iPad and the Mac; `NotificationManager`, an
//! `AlarmManager` for a later time and a UnifiedPush distributor on
//! Android — and its fake are the module's (modules/ark-notifications,
//! `ark-notifications`). The desktop and the browser have none: their
//! operations answer `Unavailable`.
//!
//! An update asks with the calls ([`schedule`], [`cancel`],
//! [`register_push`], [`unregister_push`], each a [`NotificationsOp`]
//! answered `()`, and [`authorize`], an [`Authorize`] answered whether
//! notices may be shown). What happens later — a notice tapped, one of its
//! actions pressed, the device's push endpoint handed out, a push come in —
//! comes back through the [`NotificationListener`] the process registers,
//! as a [`NotificationEvent`]: the process's runtime turns an action and a
//! push into a wake-up of its own (`ark_app`'s `Woken`), so the app hears
//! them with no screen open.
//!
//! **A tap opens the notice's route through the app's links.** A notice's
//! [`Notice::opens`] is a route's path (`/account/…`); tapped, the platform
//! opens the app at `<scheme>://<path>` exactly as a link from anywhere else
//! would, so the route is the app's to resolve, and nothing new is needed of
//! its shell. The tap is said too ([`NotificationEvent::Opened`]).

use std::sync::Arc;

use crate::{Op, Reply};

/// A button on a notice: its id (what [`NotificationEvent::Action`] names
/// it by) and its title. Pressed, the app hears it without being opened.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NoticeAction {
    pub id: String,
    pub title: String,
}

/// A notice: what it says, when (milliseconds since the epoch; none is
/// now), where a tap takes the person, its buttons and the channel it is
/// shown under.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Notice {
    /// Its id: a second notice of one id replaces the first.
    pub id: String,
    pub title: String,
    pub body: String,
    pub at_ms: Option<i64>,
    /// The route it opens when tapped, as a path (`/account/…`).
    pub opens: Option<String>,
    /// Its buttons, at most three (what every platform shows).
    pub actions: Vec<NoticeAction>,
    /// What kind of notice it is, in the person's words ("To review"): the
    /// channel Android lists it under in the app's settings, the thread the
    /// iPhone groups it in. None is the app's one general kind.
    pub channel: Option<String>,
}

/// Where a push reaches this device: what the app's server is told and
/// sends to (`ark_push`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushEndpoint {
    /// Apple's push service: the device token (hex), the app's bundle id
    /// (APNs' topic) and whether it is the development environment's.
    Apns { token: String, topic: String, sandbox: bool },
    /// A UnifiedPush distributor's endpoint (Android: the person's own
    /// choice of distributor, FCM's among them) and the Web Push keys its
    /// messages are encrypted to (RFC 8291): the device's P-256 public key
    /// and the auth secret, both base64url without padding.
    UnifiedPush { url: String, p256dh: String, auth: String },
}

/// What the notifications tell the process, on their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationEvent {
    /// A notice was tapped: its id, and the route it opens (already opened
    /// through the app's links).
    Opened { id: String, opens: Option<String> },
    /// One of a notice's buttons was pressed (`notice` is its id, `action`
    /// the button's). The app was not opened for it: the runtime hears it as
    /// a wake-up.
    Action { notice: String, action: String },
    /// The device is registered for push, here.
    Registered { endpoint: PushEndpoint },
    /// It could not register, in the platform's words (no distributor, no
    /// network, the person said no).
    Failed { why: String },
    /// A push came in: a wake-up and nothing else (it carries none of the
    /// app's data). The runtime hears it as `Woken(Push)`: it dials its
    /// server, syncs, and says what came from its own rows.
    Pushed,
}

/// Where the notifications tell what happened: the process's runtime.
pub trait NotificationListener: Send + Sync {
    fn heard(&self, event: NotificationEvent);
}

/// A device's notifications.
pub trait Notifications: Send + Sync {
    /// `notice` shown now or at its time, in place of any of its id.
    fn schedule(&self, notice: Notice);
    /// The notice of `id` taken back, shown or still to come.
    fn cancel(&self, id: String);
    /// Whether notices may be shown, the person asked the first time
    /// (iPhone: `requestAuthorization`; Android 13 and later:
    /// `POST_NOTIFICATIONS`), answered once they have said.
    fn authorize(&self, reply: Reply<bool>);
    /// The device registered for push: the endpoint comes back as
    /// [`NotificationEvent::Registered`], or [`NotificationEvent::Failed`].
    fn register_push(&self);
    /// No more pushes to this device.
    fn unregister_push(&self);
    fn set_listener(&self, listener: Option<Arc<dyn NotificationListener>>);
}

/// What an update asks of the notifications, answered `()` at once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationsOp {
    Schedule(Notice),
    Cancel(String),
    RegisterPush,
    UnregisterPush,
}

impl Op for NotificationsOp {
    type Module = dyn Notifications;
    type Output = ();
    const MODULE: &'static str = "ark-notifications";
    fn run(&self, m: &dyn Notifications, reply: Reply<()>) {
        match self {
            NotificationsOp::Schedule(n) => m.schedule(n.clone()),
            NotificationsOp::Cancel(id) => m.cancel(id.clone()),
            NotificationsOp::RegisterPush => m.register_push(),
            NotificationsOp::UnregisterPush => m.unregister_push(),
        }
        reply.done(())
    }
}

/// Whether notices may be shown: the person asked the first time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Authorize;

impl Op for Authorize {
    type Module = dyn Notifications;
    type Output = bool;
    const MODULE: &'static str = "ark-notifications";
    fn run(&self, m: &dyn Notifications, reply: Reply<bool>) {
        m.authorize(reply)
    }
}

/// `notice` shown now or at its time, in place of any of its id.
pub fn schedule(notice: Notice) -> NotificationsOp {
    NotificationsOp::Schedule(notice)
}

/// The notice of `id` taken back.
pub fn cancel(id: impl Into<String>) -> NotificationsOp {
    NotificationsOp::Cancel(id.into())
}

/// The device registered for push ([`NotificationEvent::Registered`]).
pub fn register_push() -> NotificationsOp {
    NotificationsOp::RegisterPush
}

/// No more pushes to this device.
pub fn unregister_push() -> NotificationsOp {
    NotificationsOp::UnregisterPush
}

/// Whether notices may be shown, the person asked the first time.
pub fn authorize() -> Authorize {
    Authorize
}
