//! money's server in this process on a real port, peers that pump against
//! it over its WebSocket as a desktop or a phone does, and a scripted Plaid
//! on loopback for the importer to ask.
#![allow(dead_code)]

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ark::format::Encode;
use ark::log::EntryId;
use ark::peer::Who;
use ark_auth::{Account, Login};
use ark_peer::Memory;
use money_server::importer::{Banks, Mapping};
use money_server::{start, Config, Server};
use serde_json::{json, Value};

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
}

/// A dev-auth server on an ephemeral port, told whatever `more` adds.
pub fn serve(rt: &tokio::runtime::Runtime, data: &Path, more: impl FnOnce(&mut Config)) -> Server {
    let mut config = Config::dev("127.0.0.1:0", data);
    more(&mut config);
    rt.block_on(start(config)).expect("the server starts")
}

/// A login on `server` for `user`, issued as the sign-in routes would.
pub fn login(server: &Server, user: &str) -> Login {
    let account = Account {
        id: user.into(),
        ..Account::default()
    };
    server.auth.issue(&account, "a test").unwrap()
}

/// A client of money: a peer in memory and its link.
pub struct Peer {
    pub client: ark_peer::Peer,
    pub refused: Vec<String>,
}

pub fn open(who: Who) -> Peer {
    let config = ark_peer::Config::of(money_domain::shared());
    let mut client = ark_peer::Peer::open(config, who, Box::new(Memory::default()), false);
    client.link.backoff = (20, 200);
    Peer { client, refused: vec![] }
}

/// `user`, signed in on this server as a device of their own, dialling.
pub fn signed_in(server: &Server, user: &str) -> Peer {
    let l = login(server, user);
    let mut p = open(ark_peer::who(&l));
    p.connect(&server.url(), &l.token);
    p
}

impl Peer {
    pub fn connect(&mut self, server: &str, token: &str) {
        self.client.set_token(Some(token.into()));
        self.client.connect(&ark_auth::socket_url(server));
    }

    pub fn pump(&mut self) -> ark_peer::Pumped {
        let p = self.client.pump();
        self.refused.extend(self.client.take_rejections().into_iter().map(|(_, why)| why));
        p
    }

    /// Whether the link has said `Closed` since the last look, pumping.
    pub fn closed(&mut self) -> bool {
        self.client.link.token.is_none() || !self.pump().closed.is_empty()
    }

    pub fn pending(&self) -> usize {
        self.client.log.pending.len()
    }

    pub fn cursor(&self) -> i64 {
        self.client.log.log().head().0
    }

    pub fn mutate(&mut self, name: &str, input: &impl Encode) -> EntryId {
        let id = self.client.mutate(name, input).unwrap_or_else(|why| {
            self.refused.push(why);
            EntryId::default()
        });
        self.pump();
        id
    }

    pub fn query<T: Encode>(&self, name: &str, input: &impl Encode) -> Result<T, String> {
        self.client.query(name, input)
    }
}

/// A table's rows as the hub's log holds them.
pub fn hub_rows<T: Encode + Send + 'static>(server: &Server, table: &'static str) -> Vec<T> {
    server
        .running
        .hub
        .with(move |h| h.log.store.rows(table).map(|r| T::from_bytes(r).unwrap()).collect())
        .unwrap()
}

/// A query over the hub's store, as a household member reads it.
pub fn hub_query<T: Encode + Send + 'static>(server: &Server, name: &'static str, input: impl Encode + Send + 'static) -> T {
    server
        .running
        .hub
        .with(move |h| {
            let ctx = ark::peer::Ctx {
                id: EntryId::of("q", 0),
                actor: "alice".into(),
                session: "q".into(),
                ..Default::default()
            };
            let d = money_domain::shared();
            let f = d.binding(name).unwrap().to_string();
            let b = d.ask(&h.log.store, &ctx, &f, &input.to_bytes()).unwrap_or_else(|r| panic!("{name}: {r}"));
            T::from_bytes(&b).unwrap()
        })
        .unwrap()
}

