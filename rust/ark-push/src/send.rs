//! The [`PushSender`] port's adapters, a server's (feature `send`): a
//! [`Wake`] to an [`Endpoint`], through APNs or to a UnifiedPush endpoint.
//! Blocking: `ark_peer`'s hub calls it from a thread of its own.
//!
//! **APNs** speaks HTTP/2 only. The sender signs a provider token (ES256,
//! the team's `.p8` key, its key id and team id: Apple's token-based
//! authentication) and keeps it for fifty minutes — Apple refuses one
//! older than an hour, and one made anew more often than every twenty
//! minutes — and POSTs to `/3/device/<token>` on `api.push.apple.com`, or
//! the sandbox's for a development build's token. A wake with the app's
//! words is an `alert` push (`mutable-content`, so an app's Notification
//! Service Extension may reword it from the device's own data); one without
//! a `background` push (`content-available`), which the system throttles,
//! holds back or drops.
//!
//! **UnifiedPush** is Web Push: the wake, as JSON, sealed to the device's
//! keys ([`crate::webpush::seal`]) and POSTed to the endpoint
//! (`Content-Encoding: aes128gcm`, a `TTL`, `Urgency: high`), whoever the
//! distributor is. VAPID is not sent: UnifiedPush's distributors take a
//! push without it.
//!
//! What came of each is an [`Outcome`]: delivered, gone (the device
//! unregistered, or its token is no more: the server forgets the
//! endpoint), or failed for now.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use bytes::Bytes;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::DecodePrivateKey;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{payload, webpush, Endpoint, Outcome, PushSender, Wake};

/// Apple's push service, production and development.
pub const APNS: &str = "https://api.push.apple.com";
pub const APNS_SANDBOX: &str = "https://api.sandbox.push.apple.com";

/// How long a push service may hold a wake for a device that is off: a
/// day, after which the device syncs when it is next opened anyway.
const TTL_S: u64 = 24 * 60 * 60;

/// How long a provider token is used: under Apple's hour, over its twenty
/// minutes.
const TOKEN_S: u64 = 50 * 60;

/// APNs, as a server is configured with it.
pub struct Apns {
    key_id: String,
    team_id: String,
    key: SigningKey,
    /// Where it is reached, in place of Apple's: a test's fake (`http://`,
    /// HTTP/2 without TLS) or a proxy.
    base: Option<String>,
    token: Mutex<Option<(u64, String)>>,
}

impl std::fmt::Debug for Apns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Apns(key {}, team {})", self.key_id, self.team_id)
    }
}

impl Apns {
    /// APNs with the team's key (the `.p8` file's PEM, PKCS #8), its id and
    /// the team's.
    pub fn new(key_pem: &str, key_id: &str, team_id: &str) -> Result<Apns, String> {
        let key = SigningKey::from_pkcs8_pem(key_pem).map_err(|e| format!("the APNs key is not a .p8 key: {e}"))?;
        Ok(Apns {
            key_id: key_id.into(),
            team_id: team_id.into(),
            key,
            base: None,
            token: Mutex::new(None),
        })
    }

    /// APNs with the key in the file at `path` (a systemd credential).
    pub fn from_file(path: &std::path::Path, key_id: &str, team_id: &str) -> Result<Apns, String> {
        let pem = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Apns::new(&pem, key_id, team_id)
    }

    /// Reached at `base` in place of Apple's, for either environment.
    pub fn at(mut self, base: &str) -> Apns {
        self.base = Some(base.trim_end_matches('/').into());
        self
    }

    /// The provider token at `now_s`: the one kept while it is young enough.
    fn bearer(&self, now_s: u64) -> String {
        let mut kept = self.token.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, t)) = &*kept {
            if now_s.saturating_sub(*at) < TOKEN_S {
                return t.clone();
            }
        }
        let header = B64.encode(serde_json::json!({"alg": "ES256", "kid": self.key_id}).to_string());
        let claims = B64.encode(serde_json::json!({"iss": self.team_id, "iat": now_s}).to_string());
        let input = format!("{header}.{claims}");
        let sig: Signature = self.key.sign(input.as_bytes());
        let t = format!("{input}.{}", B64.encode(sig.to_bytes()));
        *kept = Some((now_s, t.clone()));
        t
    }

    fn base(&self, sandbox: bool) -> &str {
        match (&self.base, sandbox) {
            (Some(b), _) => b,
            (None, false) => APNS,
            (None, true) => APNS_SANDBOX,
        }
    }
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

