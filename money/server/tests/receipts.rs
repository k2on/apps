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
            username_file: file("u", "me@example.com"),
            password_file: file("p", "hunter2"),
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
