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
use money_server::importer::Banks;
use money_server::store::Store;
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

/// The client id and the secret the mock takes, and the tokens it knows:
/// chase's, which the phases script, and the one a connection made through
/// Link is given — a Wells Fargo of a checking account and a card.
pub const CLIENT_ID: &str = "client-id-0f3a";
pub const SECRET: &str = "secret-77c2e1";
pub const TOKEN: &str = "access-test-5d9b";
pub const WF_TOKEN: &str = "access-wf-7a1c";
/// A second login to the same Wells Fargo, when a test has the next page
/// give it ([`Script::exchange`]): its accounts the first login's, and a
/// savings account the first did not share.
pub const WF2_TOKEN: &str = "access-wf2-3e8b";

/// A Hosted Link page the mock opened: whether for a login given again,
/// and how it ended once somebody finished it.
#[derive(Clone, Debug, Default)]
pub struct Page {
    pub update: bool,
    pub finished: Option<Value>,
}

/// What the scripted Plaid answers, and what it was asked.
#[derive(Default)]
pub struct Script {
    /// Which of the three phases chase's `/transactions/sync` is in; a
    /// fourth fails its second page.
    pub phase: u8,
    /// Every `/transactions/sync` asked: its cursor (none from the start).
    pub cursors: Vec<Option<String>>,
    /// Every request's body, whole: what the importer sent Plaid.
    pub bodies: Vec<(String, Value)>,
    /// The pages opened, by link token.
    pub pages: Vec<Page>,
    /// Tokens Plaid wants a fresh login for, and tokens it has forgotten.
    pub login_required: Vec<String>,
    pub removed: Vec<String>,
    /// Wells Fargo has a new card transaction since `w1`.
    pub wf_more: bool,
    /// The access token the next public token is traded for: the first
    /// Wells Fargo login's unless a test says.
    pub exchange: Option<String>,
    /// The first Wells Fargo login shares its savings account too, as it
    /// does once it is picked in Plaid's account picker.
    pub wf_savings: bool,
}

/// ArkOS's mock Plaid (`sync/actual-import/src/test/mock-plaid.ts`), whose
/// answers `money/domain/tests/import.rs` walks the domain through: one
/// bank of two accounts, its initial pull over two pages with a pending
/// transaction and a payroll; that transaction posting with another amount
/// while the coffee is modified; and nothing new. Beside it, Plaid's
/// Hosted Link: pages opened, finished when a test says, and a Wells
/// Fargo behind them.
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

    /// The bodies sent to one endpoint.
    pub fn asked(&self, endpoint: &str) -> Vec<Value> {
        let s = self.script.lock().unwrap();
        s.bodies.iter().filter(|(e, _)| e == endpoint).map(|(_, b)| b.clone()).collect()
    }

    /// The last page opened, finished as Plaid says a person finished it:
    /// a bank connected (a public token), or the login given again.
    pub fn finish(&self) {
        let mut s = self.script.lock().unwrap();
        let n = s.pages.len();
        let page = s.pages.last_mut().expect("a page was opened");
        page.finished = Some(match page.update {
            false => json!({ "finished_at": "2026-10-26T10:00:00Z",
                              "results": { "item_add_results": [{ "public_token": format!("public-{n}") }] } }),
            true => json!({ "finished_at": "2026-10-26T10:00:00Z", "on_success": { "public_token": null } }),
        });
        if page.update {
            s.login_required.clear();
        }
    }

    /// The last page closed by somebody who gave up.
    pub fn give_up(&self) {
        let mut s = self.script.lock().unwrap();
        let page = s.pages.last_mut().expect("a page was opened");
        page.finished = Some(json!({ "finished_at": "2026-10-26T10:00:00Z",
            "on_exit": { "error": { "error_code": "INSTITUTION_DOWN", "error_type": "INSTITUTION_ERROR",
                                    "display_message": "Wells Fargo is not answering" } } }));
    }

    pub fn login_required(&self, token: &str) {
        self.script.lock().unwrap().login_required.push(token.into());
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

type Answered = Result<Value, (&'static str, String)>;

fn chase(script: &mut Script, endpoint: &str, body: &Value) -> Answered {
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
                // From the start, in any phase: the first pull's two pages.
                (_, None) => Ok(page("p1a", vec![t1, s1], vec![], vec![], true)),
                (1..=3, Some("p1a")) => Ok(page("c1", vec![p1, t3], vec![], vec![], false)),
                (4, Some("p1a")) => Err(("INTERNAL_SERVER_ERROR", "the second page failed".into())),
                (2, Some("c1")) => Ok(page("c2", vec![t2], vec![t1m], vec![removed], false)),
                (2 | 3, Some("c2")) => Ok(page("c2", vec![], vec![], vec![], false)),
                (p, c) => Err(("INVALID_FIELD", format!("unexpected cursor {c:?} in phase {p}"))),
            }
        }
        _ => Err(("NOT_FOUND", endpoint.into())),
    }
}

