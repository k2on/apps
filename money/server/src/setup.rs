//! The settings page: setting up where receipts come from, kept off the
//! budget. `/stores` is a page of the server's own, signed in to the way
//! the browser client is (to `/auth/login`, back with a code, the code
//! traded at `/auth/exchange`), whose answers are the server's and go
//! nowhere else: what it is told is kept in the vault ([`crate::vault`]),
//! never a row.
//!
//! - **Amazon**: the login `amazon-orders` signs in with (`PUT
//!   /stores/api/amazon`), or none (`DELETE`); the page is told only who,
//!   masked, and whether it has a one-time-code secret.
//!
//! Everything under `/stores/api` asks for a household member's token
//! (`Authorization: Bearer`), as the socket does. The page is served at
//! `/settings` and `/stores` alike.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Html;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::importer::ImporterHandle;
use crate::vault::{masked, AmazonLogin, Vault};

/// What the routes share.
#[derive(Clone)]
pub struct Setup {
    pub data: PathBuf,
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
            "last": said,
        }))
    }
}

fn outcome(o: &crate::importer::Outcome) -> String {
    use crate::importer::Outcome::*;
    match o {
        Receipts { receipts } => format!("{receipts} receipt{}", if *receipts == 1 { "" } else { "s" }),
        Charges { charges } => format!("{charges} charge{}", if *charges == 1 { "" } else { "s" }),
        Imported { upserts, removed } => format!("{upserts} changed, {removed} removed"),
        NotReady => "not ready".into(),
        Replaced => "replaced by another login".into(),
        Failed(why) => format!("failed: {why}"),
    }
}

/// The routes, under `/stores`.
pub fn routes(setup: Setup) -> Router {
    Router::new()
        .route("/settings", get(|| async { Html(PAGE) }))
        .route("/stores", get(|| async { Html(PAGE) }))
        .route("/stores/api/status", get(status))
        .route("/stores/api/amazon", put(set_amazon).delete(forget_amazon))
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

/// The page.
const PAGE: &str = include_str!("stores.html");
