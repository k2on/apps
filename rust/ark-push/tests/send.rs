//! The sender against a fake APNs (HTTP/2 without TLS, as Apple's speaks
//! it under TLS) and a fake UnifiedPush endpoint (a distributor's HTTP
//! POST), each keeping what it was sent.
#![cfg(feature = "send")]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex};

use ark_push::send::{Apns, Sender};
use ark_push::webpush::{self, Keys};
use ark_push::{Endpoint, Outcome, Wake};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use bytes::Bytes;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::{EncodePrivateKey, LineEnding};

/// One request a fake was sent: its path, its headers, its body.
#[derive(Clone, Debug)]
struct Seen {
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

type Log = Arc<Mutex<Vec<Seen>>>;

/// A fake APNs on a loopback port: 200 for a token, 410 for one ending in
/// `dead`, 400 BadDeviceToken for one ending in `bad`.
fn fake_apns() -> (String, Log) {
    let log: Log = Arc::default();
    let l = log.clone();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                let l = l.clone();
                tokio::spawn(async move {
                    let mut conn = h2::server::handshake(sock).await.unwrap();
                    while let Some(Ok((req, mut respond))) = conn.accept().await {
                        let path = req.uri().path().to_string();
                        let headers = req
                            .headers()
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                            .collect();
                        let mut body = req.into_body();
                        let mut bytes = vec![];
                        while let Some(Ok(c)) = body.data().await {
                            let _ = body.flow_control().release_capacity(c.len());
                            bytes.extend_from_slice(&c);
                        }
                        l.lock().unwrap().push(Seen {
                            path: path.clone(),
                            headers,
                            body: bytes,
                        });
                        let (status, reason) = match () {
                            _ if path.ends_with("dead") => (410, r#"{"reason":"Unregistered"}"#),
                            _ if path.ends_with("bad") => (400, r#"{"reason":"BadDeviceToken"}"#),
                            _ => (200, ""),
                        };
                        let resp = http::Response::builder().status(status).body(()).unwrap();
                        let mut s = respond.send_response(resp, reason.is_empty()).unwrap();
                        if !reason.is_empty() {
                            s.send_data(Bytes::from(reason), true).unwrap();
                        }
                    }
                });
            }
        })
    });
    (format!("http://127.0.0.1:{port}"), log)
}

/// A fake UnifiedPush endpoint: 201 for `/up/…`, 404 for `/gone/…`.
fn fake_unified() -> (String, Log) {
    let log: Log = Arc::default();
    let l = log.clone();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let mut s = s.unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let mut headers = BTreeMap::new();
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                let h = h.trim_end();
                if h.is_empty() {
                    break;
                }
                let (k, v) = h.split_once(':').unwrap();
                headers.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
            let n: usize = headers.get("content-length").map_or(0, |v| v.parse().unwrap());
            let mut body = vec![0; n];
            r.read_exact(&mut body).unwrap();
            let gone = path.starts_with("/gone");
            l.lock().unwrap().push(Seen { path, headers, body });
            let status = if gone { "404 Not Found" } else { "201 Created" };
            write!(s, "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        }
    });
    (format!("http://127.0.0.1:{port}"), log)
}

/// A team's key as Apple hands it (a `.p8`, PKCS #8 PEM), and what checks
/// its signatures.
fn team_key() -> (String, VerifyingKey) {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).unwrap();
    let secret = p256::SecretKey::from_slice(&b).unwrap();
    let pem = secret.to_pkcs8_pem(LineEnding::LF).unwrap().to_string();
    (pem, VerifyingKey::from(secret.public_key()))
}

fn apns(token: &str) -> Endpoint {
    Endpoint::Apns {
        token: token.into(),
        topic: "us.koon.money".into(),
        sandbox: false,
    }
}

