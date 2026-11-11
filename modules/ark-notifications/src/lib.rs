//! Local notifications: a notice at a time, or one taken back. An update
//! asks with [`schedule`] or [`cancel`], a [`NotificationsOp`] whose
//! output is `()`.
//!
//! No native implementation yet (README.md).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ark_module::{Modules, Op, Reply};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// A local notification: what it says, and when (milliseconds since the
/// epoch; none is now).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Notice {
    /// Its id: a second notice of one id replaces the first.
    pub id: String,
    pub title: String,
    pub body: String,
    pub at_ms: Option<i64>,
    /// The route it opens, as a path (`/account/…`), when tapped.
    pub opens: Option<String>,
}

/// Local notifications.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Notifications: Send + Sync {
    fn schedule(&self, notice: Notice);
    fn cancel(&self, id: String);
}

/// What an update asks of the notifications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationsOp {
    Schedule(Notice),
    Cancel(String),
}

impl Op for NotificationsOp {
    type Module = dyn Notifications;
    type Output = ();
    const MODULE: &'static str = "ark-notifications";
    fn run(&self, m: &dyn Notifications, reply: Reply<()>) {
        match self {
            NotificationsOp::Schedule(n) => m.schedule(n.clone()),
            NotificationsOp::Cancel(id) => m.cancel(id.clone()),
        }
        reply.done(())
    }
}

/// `notice` scheduled, in place of any of its id.
pub fn schedule(notice: Notice) -> NotificationsOp {
    NotificationsOp::Schedule(notice)
}

/// The notice of `id` taken back.
pub fn cancel(id: impl Into<String>) -> NotificationsOp {
    NotificationsOp::Cancel(id.into())
}

/// The notifications, faked for tests: what is scheduled, by id.
#[derive(Clone, Default)]
pub struct Fake {
    notices: Arc<Mutex<BTreeMap<String, Notice>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the notifications.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn Notifications>(Arc::new(self.clone()));
    }

    /// The notices scheduled and not cancelled.
    pub fn notices(&self) -> Vec<Notice> {
        self.notices.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect()
    }
}

impl Notifications for Fake {
    fn schedule(&self, notice: Notice) {
        self.notices.lock().unwrap_or_else(|e| e.into_inner()).insert(notice.id.clone(), notice);
    }

    fn cancel(&self, id: String) {
        self.notices.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_of_one_id_replaces_the_first_and_is_cancelled() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        let n = |title: &str| Notice {
            id: "a".into(),
            title: title.into(),
            body: String::new(),
            at_ms: None,
            opens: None,
        };
        m.run(&schedule(n("one")), |_| {});
        m.run(&schedule(n("two")), |_| {});
        assert_eq!(fake.notices(), vec![n("two")]);
        m.run(&cancel("a"), |_| {});
        assert!(fake.notices().is_empty());
    }
}
