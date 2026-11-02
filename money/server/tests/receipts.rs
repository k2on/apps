//! The stores, inside the server: `amazon-orders` run signed in at each
//! run, and a folder of exports read and put away — each store's receipts
//! authored and confirmed, and a run said again changing nothing.

mod common;

use common::*;
use money_domain::*;
use money_server::importer::Outcome;
use money_server::stores::{AmazonOrders, Stores};

/// A stand-in for `amazon-orders`: what it prints, when signed in.
fn program(dir: &std::path::Path) -> std::path::PathBuf {
    let json = include_str!("../../domain/tests/stores/amazon-orders.json").replace('\'', "'\\''");
    let p = dir.join("amazon-orders");
    std::fs::write(
        &p,
        format!("#!/bin/sh\n[ \"$AMAZON_PASSWORD\" = hunter2 ] || exit 3\ncat <<'EOF'\n{json}\nEOF\n"),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn the_stores_receipts_are_imported() {
    let rt = runtime();
    let (data, secrets, drop) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let file = |name: &str, text: &str| {
        let p = secrets.path().join(name);
        std::fs::write(&p, text).unwrap();
        p
    };
    std::fs::create_dir_all(drop.path().join("costco")).unwrap();
    std::fs::write(
        drop.path().join("costco/october.json"),
        include_str!("../../domain/tests/stores/costco.json"),
    )
    .unwrap();
    std::fs::create_dir_all(drop.path().join("target")).unwrap();
    std::fs::write(drop.path().join("target/broken.json"), "<html>").unwrap();
    let stores = Stores {
        amazon: Some(AmazonOrders {
            program: program(secrets.path()),
            args: vec!["orders".into()],
            username_file: Some(file("u", "me@example.com")),
            password_file: Some(file("p", "hunter2")),
            otp_secret_file: None,
        }),
        dir: Some(drop.path().to_path_buf()),
    };
    let server = serve(&rt, data.path(), |c| c.stores = stores);
    let r = runs(&server, 1);
    assert_eq!(r[0].stores["amazon"], Outcome::Receipts { receipts: 2 });
    assert_eq!(r[0].stores["costco/october.json"], Outcome::Receipts { receipts: 2 });
    assert!(matches!(&r[0].stores["target/broken.json"], Outcome::Failed(why) if why.contains("not JSON")));
    assert!(drop.path().join("costco/imported/october.json").exists());
    assert!(drop.path().join("target/failed/broken.json").exists());
    let receipts: Vec<Receipt> = hub_rows(&server, "receipt");
    assert_eq!(receipts.len(), 4);
    let items: Vec<Item> = hub_rows(&server, "item");
    assert!(items.iter().any(|i| i.name == "Gas REGULAR" && i.quantity == 11_482));
    // Asked again, Amazon's same orders change nothing, and the folder is
    // empty.
    server.importer.as_ref().unwrap().nudge();
    let r = runs(&server, 2);
    assert_eq!(r[1].stores.len(), 1);
    assert_eq!(hub_rows::<Receipt>(&server, "receipt"), receipts);
    let said = server.importer.as_ref().unwrap().said().join("\n");
    assert!(!said.contains("hunter2") && !said.contains("me@example.com"), "{said}");
}

/// The stores page's answers, over HTTP as the page asks them.
fn call(server: &money_server::Server, method: &str, path: &str, token: Option<&str>, body: Option<&str>) -> (u16, String) {
    let url = format!("{}{path}", server.url());
    let mut r = ureq::request(method, &url);
    if let Some(t) = token {
        r = r.set("Authorization", &format!("Bearer {t}"));
    }
    let got = match body {
        Some(b) => r.set("Content-Type", "application/json").send_string(b),
        None => r.call(),
    };
    match got {
        Ok(resp) => (resp.status(), resp.into_string().unwrap()),
        Err(ureq::Error::Status(code, resp)) => (code, resp.into_string().unwrap()),
        Err(e) => panic!("{method} {path}: {e}"),
    }
}

/// Amazon's login given on the settings page is kept in the server's own
/// file and in no row; the page is told who, masked; nobody signed out is
/// answered anything; and the login, removed, is gone.
#[test]
fn the_settings_page_keeps_amazons_login_off_the_budget() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    for path in ["/settings", "/stores"] {
        let page = call(&server, "GET", path, None, None);
        assert!(page.0 == 200 && page.1.contains("Amazon") && !page.1.contains("Costco"), "{path}");
    }
    assert_eq!(call(&server, "GET", "/stores/api/status", None, None).0, 401);
    assert_eq!(call(&server, "GET", "/stores/api/status", Some("forged.00"), None).0, 401);

    let alice = login(&server, "alice");
    let t = Some(alice.token.as_str());
    let (code, said) = call(
        &server,
        "PUT",
        "/stores/api/amazon",
        t,
        Some(r#"{"username":"me@example.com","password":"hunter2","otp_secret":"ABCD EFGH"}"#),
    );
    assert_eq!(code, 200, "{said}");
    let status: serde_json::Value = serde_json::from_str(&said).unwrap();
    assert_eq!(status["amazon"]["login"]["who"], "m••••@example.com");
    assert_eq!(status["amazon"]["login"]["otp"], true);
    assert_eq!(status["amazon"]["program"], false);
    assert!(!said.contains("hunter2") && !said.contains("ABCD"), "{said}");
    let vault = money_server::vault::Vault::load(data.path()).unwrap();
    assert_eq!(
        vault.amazon.as_ref().map(|a| (a.password.as_str(), a.otp_secret.as_deref())),
        Some(("hunter2", Some("ABCDEFGH")))
    );
    let (code, said) = call(&server, "PUT", "/stores/api/amazon", t, Some(r#"{"username":"","password":""}"#));
    assert_eq!(code, 422, "{said}");

    // Somebody in the household makes a change, so the log has entries.
    let mut bob = signed_in(&server, "bob");
    bob.mutate("setup", &());
    pump_until(&mut [&mut bob], 10, |ps| ps[0].pending() == 0);

    call(&server, "DELETE", "/stores/api/amazon", t, None);
    assert_eq!(money_server::vault::Vault::load(data.path()).unwrap().amazon, None);

    let mut seen = vec![];
    for e in walk(&data.path().join("log")) {
        seen.extend(std::fs::read(e).unwrap());
    }
    assert!(!seen.is_empty());
    assert!(!seen.windows(7).any(|w| w == b"hunter2"), "the password is in the log");
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
