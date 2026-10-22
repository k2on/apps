//! Sign-in's routes (the `server` feature): `/auth/login`, its callback
//! and the code's exchange, as an axum `Router`.

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
/// Permissive CORS on all of them: the token is a bearer, never a cookie, so
/// another origin reading these answers learns only what it already sent.
pub fn router(auth: Arc<Auth>) -> Router {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/exchange", post(exchange))
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
            let Some(user) = q.user.filter(|u| !u.trim().is_empty()) else {
                return Html(dev_form(&q.redirect)).into_response();
            };
            // `name:role,role` is a login holding those roles
            // ([`dev_account`]), which the announcement says.
            let account = dev_account(user.trim());
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

#[derive(Debug, Deserialize)]
struct Exchange {
    code: String,
}

/// The code, for the login it stands for. Once.
async fn exchange(State(auth): State<Arc<Auth>>, Json(body): Json<Exchange>) -> Response {
    match auth.redeem(&body.code) {
        Some(login) => Json(login).into_response(),
        None => (StatusCode::BAD_REQUEST, "that code has been used, or has expired").into_response(),
    }
}

fn bad(why: String) -> Response {
    (StatusCode::BAD_REQUEST, why).into_response()
}

fn failed(why: String) -> Response {
    (StatusCode::BAD_GATEWAY, format!("sign-in failed: {why}")).into_response()
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