pub fn head(server: &Server) -> i64 {
    server.running.hub.with(|h| h.log.log().head().0).unwrap()
}

/// Pump every peer until `done` says so, or `secs` pass.
pub fn pump_until(peers: &mut [&mut Peer], secs: u64, mut done: impl FnMut(&mut [&mut Peer]) -> bool) {
    let held = ark_peer::link::until(Duration::from_secs(secs), Duration::from_millis(5), || {
        for p in peers.iter_mut() {
            p.pump();
        }
        done(peers)
    });
    assert!(
        held,
        "timed out; {:#?}",
        peers.iter().map(|p| (p.cursor(), p.pending(), &p.refused)).collect::<Vec<_>>()
    );
}

/// Wait for `f` to hold, or `secs` pass: whether it did.
pub fn eventually(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    ark_peer::link::until(Duration::from_secs(secs), Duration::from_millis(20), &mut f)
}

// * Plaid, scripted

/// The client id and the secret the mock takes, and the token it knows.
pub const CLIENT_ID: &str = "client-id-0f3a";
pub const SECRET: &str = "secret-77c2e1";
pub const TOKEN: &str = "access-test-5d9b";

/// What the scripted Plaid answers, and what it was asked.
#[derive(Default)]
pub struct Script {
    /// Which of the three phases `/transactions/sync` is in; a fourth
    /// fails its second page.
    pub phase: u8,
    /// Every `/transactions/sync` asked: its cursor (none the first time).
    pub cursors: Vec<Option<String>>,
    /// Every request's body, whole: what the importer sent Plaid.
    pub bodies: Vec<String>,
}

/// ArkOS's mock Plaid (`sync/actual-import/src/test/mock-plaid.ts`), whose
/// answers `money/domain/tests/import.rs` walks the domain through: one
/// bank of two accounts, its initial pull over two pages with a pending
/// transaction and a payroll; that transaction posting with another amount
/// while the coffee is modified; and nothing new.
pub struct Mock {
    pub url: String,
    pub script: Arc<Mutex<Script>>,
}

impl Mock {
    pub fn phase(&self, n: u8) {
        self.script.lock().unwrap().phase = n;
    }

    pub fn cursors(&self) -> Vec<Option<String>> {
        self.script.lock().unwrap().cursors.clone()
    }
}

fn txn(id: &str, amount: f64, name: &str, extra: Value) -> Value {
    let mut t = json!({
        "transaction_id": id, "account_id": "acc1", "amount": amount, "date": "2026-09-20", "name": name,
        "merchant_name": null, "original_description": name, "pending": false, "pending_transaction_id": null,
    });
    for (k, v) in extra.as_object().unwrap() {
        t[k] = v.clone();
    }
    t
}

fn page(cursor: &str, added: Vec<Value>, modified: Vec<Value>, removed: Vec<Value>, more: bool) -> Value {
    json!({ "added": added, "modified": modified, "removed": removed, "next_cursor": cursor, "has_more": more })
}

