//! Notifications: a notice now or at a time, taken back, the person asked
//! once, and the device registered for push. Its port — the
//! [`Notifications`] and [`NotificationListener`] traits, the calls
//! ([`schedule`], [`cancel`], [`register_push`], [`unregister_push`], each a
//! [`NotificationsOp`] answered `()`, and [`authorize`], answered whether
//! notices may be shown), the [`Notice`], the [`PushEndpoint`] and the
//! [`NotificationEvent`]s — is the runtime's (`ark_module::notifications`),
//! which an update asks through; here is what implements it:
//!
//! - iPhone, iPad and Mac: `PhoneNotifications` (swift/:
//!   `UNUserNotificationCenter`, APNs through the app delegate), through
//!   `phone` (feature `ffi`).
//! - Android: `PhoneNotifications` (kotlin/: `NotificationManager`, an
//!   `AlarmManager` for a later time, notification channels, and the
//!   UnifiedPush receiver protocol, implemented directly), through `phone`.
//! - The desktop and the browser: none; every operation answers
//!   `Unavailable` (README.md).
//! - Tests: [`fake::Fake`].

pub use ark_module::notifications::*;

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

pub mod fake;
pub use fake::Fake;

/// The phones' notifications, through UniFFI (`ffi`): the foreign trait
/// Swift's and Kotlin's `PhoneNotifications` implement, the listener they
/// tell, the reply a permission is answered through, it registered as the
/// port; and Android's UnifiedPush keys and the opening of a wake.
#[cfg(feature = "ffi")]
pub mod phone {
    use std::sync::Arc;

    use super::{Notice, NoticeAction, NotificationEvent, PushEndpoint};

    #[uniffi::remote(Record)]
    pub struct NoticeAction {
        pub id: String,
        pub title: String,
    }

    #[uniffi::remote(Record)]
    pub struct Notice {
        pub id: String,
        pub title: String,
        pub body: String,
        pub at_ms: Option<i64>,
        pub opens: Option<String>,
        pub actions: Vec<NoticeAction>,
        pub channel: Option<String>,
    }

    #[uniffi::remote(Enum)]
    pub enum PushEndpoint {
        Apns { token: String, topic: String, sandbox: bool },
        UnifiedPush { url: String, p256dh: String, auth: String },
    }

    #[uniffi::remote(Enum)]
    pub enum NotificationEvent {
        Opened { id: String, opens: Option<String> },
        Action { notice: String, action: String },
        Registered { endpoint: PushEndpoint },
        Unregistered,
        Failed { why: String },
        Pushed,
    }

    /// Where Swift and Kotlin tell what happened: the process's runtime.
    #[uniffi::export(with_foreign)]
    pub trait NotificationListener: Send + Sync {
        fn heard(&self, event: NotificationEvent);
    }

    /// Whether notices may be shown, answered once the person has said.
    #[derive(uniffi::Object)]
    pub struct AuthorizeReply(ark_module::Reply<bool>);

    #[uniffi::export]
    impl AuthorizeReply {
        pub fn allowed(&self, allowed: bool) {
            self.0.done(allowed)
        }
        pub fn failed(&self, why: String) {
            self.0.failed(why)
        }
    }

    /// The phone's notifications, as Swift and Kotlin implement them.
    /// Every call returns at once.
    #[uniffi::export(with_foreign)]
    pub trait NativeNotifications: Send + Sync {
        fn schedule(&self, notice: Notice);
        fn cancel(&self, id: String);
        fn authorize(&self, reply: Arc<AuthorizeReply>);
        fn register_push(&self);
        fn unregister_push(&self);
        fn set_listener(&self, listener: Option<Arc<dyn NotificationListener>>);
    }

    struct Phone(Arc<dyn NativeNotifications>);

    /// The port's listener, as the phone's.
    struct Tell(Arc<dyn super::NotificationListener>);

    impl NotificationListener for Tell {
        fn heard(&self, event: NotificationEvent) {
            self.0.heard(event)
        }
    }

    impl super::Notifications for Phone {
        fn schedule(&self, notice: Notice) {
            self.0.schedule(notice)
        }
        fn cancel(&self, id: String) {
            self.0.cancel(id)
        }
        fn authorize(&self, reply: ark_module::Reply<bool>) {
            self.0.authorize(Arc::new(AuthorizeReply(reply)))
        }
        fn register_push(&self) {
            self.0.register_push()
        }
        fn unregister_push(&self) {
            self.0.unregister_push()
        }
        fn set_listener(&self, listener: Option<Arc<dyn super::NotificationListener>>) {
            self.0.set_listener(listener.map(|l| Arc::new(Tell(l)) as Arc<dyn NotificationListener>))
        }
    }

