//! Sign-in's routes (the `server` feature): `/auth/login`, its callback,
//! the code's exchange and a person's picture, as an axum `Router`.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tower_http::cors::{Any, CorsLayer};

use super::*;
use crate::util::with_code;

/// The routes, with state applied, ready to `merge` into an app's router.
///
/// Permissive CORS on all of them: a native client's token is a bearer,
/// and a page's cookie (`/auth/session`) is `SameSite=Strict` and set only
/// on its own origin's answer — CORS that allows any origin allows no
/// credentials — so another origin reading these answers learns only what
/// it already sent.
pub fn router(auth: Arc<Auth>) -> Router {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/exchange", post(exchange))
        .route("/auth/session", post(session))
        .route("/auth/logout", post(logout))
        .route("/auth/device-code", post(device_code))
        .route("/auth/redeem", post(redeem))
        .route("/auth/picture", get(picture))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .with_state(auth)
}

#[derive(Debug, Deserialize)]
struct LoginQuery {
    redirect: String,
    user: Option<String>,
}

/// What is signing in, as its browser or program says.
fn device(headers: &HeaderMap) -> String {
    let agent = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok());
    agent.unwrap_or_default().chars().take(200).collect()
}

/// `account` signed in on `device` off the runtime (the hub is asked), and
/// sent back to `redirect` with the code.
async fn finished(auth: Arc<Auth>, account: Account, device: String, redirect: String) -> Response {
    match tokio::task::spawn_blocking(move || auth.finish(&account, &device)).await {
        Ok(Ok(code)) => Redirect::to(&with_code(&redirect, &code)).into_response(),
        Ok(Err(e)) => failed(e),
        Err(e) => failed(e.to_string()),
    }
}

/// Start a login, or in dev mode finish one.
async fn login(State(auth): State<Arc<Auth>>, headers: HeaderMap, Query(q): Query<LoginQuery>) -> Response {
    if !auth.redirect_allowed(&q.redirect) {
        return bad(format!(
            "a login code cannot be sent to {}; the server allows loopback, its own \
             origin, and what it was configured with",
            q.redirect
        ));
    }
    match &auth.mode {
        Mode::Dev => {
            // The name asked for, or a dev server's own (`ARK_DEV_USER`): who
            // it signs in, with no page between, when a login names nobody —
            // what the app screenshots run their servers with, so a phone's
            // sign-in sheet opens and comes straight back
            // (.github/screens/apps.md). Only in dev mode, where anyone is
            // whoever they say already.
            let user = q.user.filter(|u| !u.trim().is_empty()).or_else(dev_user);
            let Some(user) = user else {
                return Html(dev_form(&q.redirect)).into_response();
            };
            // `name:role,role` is a login holding those roles
            // ([`dev_account`]), which the announcement says.
            let mut account = dev_account(user.trim());
            // The dev server's own user wears the picture it was given
            // (`ARK_DEV_PICTURE`, a file on this machine): the demo's and
            // the screenshots' person has a face, not only initials.
            if dev_user().is_some_and(|d| dev_account(d.trim()).id == account.id) {
                if let Some(p) = dev_picture() {
                    account.picture = p;
                }
            }
            finished(auth.clone(), account, device(&headers), q.redirect).await
        }
        Mode::Oidc(provider) => {
            let challenge = Challenge::new();
            let url = match provider.authorize_url(&challenge, &callback_url(&auth)) {
                Ok(url) => url,
                Err(e) => return failed(e),
            };
            let now = now_ms();
            let mut pending = auth.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.retain(|_, p| now - p.started_ms < PENDING_TTL_MS);
            pending.insert(
                challenge.state.clone(),
                Pending {
                    challenge,
                    redirect: q.redirect,
                    device: device(&headers),
                    started_ms: now,
                },
            );
            Redirect::to(&url).into_response()
        }
    }
}

fn callback_url(auth: &Auth) -> String {
    format!("{}/auth/callback", auth.public_url)
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// The provider sending the person back.
async fn callback(State(auth): State<Arc<Auth>>, Query(q): Query<CallbackQuery>) -> Response {
    let Mode::Oidc(provider) = &auth.mode else {
        return bad("this server does not use a provider".into());
    };
    if let Some(error) = q.error {
        return bad(format!("the provider refused: {error} {}", q.error_description.unwrap_or_default()));
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return bad("the provider sent no code".into());
    };
    let Some(pending) = auth.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&state) else {
        return bad("this login was not started here, or took too long".into());
    };
    if now_ms() - pending.started_ms >= PENDING_TTL_MS {
        return bad("this login took too long; start again".into());
    }

    // Two round trips to the provider, off the runtime.
    let (provider, callback, challenge) = (provider.clone(), callback_url(&auth), pending.challenge);
    let exchanged = tokio::task::spawn_blocking(move || provider.exchange(&code, &challenge, &callback)).await;
    match exchanged {
        Ok(Ok(account)) => finished(auth.clone(), account, pending.device, pending.redirect).await,
        Ok(Err(e)) => failed(e),
        Err(e) => failed(e.to_string()),
    }
}

/// Another device's code, at `/auth/redeem`.
#[derive(Debug, Deserialize)]
struct Redeem {
    code: String,
}

