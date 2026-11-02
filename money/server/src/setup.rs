//! The stores page: setting up where receipts come from, kept off the
//! budget. `/stores` is a page of the server's own, signed in to the way
//! the browser client is (to `/auth/login`, back with a code, the code
//! traded at `/auth/exchange`), whose answers are the server's and go
//! nowhere else: what it is told is kept in the vault ([`crate::vault`]),
//! never a row.
//!
//! - **Amazon**: the login `amazon-orders` signs in with (`PUT
//!   /stores/api/amazon`), or none (`DELETE`); the page is told only who,
//!   masked, and whether it has a one-time-code secret.
//! - **Costco**: an upload key (`POST /stores/api/keys/costco`, `DELETE` to
//!   revoke) and a bookmarklet that carries it. Clicked on a signed-in
//!   costco.com page, the bookmarklet asks Costco's own receipts query with
//!   that page's session — the query `download_costco_receipts.js`
//!   (ankurdave/beancount_import_sources) asks — and posts the answer to
//!   `/stores/upload/costco?key=…`. Costco's password never leaves Costco;
//!   the key uploads receipts and does nothing else. Where costco.com will
//!   not let it reach the server, it saves the file instead, for the page's
//!   upload.
//! - **An export by hand** (`POST /stores/api/upload/<store>`): a file the
//!   page sends, as the folder's would be.
//!
//! Everything under `/stores/api` asks for a household member's token
//! (`Authorization: Bearer`), as the socket does. An upload is read by its
//! store's adapter before it is kept — so a file that does not read is
//! said at once — then put in the receipts folder and the importer nudged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::importer::ImporterHandle;
use crate::vault::{masked, new_key, AmazonLogin, Vault};

/// The stores that can be set up here, and how.
const KEYED: [&str; 1] = ["costco"];

/// What the routes share.
#[derive(Clone)]
pub struct Setup {
    pub data: PathBuf,
    pub receipts: PathBuf,
    pub public_url: String,
    pub amazon_program: bool,
    pub authenticate: ark_auth::server::Authenticate,
    /// Filled once the importer has started.
    pub importer: Arc<Mutex<Option<ImporterHandle>>>,
    /// One writer of the vault at a time.
    pub lock: Arc<Mutex<()>>,
}

type Fail = (StatusCode, String);

fn fail(code: StatusCode, why: impl std::fmt::Display) -> Fail {
    (code, why.to_string())
}

impl Setup {
    async fn who(&self, headers: &HeaderMap) -> Result<String, Fail> {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_string)
            .ok_or_else(|| fail(StatusCode::UNAUTHORIZED, "sign in"))?;
        let auth = self.authenticate.clone();
        let who = tokio::task::spawn_blocking(move || auth(&token))
            .await
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e))?
            .ok_or_else(|| fail(StatusCode::UNAUTHORIZED, "sign in again"))?;
        if who.actor == crate::importer::ACCOUNT {
            return Err(fail(StatusCode::FORBIDDEN, "the importer sets up nothing"));
        }
        Ok(who.actor)
    }

    fn change(&self, f: impl FnOnce(&mut Vault) -> anyhow::Result<()>) -> Result<Vault, Fail> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut v = Vault::load(&self.data).map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
        f(&mut v).map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
        v.save(&self.data)
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
        Ok(v)
    }

    fn nudge(&self) {
        if let Some(i) = self.importer.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            i.nudge();
        }
    }

    /// An export read by its store's adapter, then kept for the importer.
    fn take(&self, store: &str, body: &str) -> Result<usize, Fail> {
        let read = money_domain::stores::read(store, body).map_err(|e| fail(StatusCode::UNPROCESSABLE_ENTITY, e))?;
        let dir = self.receipts.join(store);
        std::fs::create_dir_all(&dir).map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let name = format!("upload-{ms}.json");
        std::fs::write(dir.join(name), body).map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        self.nudge();
        Ok(read.receipts.len())
    }

    fn status(&self) -> Result<serde_json::Value, Fail> {
        let v = Vault::load(&self.data).map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
        let last = self.importer.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|i| i.last());
        let said: serde_json::Map<String, serde_json::Value> = last
            .map(|r| r.stores.into_iter().map(|(k, o)| (k, json!(outcome(&o)))).collect())
            .unwrap_or_default();
        Ok(json!({
            "amazon": {
                "program": self.amazon_program,
                "login": v.amazon.as_ref().map(|a| json!({"who": masked(&a.username), "otp": a.otp_secret.is_some()})),
            },
            "costco": {
                "key": v.keys.contains_key("costco"),
                "bookmarklet": v.keys.get("costco").map(|k| costco_bookmarklet(&self.public_url, k)),
            },
            "last": said,
        }))
    }
}

