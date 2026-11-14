//! Push for any Ark app (ui/docs/app.md, Background And Notifications):
//! not one app's, so a server only decides what to send.
//!
//! - [`Endpoint`] — where a signed-in device is reached: an APNs device
//!   token (the iPhone, the iPad, the Mac) or a UnifiedPush endpoint with
//!   its Web Push keys (Android, through the distributor the person chose).
//!   A device writes it as its session's push row (`ark::auth::Push`, the
//!   function `session.push`), over its signed-in connection.
//! - [`Wake`] — what a server sends: a wake-up and nothing of the rows,
//!   the app it is for and, on an iPhone, the app's own generic words. The
//!   device's runtime syncs and builds its notices from its own data.
//! - [`PushSender`] — the server's port (`ark_peer`'s hub wakes the
//!   devices of sessions not connected through it), and its [`fake`].
//! - [`webpush`] — RFC 8291's encryption, both ends: the server seals a
//!   UnifiedPush wake, the phone opens it (and so knows it is its server's).
//! - `send` (feature `send`, a server's): `send::Sender`, the port's
//!   adapters: APNs over HTTP/2 with a token signed by the team's key, and
//!   a UnifiedPush endpoint by an HTTP POST of the sealed wake.
//!
//! No key is ever in the repository or the store: APNs' is a file a server
//! is configured with (`ark_peer::settings`, `_APNS_KEY_FILE`).

use serde::{Deserialize, Serialize};

pub mod webpush;

#[cfg(feature = "send")]
pub mod send;

/// Where a push reaches a device, as the device hands it to its server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Endpoint {
    /// Apple's push service: the device token (hex), the app's bundle id
    /// (the topic) and whether it is the development environment's.
    Apns {
        token: String,
        topic: String,
        #[serde(default)]
        sandbox: bool,
    },
    /// A UnifiedPush endpoint and the Web Push keys its messages are sealed
    /// to (base64url).
    #[serde(rename = "unifiedpush")]
    UnifiedPush { url: String, p256dh: String, auth: String },
}

impl Endpoint {
    /// As a device writes it into its session's push row.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("an endpoint serializes")
    }

    /// A push row's endpoint read; none when it is not one.
    pub fn from_json(text: &str) -> Option<Endpoint> {
        serde_json::from_str(text).ok()
    }

    /// Whether it is well formed: a token of hex, an `https` endpoint (or a
    /// loopback `http` one, a test's), keys that decode. What a server
    /// checks before it keeps one.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Endpoint::Apns { token, topic, .. } => {
                if token.is_empty() || token.len() > 200 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("an APNs token is hex".into());
                }
                if topic.is_empty() || topic.len() > 200 || topic.contains(['/', ' ']) {
                    return Err("an APNs topic is the app's bundle id".into());
                }
                Ok(())
            }
            Endpoint::UnifiedPush { url, p256dh, auth } => {
                let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
                    .iter()
                    .any(|p| url.starts_with(p));
                if !(url.starts_with("https://") || loopback) || url.len() > 2000 {
                    return Err("a UnifiedPush endpoint is an https URL".into());
                }
                webpush::seal(p256dh, auth, b"").map(|_| ()).map_err(|e| e.0)
            }
        }
    }
}

impl From<ark_module::notifications::PushEndpoint> for Endpoint {
    fn from(e: ark_module::notifications::PushEndpoint) -> Endpoint {
        use ark_module::notifications::PushEndpoint as P;
        match e {
            P::Apns { token, topic, sandbox } => Endpoint::Apns { token, topic, sandbox },
            P::UnifiedPush { url, p256dh, auth } => Endpoint::UnifiedPush { url, p256dh, auth },
        }
    }
}

/// What a wake says that is not the rows': the app it is for, and on an
/// iPhone the visible text it is shown with.
///
/// **A push is only a wake-up.** It never carries the app's data: no name,
/// amount or word from a row. A device it reaches wakes its one runtime,
/// which dials its server and syncs as it always does, and builds whatever
/// it tells the person from the rows it then holds (ui/docs/app.md,
/// Background And Notifications). So the push services — Apple's, the
/// person's UnifiedPush distributor, Google's behind one — learn only that
/// there was something.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Wake {
    /// The app's id (`money`): what the device checks a wake is for.
    pub app: String,
    /// What an iPhone shows: generic and the app's own ("New activity in
    /// Money"), never the rows'. Apple throttles a silent push and may drop
    /// it, so a visible one is the default there; an app's Notification
    /// Service Extension may rewrite it from the device's own data. None, a
    /// silent push. UnifiedPush shows nothing: the device's runtime builds
    /// its notices itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert: Option<Alert>,
}

/// An iPhone's visible text for a wake: the app's words, never the rows'.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Alert {
    pub title: String,
    pub body: String,
}

impl Wake {
    /// A silent wake for `app`.
    pub fn of(app: impl Into<String>) -> Wake {
        Wake {
            app: app.into(),
            alert: None,
        }
    }

    /// Shown on an iPhone as `title` and `body`.
    pub fn shown(mut self, title: impl Into<String>, body: impl Into<String>) -> Wake {
        self.alert = Some(Alert {
            title: title.into(),
            body: body.into(),
        });
        self
    }