    /// The phone's notifications as the port an update asks through.
    pub fn port(n: Arc<dyn NativeNotifications>) -> Arc<dyn super::Notifications> {
        Arc::new(Phone(n))
    }

    /// A device's Web Push keys (Android's UnifiedPush): `kept` in the
    /// app's private storage (the private key and the secret), `p256dh` and
    /// `auth` handed to the server with the endpoint.
    #[derive(uniffi::Record)]
    pub struct WebPushKeys {
        pub kept: String,
        pub p256dh: String,
        pub auth: String,
    }

    /// New Web Push keys.
    #[uniffi::export]
    pub fn web_push_keys() -> WebPushKeys {
        let k = ark_push::webpush::Keys::generate();
        WebPushKeys {
            kept: k.to_private(),
            p256dh: k.p256dh(),
            auth: k.auth(),
        }
    }

    /// A UnifiedPush message's body opened with the device's kept keys: the
    /// app the wake is for, which proves it sealed by its server; none when
    /// it does not open or is not a wake. A wake carries nothing else.
    #[uniffi::export]
    pub fn web_push_wake(kept: String, body: Vec<u8>) -> Option<String> {
        let keys = ark_push::webpush::Keys::from_private(&kept).ok()?;
        let plain = ark_push::webpush::open(&keys, &body).ok()?;
        ark_push::Wake::read(std::str::from_utf8(&plain).ok()?)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_sealed_wake_opens_with_the_kept_keys() {
            let k = web_push_keys();
            let w = ark_push::Wake::of("money");
            let body = ark_push::webpush::seal(&k.p256dh, &k.auth, w.json().as_bytes()).unwrap();
            assert_eq!(web_push_wake(k.kept.clone(), body).as_deref(), Some("money"));
            assert_eq!(web_push_wake(web_push_keys().kept, vec![1, 2, 3]), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use ark_module::{ModuleError, Modules};

    use super::*;

    fn notice(id: &str, title: &str) -> Notice {
        Notice {
            id: id.into(),
            title: title.into(),
            ..Notice::default()
        }
    }

    #[test]
    fn a_notice_of_one_id_replaces_the_first_and_is_cancelled() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        m.run(&schedule(notice("a", "one")), |_| {});
        m.run(&schedule(notice("a", "two")), |_| {});
        assert_eq!(fake.notices(), vec![notice("a", "two")]);
        m.run(&cancel("a"), |_| {});
        assert!(fake.notices().is_empty());
    }

    #[test]
    fn the_person_is_asked_and_their_answer_comes_back() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        fake.allow(false);
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        m.run(&authorize(), move |a| g.lock().unwrap().push(a));
        assert_eq!(*got.lock().unwrap(), vec![Ok(false)]);
        assert_eq!(fake.asked(), 1);
    }

    struct Log(Mutex<Vec<NotificationEvent>>);
    impl NotificationListener for Log {
        fn heard(&self, e: NotificationEvent) {
            self.0.lock().unwrap().push(e)
        }
    }

    #[test]
    fn a_registration_hands_out_the_endpoint_as_an_event() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        let log = Arc::new(Log(Mutex::new(vec![])));
        fake.set_listener(Some(log.clone()));
        m.run(&register_push(), |_| {});
        let endpoint = PushEndpoint::Apns {
            token: "ab".into(),
            topic: "us.koon.money".into(),
            sandbox: true,
        };
        fake.hand_out(Some(endpoint.clone()));
        m.run(&register_push(), |_| {});
        assert!(fake.registered());
        m.run(&unregister_push(), |_| {});
        assert!(!fake.registered());
        assert_eq!(
            *log.0.lock().unwrap(),
            vec![
                NotificationEvent::Failed {
                    why: "no distributor".into()
                },
                NotificationEvent::Registered { endpoint },
                NotificationEvent::Unregistered
            ]
        );
    }

    #[test]
    fn without_notifications_every_operation_is_unavailable() {
        let m = Modules::new();
        m.run(&schedule(notice("a", "one")), |a| assert_eq!(a, Err(ModuleError::Unavailable)));
        m.run(&authorize(), |a| assert_eq!(a, Err(ModuleError::Unavailable)));
    }
}
