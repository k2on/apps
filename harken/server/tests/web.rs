//! The browser client, served by harken's server with the build as its
//! validator — over a real socket, because the bug it fixes is one only a
//! real HTTP exchange has: a `304` answered to a date every file in the nix
//! store shares, and a browser that therefore kept an old client for years.
//!
//! The validator itself is ark-server's and is tested there; this holds the
//! wiring: that harken's server serves `HARKEN_WEB` with it, under the
//! module harken names, and that the page is only what the routes before it
//! do not answer.

mod common;

use std::path::{Path, PathBuf};

use common::*;

const MODULE: &str = "pkg/harken_iced_bg.wasm";

/// A build on disk: a page and a module, off the store.
fn build(root: &Path) -> PathBuf {
    let dir = root.join("web");
    std::fs::create_dir_all(dir.join("pkg")).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html>the page").unwrap();
    std::fs::write(dir.join(MODULE), b"\0asm the first build").unwrap();
    dir
}

struct Answer {
    status: u16,
    etag: Option<String>,
    cache_control: Option<String>,
    last_modified: Option<String>,
    body: String,
}

/// One GET, with whatever conditional headers a browser would send.
fn get(base: &str, path: &str, headers: &[(&str, &str)]) -> Answer {
    let mut req = ureq::get(&format!("{base}{path}"));
    for (k, v) in headers {
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
}

#[test]
fn every_file_carries_the_build_and_no_date() {
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let web = build(root.path());
    let server = serve(&rt, &root.path().join("data"), |c| {
        c.web = Some(web.clone());
        c.web_module = Some(MODULE.into());
    });
    let base = server.url();
    let tag = format!("\"{}\"", ark_server::web::build_tag(&web, Some(MODULE)));
    for path in ["/", "/pkg/harken_iced_bg.wasm", "/album/Water%20Music"] {
        let a = get(&base, path, &[]);
        assert_eq!(a.status, 200, "{path}");
        assert_eq!(a.cache_control.as_deref(), Some("no-cache"), "{path}");
        assert_eq!(a.etag.as_deref(), Some(tag.as_str()), "{path}");
        assert_eq!(
            a.last_modified, None,
            "{path}: a date is not a validator here"
        );
    }
    // A miss is the page, which is what makes a reload of a deep link work.
    assert!(get(&base, "/album/anything", &[]).body.contains("the page"));

    // The routes before it are still themselves.
    let health = get(&base, "/healthz", &[]);
    assert!(health.body.starts_with("ok\n"), "{}", health.body);
    assert_eq!(
        get(&base, "/auth/me", &[]).status,
        401,
        "sign-in answers, not the page"
    );
    rt.block_on(server.stop());
}

#[test]
fn a_browser_holding_this_build_is_told_so_and_a_rebuild_is_not() {
    let rt = runtime();
    let root = tempfile::tempdir().unwrap();
    let web = build(root.path());
    let server = serve(&rt, &root.path().join("data"), |c| {
        c.web = Some(web.clone());
        c.web_module = Some(MODULE.into());
    });
    let base = server.url();
    let tag = get(&base, "/pkg/harken_iced_bg.wasm", &[]).etag.unwrap();
    let again = get(
        &base,
        "/pkg/harken_iced_bg.wasm",
        &[("If-None-Match", &tag)],
    );
    assert_eq!(again.status, 304);
    assert!(again.body.is_empty());

    // The trap itself: every file in a store path is dated second 1 of 1970,
    // so a browser that loaded the last build sends exactly this date.
    let stale = get(
        &base,
        "/",
        &[("If-Modified-Since", "Thu, 01 Jan 1970 00:00:01 GMT")],
    );
    assert_eq!(stale.status, 200, "a date from the store means nothing");
    assert!(stale.body.contains("the page"));

    // A rebuild writes a different module, and the old tag is not this build.
    std::fs::write(web.join(MODULE), b"\0asm the second build, longer").unwrap();
    let after = get(&base, "/", &[("If-None-Match", &tag)]);
    assert_eq!(after.status, 200);
    assert_ne!(after.etag.unwrap(), tag);
    rt.block_on(server.stop());
}
