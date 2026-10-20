//! A web build, served with the build as its validator (v4's `web.rs`, as
//! it was): a nix store path's files all carry mtime 1, so `Last-Modified`
//! would make a browser keep a client for years and answer `304` about a
//! new one. Instead every answer carries `ETag: "<build>"` and
//! `Cache-Control: no-cache` (ask every time), `Last-Modified` taken off
//! both ways; the build is the store hash of the directory, or off the
//! store the module's mtime and length when one is named, or every file's
//! latest mtime, total length and count. A miss falls back to
//! `index.html`, so a deep link reloads.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

/// `dir` at `/`, with the file whose change is a rebuild named or not.
pub fn router(dir: PathBuf, module: Option<String>) -> Router {
    let index = dir.join("index.html");
    Router::new()
        .fallback_service(ServeDir::new(&dir).fallback(ServeFile::new(index)))
        .layer(from_fn_with_state(Arc::new((dir, module)), validate))
}

/// Which build `dir` is, as a string that changes exactly when it does.
pub fn build_tag(dir: &Path, module: Option<&str>) -> String {
    let store = dir
        .strip_prefix("/nix/store")
        .ok()
        .and_then(|p| p.iter().next()?.to_str()?.split_once('-'));
    if let Some((hash, _)) = store.filter(|(h, _)| h.len() == 32 && h.bytes().all(|b| b.is_ascii_alphanumeric())) {
        return hash.to_string();
    }
    let stamp = |m: &std::fs::Metadata| {
        m.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos())
    };
    if let Some(module) = module {
        return std::fs::metadata(dir.join(module)).map_or("0".into(), |m| format!("{:x}-{:x}", stamp(&m), m.len()));
    }
    let (mut latest, mut total, mut count, mut todo) = (0u128, 0u64, 0u64, vec![dir.to_path_buf()]);
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                todo.push(e.path());
            } else {
                (latest, total, count) = (latest.max(stamp(&m)), total + m.len(), count + 1);
            }
        }
    }
    if count == 0 {
        return "0".into();
    }
    format!("{latest:x}-{total:x}-{count:x}")
}

async fn validate(State(cfg): State<Arc<(PathBuf, Option<String>)>>, mut req: Request, next: Next) -> Response {
    let tag = format!("\"{}\"", build_tag(&cfg.0, cfg.1.as_deref()));
    let tag = HeaderValue::from_str(&tag).unwrap_or(HeaderValue::from_static("\"0\""));
    let stamp = |mut res: Response| {
        let h = res.headers_mut();
        h.remove(header::LAST_MODIFIED);
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        h.insert(header::ETAG, tag.clone());
        res
    };
    let held = req.headers().get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok());
    if held.is_some_and(|v| v.split(',').any(|t| t.trim() == tag)) {
        return stamp(StatusCode::NOT_MODIFIED.into_response());
    }
    req.headers_mut().remove(header::IF_MODIFIED_SINCE);
    stamp(next.run(req).await)
}