fn outcome(o: &crate::importer::Outcome) -> String {
    use crate::importer::Outcome::*;
    match o {
        Receipts { receipts } => format!("{receipts} receipt{}", if *receipts == 1 { "" } else { "s" }),
        Imported { upserts, removed } => format!("{upserts} changed, {removed} removed"),
        NotReady => "not ready".into(),
        Failed(why) => format!("failed: {why}"),
    }
}

/// The routes, under `/stores`.
pub fn routes(setup: Setup) -> Router {
    Router::new()
        .route("/stores", get(|| async { Html(PAGE) }))
        .route("/stores/api/status", get(status))
        .route("/stores/api/amazon", put(set_amazon).delete(forget_amazon))
        .route("/stores/api/keys/:store", post(make_key).delete(drop_key))
        .route("/stores/api/upload/:store", post(upload_signed_in))
        .route("/stores/upload/:store", post(upload_keyed).options(preflight))
        .with_state(setup)
}

async fn status(State(s): State<Setup>, headers: HeaderMap) -> Result<Json<serde_json::Value>, Fail> {
    s.who(&headers).await?;
    Ok(Json(s.status()?))
}

#[derive(Deserialize)]
struct AmazonForm {
    username: String,
    password: String,
    #[serde(default)]
    otp_secret: String,
}

async fn set_amazon(State(s): State<Setup>, headers: HeaderMap, Json(f): Json<AmazonForm>) -> Result<Json<serde_json::Value>, Fail> {
    s.who(&headers).await?;
    let (username, password) = (f.username.trim().to_string(), f.password.to_string());
    if username.is_empty() || password.is_empty() {
        return Err(fail(StatusCode::UNPROCESSABLE_ENTITY, "Amazon's email and password, both"));
    }
    let otp: String = f.otp_secret.chars().filter(|c| !c.is_whitespace()).collect();
    s.change(|v| {
        v.amazon = Some(AmazonLogin {
            username,
            password,
            otp_secret: (!otp.is_empty()).then_some(otp),
        });
        Ok(())
    })?;
    s.nudge();
    Ok(Json(s.status()?))
}

async fn forget_amazon(State(s): State<Setup>, headers: HeaderMap) -> Result<Json<serde_json::Value>, Fail> {
    s.who(&headers).await?;
    s.change(|v| {
        v.amazon = None;
        Ok(())
    })?;
    // The session amazon-orders kept goes with the login.
    let _ = std::fs::remove_dir_all(s.data.join("amazon-orders"));
    Ok(Json(s.status()?))
}

fn keyed(store: &str) -> Result<(), Fail> {
    match KEYED.contains(&store) {
        true => Ok(()),
        false => Err(fail(StatusCode::NOT_FOUND, format!("no bookmarklet for {store}"))),
    }
}

async fn make_key(State(s): State<Setup>, UrlPath(store): UrlPath<String>, headers: HeaderMap) -> Result<Json<serde_json::Value>, Fail> {
    s.who(&headers).await?;
    keyed(&store)?;
    let key = new_key().map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
    s.change(|v| {
        v.keys.insert(store.clone(), key);
        Ok(())
    })?;
    Ok(Json(s.status()?))
}

async fn drop_key(State(s): State<Setup>, UrlPath(store): UrlPath<String>, headers: HeaderMap) -> Result<Json<serde_json::Value>, Fail> {
    s.who(&headers).await?;
    s.change(|v| {
        v.keys.remove(&store);
        Ok(())
    })?;
    Ok(Json(s.status()?))
}