/// One HTTP/2 request: its path, its headers and its body.
struct Request {
    path: String,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

/// Sends pushes: to APNs when it was configured with it, and to any
/// UnifiedPush endpoint.
pub struct Sender {
    apns: Option<Arc<Apns>>,
    http: ureq::Agent,
}

impl Sender {
    /// A sender; `apns` none, an APNs endpoint fails ("not configured").
    pub fn new(apns: Option<Apns>) -> Sender {
        Sender {
            apns: apns.map(Arc::new),
            http: ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).build(),
        }
    }

    /// `m` to one endpoint.
    pub fn send(&self, to: &Endpoint, m: &Wake) -> Outcome {
        self.send_all(std::slice::from_ref(to), m).remove(0)
    }

    /// `m` to every endpoint, what came of each in order: APNs' over one
    /// connection for each environment.
    pub fn send_all(&self, to: &[Endpoint], m: &Wake) -> Vec<Outcome> {
        let mut out = vec![Outcome::Failed("not sent".into()); to.len()];
        for sandbox in [false, true] {
            let mine: Vec<usize> = (0..to.len())
                .filter(|i| matches!(&to[*i], Endpoint::Apns { sandbox: s, .. } if *s == sandbox))
                .collect();
            if mine.is_empty() {
                continue;
            }
            let got = self.apns(mine.iter().map(|i| &to[*i]), m, sandbox);
            for (i, o) in mine.into_iter().zip(got) {
                out[i] = o;
            }
        }
        for (i, e) in to.iter().enumerate() {
            if let Endpoint::UnifiedPush { url, p256dh, auth } = e {
                out[i] = self.unified(url, p256dh, auth, &payload(e, m));
            }
        }
        out
    }

    fn unified(&self, url: &str, p256dh: &str, auth: &str, plain: &[u8]) -> Outcome {
        let body = match webpush::seal(p256dh, auth, plain) {
            Ok(b) => b,
            Err(e) => return Outcome::Failed(e.0),
        };
        // High: a wake is what the person is waiting on. One topic: a newer
        // wake replaces one the distributor still holds (RFC 8030).
        let req = self
            .http
            .post(url)
            .set("Content-Encoding", "aes128gcm")
            .set("Content-Type", "application/octet-stream")
            .set("TTL", &TTL_S.to_string())
            .set("Urgency", "high")
            .set("Topic", "wake");
        match req.send_bytes(&body) {
            Ok(_) => Outcome::Delivered,
            Err(ureq::Error::Status(404 | 410, _)) => Outcome::Gone,
            Err(ureq::Error::Status(code, r)) => Outcome::Failed(format!("{code} {}", r.into_string().unwrap_or_default())),
            Err(e) => Outcome::Failed(e.to_string()),
        }
    }

    fn apns<'e>(&self, to: impl Iterator<Item = &'e Endpoint>, m: &Wake, sandbox: bool) -> Vec<Outcome> {
        let to: Vec<&Endpoint> = to.collect();
        let Some(apns) = &self.apns else {
            return to
                .iter()
                .map(|_| Outcome::Failed("APNs is not configured on this server".into()))
                .collect();
        };
        let bearer = apns.bearer(now_s());
        let shown = m.alert.is_some();
        let expiration = now_s() + TTL_S;
        let requests: Vec<Request> = to
            .iter()
            .map(|e| {
                let Endpoint::Apns { token, topic, .. } = e else {
                    unreachable!("APNs' endpoints only")
                };
                let headers = vec![
                    ("authorization", format!("bearer {bearer}")),
                    ("apns-topic", topic.clone()),
                    ("apns-push-type", if shown { "alert" } else { "background" }.into()),
                    ("apns-priority", if shown { "10" } else { "5" }.into()),
                    ("apns-expiration", expiration.to_string()),
                    // A newer wake replaces one still shown.
                    ("apns-collapse-id", format!("{}.wake", m.app)),
                    ("content-type", "application/json".into()),
                ];
                Request {
                    path: format!("/3/device/{token}"),
                    headers,
                    body: payload(e, m),
                }
            })
            .collect();
        let n = requests.len();
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => return vec![Outcome::Failed(e.to_string()); n],
        };
        let base = apns.base(sandbox).to_string();
        match rt.block_on(async move { tokio::time::timeout(std::time::Duration::from_secs(30), exchange(&base, requests)).await }) {
            Ok(Ok(answers)) => answers.into_iter().map(outcome).collect(),
            Ok(Err(e)) => vec![Outcome::Failed(e); n],
            Err(_) => vec![Outcome::Failed("APNs did not answer in time".into()); n],
        }
    }
}

/// What APNs' answer means: 200 taken; 410, or a token it does not know,
/// gone; anything else its reason.
fn outcome(a: Result<(u16, String), String>) -> Outcome {
    match a {
        Ok((200, _)) => Outcome::Delivered,
        Ok((410, _)) => Outcome::Gone,
        Ok((400, body)) if body.contains("BadDeviceToken") || body.contains("DeviceTokenNotForTopic") => Outcome::Gone,
        Ok((code, body)) => Outcome::Failed(format!("{code} {body}")),
        Err(e) => Outcome::Failed(e),
    }
}

