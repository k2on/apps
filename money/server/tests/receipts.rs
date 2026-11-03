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
    let orders = include_str!("../../domain/tests/stores/amazon-orders.json");
    let charges = include_str!("../../domain/tests/stores/amazon-transactions.json");
    let p = dir.join("amazon-orders");
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\n[ \"$AMAZON_PASSWORD\" = hunter2 ] || exit 3\nif [ \"$1\" = transactions ]; then\ncat <<'EOF'\n{charges}\nEOF\nelse\ncat <<'EOF'\n{orders}\nEOF\nfi\n"
        ),
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
            charges_args: vec!["transactions".into()],
            username_file: Some(file("u", "me@example.com")),
            password_file: Some(file("p", "hunter2")),
            otp_secret_file: None,
        }),
        costco: Default::default(),
        dir: Some(drop.path().to_path_buf()),
    };
    let server = serve(&rt, data.path(), |c| c.stores = stores);
    let r = runs(&server, 1);
    assert_eq!(r[0].stores["amazon"], Outcome::Receipts { receipts: 2 });
    assert_eq!(r[0].stores["amazon charges"], Outcome::Charges { charges: 3 });
    let charges: Vec<Charge> = hub_rows(&server, "charge");
    assert_eq!(charges.len(), 3);
    assert!(charges.iter().all(|c| c.receipt_id == "amazon:112-1234567-1234567"));
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
    assert_eq!(r[1].stores.len(), 2, "amazon's orders and charges, and an empty folder");
    assert_eq!(hub_rows::<Receipt>(&server, "receipt"), receipts);
    let said = server.importer.as_ref().unwrap().said().join("\n");
    assert!(!said.contains("hunter2") && !said.contains("me@example.com"), "{said}");
}

/// Amazon's login given in the app's Settings goes over the socket as a
/// call of the server's own and is kept in the server's own file, in no
/// row; the app is told who, masked, and never the password; a bad one is
/// refused; and the login, removed, is gone. Nothing of it is in the log,
/// and the importer's own account sets up nothing.
#[test]
fn settings_keep_amazons_login_off_the_budget() {
    use money_domain::setup::{self as calls, AmazonLogin, CostcoPage, Stores};
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    for path in ["/settings", "/stores"] {
        let code = match ureq::get(&format!("{}{path}", server.url())).call() {
            Ok(r) => r.status(),
            Err(ureq::Error::Status(code, _)) => code,
            Err(e) => panic!("{path}: {e}"),
        };
        assert_ne!(code, 200, "{path}: the server has no page of its own for it");
    }

    let mut alice = signed_in(&server, "alice");
    let s: Stores = alice
        .call(
            calls::AMAZON,
            &AmazonLogin {
                username: "me@example.com".into(),
                password: "hunter2".into(),
                otp_secret: "ABCD EFGH".into(),
            },
        )
        .unwrap();
    assert_eq!(
        (s.amazon.who.as_deref(), s.amazon.otp, s.amazon.program),
        (Some("m••••@example.com"), true, false)
    );
    assert!(!format!("{s:?}").contains("hunter2"));
    let vault = money_server::vault::Vault::load(data.path()).unwrap();
    assert_eq!(
        vault.amazon.as_ref().map(|a| (a.password.as_str(), a.otp_secret.as_deref())),
        Some(("hunter2", Some("ABCDEFGH")))
    );
    let bad = alice.call::<Stores>(calls::AMAZON, &AmazonLogin::default()).unwrap_err();
    assert!(bad.contains("email and password"), "{bad}");
    assert_eq!(alice.call::<Stores>(calls::STATUS, &()).unwrap(), s);

    // Costco's sign-in begins on Costco's own page, as its app's does.
    let page: CostcoPage = alice.call(calls::COSTCO_SIGN_IN, &()).unwrap();
    assert!(
        page.url.starts_with("https://signin.costco.com/") && page.url.contains("code_challenge="),
        "{}",
        page.url
    );
    let none = alice.call::<Stores>(
        calls::COSTCO,
        &calls::CostcoAddress {
            address: "https://www.costco.com/".into(),
        },
    );
    assert!(none.unwrap_err().contains("no code"));

    // Somebody in the household makes a change, so the log has entries.
    let mut bob = signed_in(&server, "bob");
    bob.mutate("setup", &());
    pump_until(&mut [&mut bob], 10, |ps| ps[0].pending() == 0);

    let s: Stores = alice.call(calls::FORGET_AMAZON, &()).unwrap();
    assert_eq!(s.amazon.who, None);
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
