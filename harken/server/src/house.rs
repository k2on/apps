//! The house's door: `POST /house/sign-in`, where a peer holding the
//! house's key (`HARKEN_HOUSE_KEY`'s file, which the server keeps) is handed
//! a login — the house's own, or a speaker's in an account's session.
//!
//! The house is a peer like any other (`harken-peer --house`); what it
//! needs that no person's device does is to stand in *every* account's
//! session, as a speaker of that account. So it is handed, per speaker, a
//! sign-in of that account whose session is `user@entity` — the speaker's
//! id in the session, which two accounts' kitchens do not share — written in
//! the log as any sign-in is and renewed when it has less than a day left.
//! The house's own login, `the house@house`, watches the sessions and
//! stands in none. A key is all it takes, so the key is a file only the
//! server and the house can read.

use std::sync::Arc;

use ark::format::Encode;
use ark::row::Row;
use ark::store::{Change, Read, Scan};
use ark_auth::server::Auth;
use ark_auth::SignIn;
use ark_peer::{now_ms, Handle};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

/// The account the house's own login is.
pub const HOUSE: &str = "the house";

/// What the door needs: the key, who signs in, and the hub the sign-in is
/// written to.
#[derive(Clone)]
pub struct Door {
    pub key: String,
    pub auth: Arc<Auth>,
    pub hub: Handle,
}

#[derive(Deserialize)]
struct Asked {
    user: Option<String>,
    speaker: Option<String>,
}

pub fn routes(door: Door) -> Router {
    Router::new()
        .route("/house/sign-in", post(sign_in))
        .with_state(door)
}

/// Whether two strings are equal, in time that does not say where they
/// first differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

async fn sign_in(
    State(door): State<Door>,
    headers: HeaderMap,
    Json(asked): Json<Asked>,
) -> Response {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if !bearer.is_some_and(|b| same(b, &door.key)) {
        return (StatusCode::UNAUTHORIZED, "not the house\n").into_response();
    }
    let (user, device) = match (asked.user, asked.speaker) {
        (Some(u), Some(s)) if !u.is_empty() && !s.is_empty() && !u.contains('@') => (u, s),
        (None, None) => (HOUSE.to_string(), "house".to_string()),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "a user and a speaker, or neither\n",
            )
                .into_response()
        }
    };
    match tokio::task::spawn_blocking(move || door.login(&user, &device)).await {
        Ok(Ok((token, session))) => {
            Json(serde_json::json!({ "token": token, "session": session })).into_response()
        }
        Ok(Err(why)) => (StatusCode::SERVICE_UNAVAILABLE, why).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

impl Door {
    /// `user`'s session `user@device`: its row live with a day left, or
    /// written (again) now; and its token.
    pub fn login(&self, user: &str, device: &str) -> Result<(String, String), String> {
        let id = format!("{user}@{device}");
        let (now, day) = (now_ms(), 24 * 60 * 60 * 1000);
        let want = id.clone();
        let held = self
            .hub
            .with(move |h| {
                h.log
                    .store
                    .scan(SignIn::TABLE, &Scan::all())
                    .iter()
                    .filter_map(|r| SignIn::from_bytes(r).ok())
                    .find(|s| s.id == want)
            })
            .ok_or("the hub is gone")?;
        if held
            .as_ref()
            .is_some_and(|s| !s.revoked && s.expires_ms > now + day)
        {
            return Ok((self.auth.mint(&id), id));
        }
        let row = SignIn {
            id: id.clone(),
            user: user.into(),
            name: String::new(),
            email: String::new(),
            device: format!("{HOUSE}: {device}"),
            issued_ms: now,
            expires_ms: now + ark_auth::server::DEFAULT_TTL_MS,
            revoked: false,
            roles: vec![],
        };
        let change = match held {
            Some(old) => Change::Edit {
                table: SignIn::TABLE.into(),
                old: old.to_bytes(),
                new: row.to_bytes(),
            },
            None => Change::Add {
                table: SignIn::TABLE.into(),
                row: row.to_bytes(),
            },
        };
        self.hub
            .write(vec![change])
            .ok_or("the hub is gone")?
            .map_err(|e| format!("could not record the house's session: {e}"))?;
        Ok((self.auth.mint(&id), id))
    }
}