/// Wells Fargo: checking, a card, and history on both — and a savings
/// account, for the second login (`second`) or once the first shares it.
/// The second login's ids are its own: `wf2-…`, `W2…`.
fn wells(script: &mut Script, endpoint: &str, body: &Value, second: bool) -> Answered {
    let (a, t_, item) = match second {
        false => ("wf", "W", "item-wf"),
        true => ("wf2", "W2", "item-wf2"),
    };
    let savings = second || script.wf_savings;
    match endpoint {
        "/accounts/get" => {
            let mut accounts = vec![
                json!({ "account_id": format!("{a}-chk"), "name": "Everyday Checking", "mask": "4321", "type": "depository", "subtype": "checking", "balances": { "current": 1000.0 } }),
                json!({ "account_id": format!("{a}-card"), "name": "Active Cash Card", "mask": "8765", "type": "credit", "subtype": "credit card", "balances": { "current": 300.0 } }),
            ];
            if savings {
                accounts.push(json!({ "account_id": format!("{a}-sav"), "name": "Way Savings", "mask": "5555", "type": "depository", "subtype": "savings", "balances": { "current": 700.0 } }));
            }
            Ok(json!({ "accounts": accounts, "item": { "item_id": item, "institution_id": "ins_127991" } }))
        }
        "/transactions/sync" => {
            let cursor = body["cursor"].as_str().map(str::to_string);
            script.cursors.push(cursor.clone());
            let t = |id: &str, account: &str, amount: f64, date: &str| {
                let id = format!("{t_}{id}");
                txn(
                    &id,
                    amount,
                    &format!("{id} STORE"),
                    json!({ "account_id": format!("{a}-{account}"), "date": date }),
                )
            };
            match cursor.as_deref() {
                None => {
                    let mut added = vec![t("C1", "card", 200.0, "2026-09-12"), t("K1", "chk", 40.0, "2026-09-14")];
                    if savings {
                        added.push(t("S1", "sav", -500.0, "2026-09-20"));
                    }
                    Ok(page("w1", added, vec![], vec![], false))
                }
                Some("w1") if script.wf_more => Ok(page("w2", vec![t("C2", "card", 25.0, "2026-10-20")], vec![], vec![], false)),
                Some("w1" | "w2") => Ok(page(cursor.as_deref().unwrap(), vec![], vec![], vec![], false)),
                Some(c) => Err(("INVALID_FIELD", format!("unexpected cursor {c}"))),
            }
        }
        _ => Err(("NOT_FOUND", endpoint.into())),
    }
}

