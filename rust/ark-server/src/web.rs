//! A browser client, served with a validator the nix store cannot give it.
//!
//! A web build is usually a nix store path, and nix sets every file in one
//! to mtime 1. `ServeDir` answers a plain GET with `Last-Modified: Thu, 01
//! Jan 1970 00:00:01 GMT` and no `Cache-Control`, and from that a browser
//! draws two conclusions that are each correct and together permanent:
//!
//! - **It is fresh for years.** With no `Cache-Control`, freshness is a
//!   heuristic — a tenth of the time since `Last-Modified`, about five and a
//!   half years for 1970. A reload revalidates the page it is on; the
//!   module and the wasm are subresources, taken from the cache unasked.
//! - **And when it does ask, the answer is always no.** `If-Modified-Since:
//!   …1970…` against a new file of the same mtime is `304 Not Modified` —
//!   about a build that replaced every byte.
//!
//! So a browser that has ever loaded a client keeps it, whatever is
//! deployed behind it (harken lost its device picker to exactly this: old
//! tabs went on dialling a socket the server no longer had). Instead, for
//! every file under the directory:
//!
//! - **The validator is the build, not the file.** `ETag` is the store hash
//!   of the directory — what changes with every rebuild and never without
//!   one. Off the store it is the module's mtime and length when the app
//!   names its module ([`router_with_module`]), or every file's latest
//!   mtime, total length and count when it does not.
//! - **`no-cache` is not "do not cache".** It is "ask every time": one
//!   conditional GET per file answered `304` with no body.
//! - **`Last-Modified` is taken off, both ways.** Off the response so a
//!   browser has nothing to be heuristic about, and `If-Modified-Since` off
//!   the request so a browser still holding the 1970 date gets `200`.
//!
//! The other half is the page's: name the module and the wasm with
//! `?v=<build>` filled in at build time, so a heuristically fresh cache —
//! never asked about at all — finds URLs it has never seen.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

/// Serve `dir` at `/`, every miss falling back to its `index.html` — which is
/// what makes a reload of a deep link work in a single-page client — with the
/// build as the validator on every answer.
pub fn router(dir: PathBuf) -> Router {
    router_with_module(dir, None)
}

/// [`router`], with the file whose change *is* a rebuild off the store
/// named — `pkg/myapp_bg.wasm` — so the tag is its mtime and length alone.
pub fn router_with_module(dir: PathBuf, module: Option<String>) -> Router {
    let index = dir.join("index.html");
    Router::new()
        .fallback_service(ServeDir::new(&dir).fallback(ServeFile::new(index)))
        .layer(from_fn_with_state(Arc::new((dir, module)), validate))
}

/// Which build this directory is, as an opaque string that changes exactly
/// when the build does.
///
/// A store path carries its own answer in its name: the hash is over every
/// input. Anywhere else, the module's mtime and length when one is named,
/// or every file's — on a laptop the files are what a rebuild rewrites.
pub fn build_tag(dir: &Path, module: Option<&str>) -> String {
    if let Some(hash) = store_hash(dir) {
        return hash.to_string();
    }
    let stamp = |meta: &std::fs::Metadata| {
        meta.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos())
    };
    if let Some(module) = module {
        return match std::fs::metadata(dir.join(module)) {
            Ok(meta) => format!("{:x}-{:x}", stamp(&meta), meta.len()),
            // No module: nothing to validate against, so every request is a
            // miss, which is right for a directory with nothing to be stale
            // about.
            Err(_) => String::from("0"),
        };
    }
    let (mut latest, mut total, mut count) = (0u128, 0u64, 0u64);
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                todo.push(e.path());
            } else {
                latest = latest.max(stamp(&meta));
                total += meta.len();
                count += 1;
            }
        }
    }
    if count == 0 {
        return String::from("0");
    }
    format!("{latest:x}-{total:x}-{count:x}")
}

/// The 32-character hash at the front of a store path's name, if `dir` is one.
fn store_hash(dir: &Path) -> Option<&str> {
    let name = dir.strip_prefix("/nix/store").ok()?.iter().next()?.to_str()?;
    let (hash, _) = name.split_once('-')?;
    (hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_alphanumeric())).then_some(hash)
}

/// The build as an entity tag, quoted the way the header wants it.
fn etag(dir: &Path, module: Option<&str>) -> HeaderValue {
    HeaderValue::from_str(&format!("\"{}\"", build_tag(dir, module))).unwrap_or_else(|_| HeaderValue::from_static("\"0\""))
}

/// One request: answer it from the tag if the browser already has this build,
/// and stamp the tag on whatever `ServeDir` says otherwise.
async fn validate(State(cfg): State<Arc<(PathBuf, Option<String>)>>, mut req: Request, next: Next) -> Response {
    let tag = etag(&cfg.0, cfg.1.as_deref());
    let stamp = |mut res: Response| {
        let headers = res.headers_mut();
        headers.remove(header::LAST_MODIFIED);
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        headers.insert(header::ETAG, tag.clone());
        res
    };
    // `If-None-Match` may carry several tags, and `*`. Only an exact match on
    // ours is this build; anything else is a browser holding something else.
    let held = req
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == tag));
    if held {
        return stamp(StatusCode::NOT_MODIFIED.into_response());
    }
    // A date is not a validator here — see the module docs — so `ServeDir`
    // never sees one and cannot answer `304` about a file it has not compared.
    req.headers_mut().remove(header::IF_MODIFIED_SINCE);
    stamp(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULE: &str = "pkg/app_bg.wasm";

    #[test]
    fn a_store_path_is_its_hash() {
        let dir = Path::new("/nix/store/6ilgmw07dbmi7y4klpgc9v5k1nmfbs66-app-web-0.1.0");
        assert_eq!(build_tag(dir, None), "6ilgmw07dbmi7y4klpgc9v5k1nmfbs66");
        assert_eq!(build_tag(dir, Some(MODULE)), "6ilgmw07dbmi7y4klpgc9v5k1nmfbs66");
        assert!(store_hash(Path::new("/nix/store/not-a-hash")).is_none());
        assert!(store_hash(Path::new("/srv/6ilgmw07dbmi7y4klpgc9v5k1nmfbs66-x")).is_none());
    }

    #[test]
    fn off_the_store_the_files_are_the_build() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        std::fs::create_dir_all(dir.join("pkg")).unwrap();
        assert_eq!(build_tag(dir, Some(MODULE)), "0", "no module, no build");
        assert_eq!(build_tag(dir, None), "0", "no files, no build");
        std::fs::write(dir.join(MODULE), b"one build").unwrap();
        let (named, whole) = (build_tag(dir, Some(MODULE)), build_tag(dir, None));
        assert_ne!(named, "0");
        assert_ne!(whole, "0");
        // A rebuild writes a different module. Length changes here so the
        // test does not depend on the filesystem's mtime resolution.
        std::fs::write(dir.join(MODULE), b"another build entirely").unwrap();
        assert_ne!(build_tag(dir, Some(MODULE)), named);
        assert_ne!(build_tag(dir, None), whole);
        // Unnamed, any file is part of the build; named, only the module.
        let (named, whole) = (build_tag(dir, Some(MODULE)), build_tag(dir, None));
        std::fs::write(dir.join("index.html"), b"a page").unwrap();
        assert_eq!(build_tag(dir, Some(MODULE)), named);
        assert_ne!(build_tag(dir, None), whole);
    }
}