    /// What a device reads from a wake's own bytes (a UnifiedPush message
    /// opened, APNs' `ark` key): only the app it is for.
    pub fn json(&self) -> String {
        serde_json::json!({ "wake": self.app }).to_string()
    }

    /// The app a wake's JSON is for; none when it is not a wake.
    pub fn read(json: &str) -> Option<String> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        v.get("wake")?.as_str().map(str::to_string)
    }
}

/// What came of one wake.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The push service took it.
    Delivered,
    /// The endpoint is no more (unregistered, a token Apple no longer
    /// knows): its row is let go.
    Gone,
    /// Not now, in the service's words: kept, and tried at the next wake.
    Failed(String),
}

/// The server's port for waking devices: APNs and UnifiedPush are its
/// adapters ([`send::Sender`], feature `send`), a test's is a fake that
/// keeps what it would have sent ([`fake::Fake`]). Blocking.
pub trait PushSender: Send + Sync {
    /// `wake` to every endpoint, what came of each in order.
    fn wake(&self, to: &[Endpoint], wake: &Wake) -> Vec<Outcome>;
}

/// The bytes a wake is to `to`, before any sealing: APNs' JSON, or a
/// UnifiedPush message's plaintext. What the adapters send, and what a
/// test reads to see that nothing of the rows is in it.
pub fn payload(to: &Endpoint, wake: &Wake) -> Vec<u8> {
    match to {
        Endpoint::Apns { .. } => {
            let aps = match &wake.alert {
                Some(a) => serde_json::json!({
                    "alert": {"title": a.title, "body": a.body},
                    "sound": "default",
                    "mutable-content": 1,
                }),
                None => serde_json::json!({"content-available": 1}),
            };
            serde_json::json!({"aps": aps, "ark": {"wake": wake.app}}).to_string().into_bytes()
        }
        Endpoint::UnifiedPush { .. } => wake.json().into_bytes(),
    }
}

pub mod fake {
    //! The port faked for tests: every wake kept with the bytes it would
    //! have been sent as ([`crate::payload`]), each endpoint's outcome as
    //! the test says (delivered, unless it said gone).

    use std::sync::{Arc, Mutex};

    use crate::{payload, Endpoint, Outcome, PushSender, Wake};

    /// One wake the fake was asked to send.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Sent {
        pub to: Endpoint,
        pub wake: Wake,
        pub payload: Vec<u8>,
    }

    #[derive(Clone, Default)]
    pub struct Fake {
        sent: Arc<Mutex<Vec<Sent>>>,
        gone: Arc<Mutex<Vec<Endpoint>>>,
    }

    impl Fake {
        pub fn new() -> Fake {
            Fake::default()
        }

        /// What was sent, taken.
        pub fn take(&self) -> Vec<Sent> {
            std::mem::take(&mut *self.sent.lock().unwrap_or_else(|e| e.into_inner()))
        }

        /// `to` answered gone from now on.
        pub fn gone(&self, to: Endpoint) {
            self.gone.lock().unwrap_or_else(|e| e.into_inner()).push(to)
        }
    }

    impl PushSender for Fake {
        fn wake(&self, to: &[Endpoint], wake: &Wake) -> Vec<Outcome> {
            let gone = self.gone.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let mut sent = self.sent.lock().unwrap_or_else(|e| e.into_inner());
            to.iter()
                .map(|e| {
                    sent.push(Sent {
                        to: e.clone(),
                        wake: wake.clone(),
                        payload: payload(e, wake),
                    });
                    if gone.contains(e) {
                        Outcome::Gone
                    } else {
                        Outcome::Delivered
                    }
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_endpoint_reads_as_the_device_writes_it() {
        let e: Endpoint = serde_json::from_str(r#"{"apns":{"token":"ab12","topic":"us.koon.money"}}"#).unwrap();
        assert_eq!(
            e,
            Endpoint::Apns {
                token: "ab12".into(),
                topic: "us.koon.money".into(),
                sandbox: false
            }
        );
        assert!(e.check().is_ok());
        let keys = webpush::Keys::generate();
        let u = Endpoint::UnifiedPush {
            url: "https://ntfy.sh/upAbc".into(),
            p256dh: keys.p256dh(),
            auth: keys.auth(),
        };
        let text = serde_json::to_string(&u).unwrap();
        assert!(text.starts_with(r#"{"unifiedpush":{"url""#), "{text}");
        assert_eq!(serde_json::from_str::<Endpoint>(&text).unwrap(), u);
        assert!(u.check().is_ok());
    }

    #[test]
    fn a_bad_endpoint_is_refused() {
        let bad = Endpoint::Apns {
            token: "not hex".into(),
            topic: "us.koon.money".into(),
            sandbox: false,
        };
        assert!(bad.check().is_err());
        let keys = webpush::Keys::generate();
        let plain = Endpoint::UnifiedPush {
            url: "http://example.com/up".into(),
            p256dh: keys.p256dh(),
            auth: keys.auth(),
        };
        assert!(plain.check().is_err());
        let keyless = Endpoint::UnifiedPush {
            url: "https://example.com/up".into(),
            p256dh: "AAAA".into(),
            auth: keys.auth(),
        };
        assert!(keyless.check().is_err());
    }
}
