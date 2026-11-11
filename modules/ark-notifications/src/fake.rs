//! The notifications, faked for tests: what is scheduled, by id; whether
//! the person said yes; the endpoint a registration hands out; and events
//! told to the listener when a test says.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use ark_module::{Modules, Reply};

use crate::{Notice, NotificationEvent, NotificationListener, Notifications, PushEndpoint};

struct Seen {
    notices: BTreeMap<String, Notice>,
    allowed: bool,
    asked: u32,
    endpoint: Option<PushEndpoint>,
    registered: bool,
    listener: Option<Arc<dyn NotificationListener>>,
}

impl Default for Seen {
    fn default() -> Seen {
        Seen {
            notices: BTreeMap::new(),
            allowed: true,
            asked: 0,
            endpoint: None,
            registered: false,
            listener: None,
        }
    }
}

#[derive(Clone, Default)]
pub struct Fake {
    seen: Arc<Mutex<Seen>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    fn seen(&self) -> MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// This fake registered as the notifications.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn Notifications>(Arc::new(self.clone()));
    }

    /// The notices scheduled and not cancelled, by id.
    pub fn notices(&self) -> Vec<Notice> {
        self.seen().notices.values().cloned().collect()
    }

    /// What the person says when asked (yes, unless a test says no).
    pub fn allow(&self, allowed: bool) {
        self.seen().allowed = allowed
    }

    /// How many times the person was asked.
    pub fn asked(&self) -> u32 {
        self.seen().asked
    }

    /// The endpoint a registration hands out; none, it fails.
    pub fn hand_out(&self, endpoint: Option<PushEndpoint>) {
        self.seen().endpoint = endpoint
    }

    /// Whether the device is registered for push.
    pub fn registered(&self) -> bool {
        self.seen().registered
    }

    /// `event` told to the listener, as the platform would.
    pub fn hear(&self, event: NotificationEvent) {
        let l = self.seen().listener.clone();
        if let Some(l) = l {
            l.heard(event)
        }
    }
}

impl Notifications for Fake {
    fn schedule(&self, notice: Notice) {
        self.seen().notices.insert(notice.id.clone(), notice);
    }

    fn cancel(&self, id: String) {
        self.seen().notices.remove(&id);
    }

    fn authorize(&self, reply: Reply<bool>) {
        let allowed = {
            let mut s = self.seen();
            s.asked += 1;
            s.allowed
        };
        reply.done(allowed)
    }

    fn register_push(&self) {
        let event = {
            let mut s = self.seen();
            s.registered = s.endpoint.is_some();
            match s.endpoint.clone() {
                Some(endpoint) => NotificationEvent::Registered { endpoint },
                None => NotificationEvent::Failed {
                    why: "no distributor".into(),
                },
            }
        };
        self.hear(event)
    }

    fn unregister_push(&self) {
        self.seen().registered = false
    }

    fn set_listener(&self, listener: Option<Arc<dyn NotificationListener>>) {
        self.seen().listener = listener
    }
}