/// The proof (the code), for the login it stands for. Once.
async fn exchange(State(auth): State<Arc<Auth>>, Json(proof): Json<Proof>) -> Response {
    match auth.prove(&proof) {
        Some(login) => Json(login).into_response(),
        None => (StatusCode::BAD_REQUEST, "that code has been used, or has expired").into_response(),
    }
}

/// The code, for a browser's session: the token kept in the session
/// cookie ([`Auth::session_cookie`]), where no script reads it, and the
/// login answered without it. Once, as `/auth/exchange`.
async fn session(State(auth): State<Arc<Auth>>, Json(proof): Json<Proof>) -> Response {
    match auth.prove(&proof) {
        Some(login) => {
            let cookie = auth.session_cookie(&login);
            let login = Login {
                token: String::new(),
                ..login
            };
            ([(header::SET_COOKIE, cookie)], Json(login)).into_response()
        }
        None => (StatusCode::BAD_REQUEST, "that code has been used, or has expired").into_response(),
    }
}

/// A browser's session cookie forgotten. Its row is the session's own to
/// end, over the socket (`session.revoke`), before it is asked this.
async fn logout(State(auth): State<Arc<Auth>>) -> Response {
    ([(header::SET_COOKIE, auth.cleared_cookie())], StatusCode::NO_CONTENT).into_response()
}

/// A one-time code for another device, made by a signed-in one: its
/// session's token as the bearer.
async fn device_code(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Response {
    let bearer = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    let Some(token) = bearer.and_then(|b| b.strip_prefix("Bearer ")).map(str::to_string) else {
        return (StatusCode::UNAUTHORIZED, "sign in first").into_response();
    };
    match tokio::task::spawn_blocking(move || auth.device_code(&token)).await {
        Ok(Ok((code, expires_ms))) => Json(serde_json::json!({ "code": code, "expires_ms": expires_ms })).into_response(),
        Ok(Err(e)) => (StatusCode::FORBIDDEN, e).into_response(),
        Err(e) => failed(e.to_string()),
    }
}

/// A device code, for a login of the new device's own. Once.
async fn redeem(State(auth): State<Arc<Auth>>, headers: HeaderMap, Json(body): Json<Redeem>) -> Response {
    let device = device(&headers);
    match tokio::task::spawn_blocking(move || auth.redeem_device_code(&body.code, &device)).await {
        Ok(Ok(login)) => Json(login).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, e).into_response(),
        Err(e) => failed(e.to_string()),
    }
}

#[derive(Debug, Deserialize)]
struct PictureQuery {
    user: String,
}

/// A person's picture, fetched from their provider ([`Auth::picture`]):
/// what a page cannot load from a provider that does not answer it with
/// CORS. Asked by anybody, as `/media` is — a picture is what a person
/// shows to whoever they are signed in beside — and kept a day by whoever
/// asked: the address has a version in it (`crate::picture_url`).
async fn picture(State(auth): State<Arc<Auth>>, Query(q): Query<PictureQuery>) -> Response {
    match tokio::task::spawn_blocking(move || auth.picture(&q.user)).await {
        Ok(Ok(Some(p))) => (
            [
                (header::CONTENT_TYPE, p.kind),
                (header::CACHE_CONTROL, "public, max-age=86400".into()),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
            ],
            p.bytes,
        )
            .into_response(),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "no picture").into_response(),
        Ok(Err(e)) => (StatusCode::BAD_GATEWAY, e).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

fn bad(why: String) -> Response {
    (StatusCode::BAD_REQUEST, why).into_response()
}

fn failed(why: String) -> Response {
    (StatusCode::BAD_GATEWAY, format!("sign-in failed: {why}")).into_response()
}

/// A dev server's own name for a login that names nobody (`ARK_DEV_USER`).
fn dev_user() -> Option<String> {
    std::env::var("ARK_DEV_USER").ok().filter(|u| !u.trim().is_empty())
}

/// The dev server's own user's picture (`ARK_DEV_PICTURE`): an image file
/// on this machine, as the `file://` address [`Auth::picture`] reads in dev
/// mode alone.
fn dev_picture() -> Option<String> {
    let path = std::env::var("ARK_DEV_PICTURE").ok().filter(|p| !p.trim().is_empty())?;
    let path = std::fs::canonicalize(path.trim()).ok()?;
    Some(format!("file://{}", path.display()))
}

/// Dev mode's provider: a text box.
fn dev_form(redirect: &str) -> String {
    let redirect = redirect.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;");
    format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\">\
         <title>sign in</title>\
         <style>body{{font-family:system-ui;max-width:24em;margin:4em auto;padding:0 1em}}\
         input,button{{font:inherit;padding:.5em;width:100%;box-sizing:border-box;margin:.25em 0}}\
         p{{color:#666}}</style>\
         <h1>sign in</h1>\
         <p>This server is running without an identity provider, so it takes your word \
         for who you are. That is fine on a laptop and nowhere else.</p>\
         <form method=get action=/auth/login>\
         <input type=hidden name=redirect value=\"{redirect}\">\
         <input name=user placeholder=\"a name\" autofocus autocapitalize=off>\
         <button>sign in</button></form>"
    )
}