fn answer(script: &mut Script, endpoint: &str, body: &Value) -> Answered {
    match endpoint {
        "/institutions/get_by_id" => match body["institution_id"].as_str() {
            Some("ins_127991") => Ok(json!({ "institution": { "institution_id": "ins_127991", "name": "Wells Fargo" } })),
            _ => Err(("INVALID_INSTITUTION", "no such institution".into())),
        },
        "/link/token/create" => {
            if body["hosted_link"].is_null() {
                return Err(("INVALID_FIELD", "not a Hosted Link page".into()));
            }
            let update = body["access_token"].is_string();
            if update == body["products"].is_array() {
                return Err(("INVALID_FIELD", "products, or an access token, not both".into()));
            }
            script.pages.push(Page { update, finished: None });
            let n = script.pages.len();
            Ok(
                json!({ "link_token": format!("link-{n}"), "hosted_link_url": format!("https://hosted.plaid.test/link-{n}"), "expiration": "2026-10-26T11:00:00Z" }),
            )
        }
        "/link/token/get" => {
            let n: usize = body["link_token"]
                .as_str()
                .and_then(|t| t.strip_prefix("link-"))
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            let Some(page) = n.checked_sub(1).and_then(|i| script.pages.get(i)) else {
                return Err(("INVALID_LINK_TOKEN", "no such link token".into()));
            };
            Ok(json!({ "link_token": body["link_token"], "link_sessions": page.finished.clone().into_iter().collect::<Vec<_>>() }))
        }
        "/item/public_token/exchange" => match body["public_token"].as_str() {
            Some(p) if p.starts_with("public-") => match script.exchange.clone() {
                Some(t) if t == WF2_TOKEN => Ok(json!({ "access_token": t, "item_id": "item-wf2" })),
                _ => Ok(json!({ "access_token": WF_TOKEN, "item_id": "item-wf" })),
            },
            _ => Err(("INVALID_PUBLIC_TOKEN", "no such public token".into())),
        },
        _ => {
            let token = body["access_token"].as_str().unwrap_or("").to_string();
            if script.removed.contains(&token) || ![TOKEN, WF_TOKEN, WF2_TOKEN].contains(&token.as_str()) {
                return Err(("INVALID_ACCESS_TOKEN", "bad token".into()));
            }
            if endpoint == "/item/remove" {
                script.removed.push(token);
                return Ok(json!({ "request_id": "r" }));
            }
            if script.login_required.contains(&token) && endpoint == "/accounts/get" {
                return Err(("ITEM_LOGIN_REQUIRED", "the login details of this item have changed".into()));
            }
            match token == TOKEN {
                true => chase(script, endpoint, body),
                false => wells(script, endpoint, body, token == WF2_TOKEN),
            }
        }
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
            script.bodies.push((uri.path().to_string(), body.clone()));
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

/// The importer's configuration for `mock`: its keys in files under `dir`,
/// and a page asked after every 50ms.
pub fn banks(mock: &Mock, dir: &Path) -> Banks {
    let file = |name: &str, text: &str| {
        let p = dir.join(name);
        std::fs::write(&p, format!("{text}\n")).unwrap();
        p
    };
    Banks {
        poll: Duration::from_millis(50),
        ..Banks::new(&mock.url, file("plaid-client-id", CLIENT_ID), file("plaid-secret", SECRET))
    }
}

/// `banks.json` in `data`, as somebody would write it by hand: each bank
/// by name, its token, and its cursor when it has one.
pub fn connect(data: &Path, banks: &[(&str, &str, Option<&str>)]) {
    std::fs::create_dir_all(data).unwrap();
    let mut all = serde_json::Map::new();
    for (name, token, cursor) in banks {
        let mut c = json!({ "access_token": token });
        if let Some(cursor) = cursor {
            c["cursor"] = json!(cursor);
        }
        all.insert(name.to_string(), c);
    }
    std::fs::write(data.join("banks.json"), json!({ "banks": all }).to_string()).unwrap();
}

/// The store as the server keeps it.
pub fn store(data: &Path) -> Store {
    Store::load(data).unwrap()
}

/// A server importing from `mock`, its keys under `secrets`.
pub fn importing(rt: &tokio::runtime::Runtime, data: &Path, mock: &Mock, secrets: &Path) -> Server {
    let b = banks(mock, secrets);
    serve(rt, data, |c| c.banks = Some(b))
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

/// `user`, signed in on a device of their own and caught up with the hub.
pub fn caught_up(server: &Server, user: &str) -> Peer {
    let mut p = signed_in(server, user);
    pump_until(&mut [&mut p], 10, |ps| ps[0].cursor() == head(server));
    p
}

/// Whether `needle` is anywhere in a store's rows, any table's.
pub fn in_rows(store: &ark::store::Store, needle: &str) -> bool {
    let n = needle.as_bytes();
    let tables: Vec<String> = money_domain::shared().schema.tables.iter().map(|t| t.name.clone()).collect();
    tables.iter().any(|t| store.rows(t).any(|r| r.windows(n.len()).any(|w| w == n)))
}

/// Whether `needle` is anywhere in what the hub holds.
pub fn leaked(server: &Server, needle: &'static str) -> bool {
    server.running.hub.with(move |h| in_rows(&h.log.store, needle)).unwrap()
}

/// A directory copied whole, as a backup restored is.
pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        match e.file_type().unwrap().is_dir() {
            true => copy_dir(&e.path(), &target),
            false => drop(std::fs::copy(e.path(), target).unwrap()),
        }
    }
}