impl PushSender for Sender {
    fn wake(&self, to: &[Endpoint], wake: &Wake) -> Vec<Outcome> {
        self.send_all(to, wake)
    }
}

/// `base` (`https://host[:port]`, or `http://` for HTTP/2 without TLS)
/// split into whether it is TLS, its host and its port.
fn parse_base(base: &str) -> Result<(bool, String, u16), String> {
    let (tls, rest) = match base.split_once("://") {
        Some(("https", r)) => (true, r),
        Some(("http", r)) => (false, r),
        _ => return Err(format!("{base} is not an http(s) address")),
    };
    let rest = rest.split('/').next().unwrap_or(rest);
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => (h, p.parse().map_err(|_| format!("{base}: not a port"))?),
        _ => (rest, if tls { 443 } else { 80 }),
    };
    Ok((tls, host.trim_matches(['[', ']']).to_string(), port))
}

/// `requests` sent over one HTTP/2 connection to `base`; each one's status
/// and body, or why it got none.
async fn exchange(base: &str, requests: Vec<Request>) -> Result<Vec<Result<(u16, String), String>>, String> {
    let (tls, host, port) = parse_base(base)?;
    let tcp = tokio::net::TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| format!("{base}: {e}"))?;
    let _ = tcp.set_nodelay(true);
    let authority = format!("{host}:{port}");
    let scheme = if tls { "https" } else { "http" };
    if tls {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec()];
        let name = rustls::pki_types::ServerName::try_from(host.clone()).map_err(|e| e.to_string())?;
        let stream = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await
            .map_err(|e| format!("{base}: {e}"))?;
        over(stream, scheme, &authority, requests).await
    } else {
        over(tcp, scheme, &authority, requests).await
    }
}

async fn over<T>(io: T, scheme: &str, authority: &str, requests: Vec<Request>) -> Result<Vec<Result<(u16, String), String>>, String>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (client, connection) = h2::client::handshake(io).await.map_err(|e| e.to_string())?;
    let driving = tokio::spawn(async move {
        let _ = connection.await;
    });
    let mut out = Vec::with_capacity(requests.len());
    for r in requests {
        out.push(one(client.clone(), scheme, authority, r).await);
    }
    drop(client);
    driving.abort();
    Ok(out)
}

async fn one(client: h2::client::SendRequest<Bytes>, scheme: &str, authority: &str, r: Request) -> Result<(u16, String), String> {
    let mut client = client.ready().await.map_err(|e| e.to_string())?;
    let mut req = http::Request::post(format!("{scheme}://{authority}{}", r.path));
    for (k, v) in &r.headers {
        req = req.header(*k, v);
    }
    let req = req.body(()).map_err(|e| e.to_string())?;
    let (response, mut stream) = client.send_request(req, false).map_err(|e| e.to_string())?;
    stream.send_data(Bytes::from(r.body), true).map_err(|e| e.to_string())?;
    let response = response.await.map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let mut body = response.into_body();
    let mut text = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        let _ = body.flow_control().release_capacity(chunk.len());
        text.extend_from_slice(&chunk);
    }
    Ok((status, String::from_utf8_lossy(&text).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_base_is_split() {
        assert_eq!(
            parse_base("https://api.push.apple.com").unwrap(),
            (true, "api.push.apple.com".into(), 443)
        );
        assert_eq!(parse_base("http://127.0.0.1:8443").unwrap(), (false, "127.0.0.1".into(), 8443));
        assert_eq!(parse_base("http://[::1]:9").unwrap(), (false, "::1".into(), 9));
        assert!(parse_base("ftp://x").is_err());
    }

    #[test]
    fn a_wake_with_words_is_an_alert_and_one_without_a_background_push() {
        let to = Endpoint::Apns {
            token: "ab".into(),
            topic: "us.koon.money".into(),
            sandbox: false,
        };
        let n: serde_json::Value = serde_json::from_slice(&payload(&to, &Wake::of("money").shown("Money", "New activity in Money"))).unwrap();
        assert_eq!(n["aps"]["alert"]["body"], "New activity in Money");
        assert_eq!(n["aps"]["mutable-content"], 1);
        assert_eq!(n["ark"], serde_json::json!({"wake": "money"}));
        let w: serde_json::Value = serde_json::from_slice(&payload(&to, &Wake::of("money"))).unwrap();
        assert_eq!(w["aps"], serde_json::json!({"content-available": 1}));
    }
}