async fn upload_signed_in(
    State(s): State<Setup>,
    UrlPath(store): UrlPath<String>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<serde_json::Value>, Fail> {
    s.who(&headers).await?;
    let n = s.take(&store, &body)?;
    Ok(Json(json!({ "receipts": n })))
}

/// Anybody may ask from another site's page; only the key lets an upload in.
fn cors(mut r: Response) -> Response {
    let h = r.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("POST, OPTIONS"));
    h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type"));
    r
}

async fn preflight() -> Response {
    cors(StatusCode::NO_CONTENT.into_response())
}

async fn upload_keyed(State(s): State<Setup>, UrlPath(store): UrlPath<String>, Query(q): Query<HashMap<String, String>>, body: String) -> Response {
    let key = q.get("key").map(String::as_str).unwrap_or("");
    let allowed = Vault::load(&s.data).ok().is_some_and(|v| v.store_of_key(key) == Some(store.as_str()));
    let r = match allowed {
        false => fail(
            StatusCode::FORBIDDEN,
            "that key uploads nothing here: make a new bookmarklet on the stores page",
        )
        .into_response(),
        true => match s.take(&store, &body) {
            Ok(n) => format!("{n} receipt{} sent", if n == 1 { "" } else { "s" }).into_response(),
            Err(f) => f.into_response(),
        },
    };
    cors(r)
}

/// The bookmarklet for Costco, carrying where to send and the key.
pub fn costco_bookmarklet(server: &str, key: &str) -> String {
    let url = format!("{}/stores/upload/costco?key={key}", server.trim_end_matches('/'));
    let code = COSTCO.replace("__UPLOAD__", &url);
    format!("javascript:{}", code.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Costco's receipts for the last year, asked as costco.com's own page
/// asks them, sent to money; saved as a file where they cannot be sent.
const COSTCO: &str = r#"(async () => {
  const UPLOAD = "__UPLOAD__";
  const day = d => d.toLocaleDateString("en-US", {year: "numeric", month: "2-digit", day: "2-digit"});
  const end = new Date(), start = new Date(end.getTime() - 365 * 864e5);
  const query = "query receipts($startDate: String!, $endDate: String!) { receipts(startDate: $startDate, endDate: $endDate) { warehouseName documentType transactionDateTime transactionDate warehouseNumber warehouseShortName transactionType transactionBarcode total totalItemCount subTotal taxes itemArray { itemNumber itemDescription01 itemDescription02 itemIdentifier unit amount taxFlag } instantSavings } }";
  let receipts;
  try {
    const r = await fetch("https://ecom-api.costco.com/ebusiness/order/v1/orders/graphql", {
      method: "POST",
      headers: {
        "Content-Type": "application/json-patch+json",
        "Costco.Env": "ecom",
        "Costco.Service": "restOrders",
        "Costco-X-Wcs-Clientid": localStorage.getItem("clientID"),
        "Client-Identifier": "481b1aec-aa3b-454b-b81b-48187e28f205",
        "Costco-X-Authorization": "Bearer " + localStorage.getItem("idToken")
      },
      body: JSON.stringify({query, variables: {startDate: day(start), endDate: day(end)}})
    });
    receipts = (await r.json()).data.receipts;
  } catch (e) {
    alert("money: open costco.com's Orders & Purchases, signed in, and click this again. (" + e + ")");
    return;
  }
  const body = JSON.stringify(receipts);
  try {
    const u = await fetch(UPLOAD, {method: "POST", headers: {"Content-Type": "text/plain"}, body});
    alert("money: " + (await u.text()));
  } catch (e) {
    const a = document.createElement("a");
    a.download = "costco-receipts.json";
    a.href = URL.createObjectURL(new Blob([body], {type: "application/json"}));
    document.body.appendChild(a);
    a.click();
    alert("money: costco.com would not let this reach your server, so it saved costco-receipts.json. Upload it on money's stores page.");
  }
})();"#;

/// The page.
const PAGE: &str = include_str!("stores.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bookmarklet_carries_where_and_the_key() {
        let b = costco_bookmarklet("https://money.example.com/", "abc123");
        assert!(b.starts_with("javascript:(async () => {"));
        assert!(b.contains("\"https://money.example.com/stores/upload/costco?key=abc123\""));
        assert!(!b.contains('\n'));
    }
}