#[test]
fn a_wake_reaches_apns_signed_and_a_dead_token_is_gone() {
    let (base, log) = fake_apns();
    let (pem, verify) = team_key();
    let sender = Sender::new(Some(Apns::new(&pem, "KEY123", "TEAM456").unwrap().at(&base)));
    let w = Wake::of("money").shown("Money", "New activity in Money");
    let out = sender.send_all(&[apns("ab12"), apns("cddead"), apns("eebad")], &w);
    assert_eq!(out, vec![Outcome::Delivered, Outcome::Gone, Outcome::Gone]);

    let seen = log.lock().unwrap().clone();
    assert_eq!(seen.len(), 3);
    let first = &seen[0];
    assert_eq!(first.path, "/3/device/ab12");
    assert_eq!(first.headers["apns-topic"], "us.koon.money");
    assert_eq!(first.headers["apns-push-type"], "alert");
    assert_eq!(first.headers["apns-priority"], "10");
    assert_eq!(first.headers["apns-collapse-id"], "money.wake");
    let body: serde_json::Value = serde_json::from_slice(&first.body).unwrap();
    assert_eq!(body["aps"]["alert"]["body"], "New activity in Money");
    assert_eq!(body["ark"], serde_json::json!({"wake": "money"}));

    // The provider token: ES256 by the team's key, its key id and team.
    let bearer = first.headers["authorization"].strip_prefix("bearer ").unwrap();
    let parts: Vec<&str> = bearer.split('.').collect();
    let header: serde_json::Value = serde_json::from_slice(&B64.decode(parts[0]).unwrap()).unwrap();
    let claims: serde_json::Value = serde_json::from_slice(&B64.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(header, serde_json::json!({"alg": "ES256", "kid": "KEY123"}));
    assert_eq!(claims["iss"], "TEAM456");
    let sig = Signature::from_slice(&B64.decode(parts[2]).unwrap()).unwrap();
    verify.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig).unwrap();
    // One token for every push while it is young.
    assert!(seen.iter().all(|s| s.headers["authorization"] == first.headers["authorization"]));
}

#[test]
fn a_silent_wake_is_a_background_push() {
    let (base, log) = fake_apns();
    let (pem, _) = team_key();
    let sender = Sender::new(Some(Apns::new(&pem, "K", "T").unwrap().at(&base)));
    assert_eq!(sender.send(&apns("ab"), &Wake::of("money")), Outcome::Delivered);
    let seen = log.lock().unwrap().clone();
    assert_eq!(seen[0].headers["apns-push-type"], "background");
    assert_eq!(seen[0].headers["apns-priority"], "5");
    let body: serde_json::Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(body["aps"], serde_json::json!({"content-available": 1}));
}

#[test]
fn without_apns_configured_an_apns_endpoint_fails_and_is_kept() {
    let out = Sender::new(None).send(&apns("ab"), &Wake::of("money"));
    assert!(matches!(out, Outcome::Failed(why) if why.contains("not configured")));
}

#[test]
fn a_unifiedpush_wake_is_sealed_to_the_device_and_opens_there() {
    let (base, log) = fake_unified();
    let keys = Keys::generate();
    let to = |path: &str| Endpoint::UnifiedPush {
        url: format!("{base}{path}"),
        p256dh: keys.p256dh(),
        auth: keys.auth(),
    };
    let sender = Sender::new(None);
    let w = Wake::of("money").shown("Money", "New activity in Money");
    assert_eq!(
        sender.send_all(&[to("/up/abc"), to("/gone/def")], &w),
        vec![Outcome::Delivered, Outcome::Gone]
    );

    let seen = log.lock().unwrap().clone();
    assert_eq!(seen[0].path, "/up/abc");
    assert_eq!(seen[0].headers["content-encoding"], "aes128gcm");
    assert_eq!(seen[0].headers["urgency"], "high");
    assert_eq!(seen[0].headers["topic"], "wake");
    // The distributor holds only ciphertext; the device opens a wake for
    // its app and nothing else (no iPhone's words either).
    assert!(!String::from_utf8_lossy(&seen[0].body).contains("money"));
    let opened = webpush::open(&keys, &seen[0].body).unwrap();
    assert_eq!(Wake::read(std::str::from_utf8(&opened).unwrap()).as_deref(), Some("money"));
    assert_eq!(opened, br#"{"wake":"money"}"#);
}
