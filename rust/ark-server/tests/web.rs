//! The browser client's cache validator, over a real socket — because the
//! bug it fixes is one only a real HTTP exchange has: a `304` answered to a
//! date every file in the nix store shares.

use std::net::SocketAddr;

const MODULE: &str = "pkg/app_bg.wasm";
use std::path::PathBuf;

/// A build on disk: a page and a module, off the store.
fn build(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ark-web-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("pkg")).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html>the page").unwrap();
    std::fs::write(dir.join(MODULE), b"\0asm the first build").unwrap();
    dir
}

async fn serve(dir: PathBuf) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, ark_server::web::router_with_module(dir, Some(MODULE.into())))
            .await
            .unwrap();
    });
    addr
}

/// One GET, with whatever conditional headers a browser would send. Answers
/// the status and the response headers that matter here.
async fn get(addr: SocketAddr, path: &str, headers: &[(&str, &str)]) -> Answer {
    let url = format!("http://{addr}{path}");
    let headers: Vec<(String, String)> = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    tokio::task::spawn_blocking(move || {
        let mut req = ureq::get(&url);
        for (k, v) in &headers {
            req = req.set(k, v);
        }
        let res = match req.call() {
            Ok(res) => res,
            Err(ureq::Error::Status(_, res)) => res,
            Err(e) => panic!("{e}"),
        };
        Answer {
            status: res.status(),
            etag: res.header("etag").map(str::to_string),
            cache_control: res.header("cache-control").map(str::to_string),
            last_modified: res.header("last-modified").map(str::to_string),
            body: res.into_string().unwrap_or_default(),
        }
    })
    .await
    .unwrap()
}

struct Answer {
    status: u16,
    etag: Option<String>,
    cache_control: Option<String>,
    last_modified: Option<String>,
    body: String,
}

#[tokio::test]
async fn every_file_carries_the_build_and_no_date() {
    let dir = build("stamp");
    let addr = serve(dir.clone()).await;
    for path in ["/", "/pkg/app_bg.wasm", "/album/anything"] {
        let a = get(addr, path, &[]).await;
        assert_eq!(a.status, 200, "{path}");
        assert_eq!(a.cache_control.as_deref(), Some("no-cache"), "{path}");
        assert_eq!(
            a.etag.as_deref(),
            Some(format!("\"{}\"", ark_server::web::build_tag(&dir, Some(MODULE))).as_str()),
            "{path}"
        );
        assert_eq!(a.last_modified, None, "{path}: a date is not a validator here");
    }
    // A miss is the page, as before.
    let a = get(addr, "/album/anything", &[]).await;
    assert!(a.body.contains("the page"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_browser_holding_this_build_is_told_so_and_nothing_else() {
    let dir = build("held");
    let addr = serve(dir.clone()).await;
    let first = get(addr, "/pkg/app_bg.wasm", &[]).await;
    let tag = first.etag.clone().unwrap();
    let again = get(addr, "/pkg/app_bg.wasm", &[("If-None-Match", &tag)]).await;
    assert_eq!(again.status, 304);
    assert_eq!(again.etag, Some(tag.clone()));
    assert!(again.body.is_empty());
    // Several tags, ours among them, is still ours.
    let many = format!("\"stale\", {tag}");
    let again = get(addr, "/", &[("If-None-Match", &many)]).await;
    assert_eq!(again.status, 304);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The trap itself. Every file in a store path is modified at second 1 of
/// 1970, so a browser that loaded the last build sends exactly this date and
/// a `ServeDir` left to itself answers `304` about a file that is entirely
/// different. The router does not let it see the date.
#[tokio::test]
async fn the_stores_date_is_not_believed() {
    let dir = build("epoch");
    // As the nix store has it: every file modified at second 1 of 1970.
    let epoch = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1);
    for f in ["index.html", MODULE] {
        std::fs::File::options()
            .write(true)
            .open(dir.join(f))
            .unwrap()
            .set_modified(epoch)
            .unwrap();
    }
    let addr = serve(dir.clone()).await;
    let a = get(addr, "/", &[("If-Modified-Since", "Thu, 01 Jan 1970 00:00:01 GMT")]).await;
    assert_eq!(a.status, 200, "a date from the store means nothing");
    assert!(a.body.contains("the page"));
    // Falsified by deleting the `IF_MODIFIED_SINCE` removal in `validate`:
    // `ServeDir` then compares the date with a file of that same second and
    // answers 304 about a build it never compared.
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_rebuild_is_a_different_tag() {
    let dir = build("rebuild");
    let addr = serve(dir.clone()).await;
    let before = get(addr, "/", &[]).await.etag.unwrap();
    std::fs::write(dir.join(MODULE), b"\0asm the second build, longer").unwrap();
    let after = get(addr, "/", &[("If-None-Match", &before)]).await;
    assert_eq!(after.status, 200, "the old tag is not this build");
    assert_ne!(after.etag.unwrap(), before);
    let _ = std::fs::remove_dir_all(&dir);
}