fn answer(script: &mut Script, endpoint: &str, body: &Value) -> Result<Value, (&'static str, String)> {
    let account = |id: &str, name: &str, mask: &str, subtype: &str| json!({ "account_id": id, "name": name, "mask": mask, "type": "depository", "subtype": subtype, "balances": { "current": 923.5 } });
    match endpoint {
        "/accounts/get" => Ok(json!({
            "accounts": [account("acc1", "Plaid Checking", "0000", "checking"), account("acc2", "Plaid Savings", "1111", "savings")],
            "item": {},
        })),
        "/transactions/sync" => {
            let cursor = body["cursor"].as_str().map(str::to_string);
            script.cursors.push(cursor.clone());
            let t1 = txn("T1", 50.25, "COFFEE SHOP #12", json!({ "merchant_name": "Coffee Shop" }));
            let p1 = txn("P1", 20.0, "GAS STATION PENDING", json!({ "pending": true, "date": "2026-09-21" }));
            let t2 = txn("T2", 21.5, "GAS STATION", json!({ "pending_transaction_id": "P1", "date": "2026-09-22" }));
            let t3 = txn("T3", -1000.0, "PAYROLL", json!({ "date": "2026-09-23" }));
            let s1 = txn("S1", -10.0, "INTEREST", json!({ "account_id": "acc2" }));
            let t1m = txn("T1", 55.0, "COFFEE SHOP #12", json!({ "merchant_name": "Coffee Shop" }));
            let removed = json!({ "transaction_id": "P1", "account_id": "acc1" });
            match (script.phase, cursor.as_deref()) {
                (1 | 4, None) => Ok(page("p1a", vec![t1, s1], vec![], vec![], true)),
                (1, Some("p1a")) => Ok(page("c1", vec![p1, t3], vec![], vec![], false)),
                (4, Some("p1a")) => Err(("INTERNAL_SERVER_ERROR", "the second page failed".into())),
                (2, Some("c1")) => Ok(page("c2", vec![t2], vec![t1m], vec![removed], false)),
                (2 | 3, Some("c2")) => Ok(page("c2", vec![], vec![], vec![], false)),
                (p, c) => Err(("INVALID_FIELD", format!("unexpected cursor {c:?} in phase {p}"))),
            }
        }
        _ => Err(("NOT_FOUND", endpoint.into())),
    }
}

/// The scripted Plaid, on an ephemeral loopback port of `rt`'s.
pub fn plaid(rt: &tokio::runtime::Runtime) -> Mock {
    use axum::http::{HeaderMap, StatusCode, Uri};
    use axum::response::IntoResponse;
    let script = Arc::new(Mutex::new(Script {
        phase: 1,
        ..Script::default()
    }));
    let s = script.clone();
    let app = axum::Router::new().fallback(move |uri: Uri, headers: HeaderMap, body: String| {
        let s = s.clone();
        async move {
            let error = |code: &str, message: String| {
                let e = json!({ "error_type": "INVALID_INPUT", "error_code": code, "error_message": message });
                (StatusCode::BAD_REQUEST, axum::Json(e)).into_response()
            };
            let header = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            if header("plaid-client-id") != CLIENT_ID || header("plaid-secret") != SECRET {
                return error("INVALID_API_KEYS", "bad keys".into());
            }
            let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let mut script = s.lock().unwrap();
            script.bodies.push(body.to_string());
            if body["access_token"] != TOKEN {
                return error("INVALID_ACCESS_TOKEN", "bad token".into());
            }
            match answer(&mut script, uri.path(), &body) {
                Ok(v) => axum::Json(v).into_response(),
                Err((code, message)) => error(code, message),
            }
        }
    });
    let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    rt.spawn(async move { axum::serve(listener, app).await });
    Mock { url, script }
}

/// The importer's configuration for `mock`: its keys and the bank's token
/// in files under `dir`, the bank `chase`, and `accounts`.
pub fn banks(mock: &Mock, dir: &Path, accounts: &[(&str, &str, &str)]) -> Banks {
    let file = |name: &str, text: &str| {
        let p = dir.join(name);
        std::fs::write(&p, format!("{text}\n")).unwrap();
        p
    };
    Banks {
        url: mock.url.clone(),
        client_id_file: file("plaid-client-id", CLIENT_ID),
        secret_file: file("plaid-secret", SECRET),
        items: [("chase".to_string(), file("plaid-chase", TOKEN))].into(),
        accounts: accounts
            .iter()
            .map(|(name, item, mask)| {
                let m = Mapping {
                    item: item.to_string(),
                    mask: Some(mask.to_string()),
                    account_id: None,
                };
                (name.to_string(), m)
            })
            .collect(),
        history: 90,
    }
}

/// The importer's finished runs, once there are `n` of them.
pub fn runs(server: &Server, n: usize) -> Vec<money_server::importer::Run> {
    let i = server.importer.as_ref().expect("an importer");
    assert!(
        eventually(20, || i.runs().len() >= n),
        "{} runs, waiting for {n}; said {:#?}",
        i.runs().len(),
        i.said()
    );
    i.runs()
}
