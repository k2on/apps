//! The stores, inside the server: `amazon-orders` run signed in at each
//! run, and a folder of exports read and put away — each store's receipts
//! authored and confirmed, and a run said again changing nothing.

mod common;

use common::*;
use money_core::*;
use money_server::importer::Outcome;
use money_server::stores::{AmazonOrders, Stores};

/// An export the domain's tests read the stores from (money/core/tests/stores),
/// read as the test runs: a crate is compiled from its own directory alone.
fn domain_store(name: &str) -> String {
    let path = format!("{}/../domain/tests/stores/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A stand-in for `amazon-orders`: what it prints, when signed in.
fn program(dir: &std::path::Path) -> std::path::PathBuf {
    let orders = domain_store("amazon-orders.json");
    let charges = domain_store("amazon-transactions.json");
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
    std::fs::write(drop.path().join("costco/october.json"), domain_store("costco.json")).unwrap();
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
            browser: None,
        }),
        costco: Default::default(),
        dir: Some(drop.path().to_path_buf()),
        ocr: None,
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
    use money_core::setup::{self as calls, AmazonLogin, BrowserPage, Stores};
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
                otp_secret: "JBSW Y3DP EHPK 3PXP".into(),
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
        Some(("hunter2", Some("JBSWY3DPEHPK3PXP")))
    );
    let bad = alice.call::<Stores>(calls::AMAZON, &AmazonLogin::default()).unwrap_err();
    assert!(bad.contains("email and password"), "{bad}");
    // A one-time code given for the secret is refused, and the secret kept.
    let code = AmazonLogin {
        username: "me@example.com".into(),
        password: "hunter2".into(),
        otp_secret: "123 456".into(),
    };
    let bad = alice.call::<Stores>(calls::AMAZON, &code).unwrap_err();
    assert!(bad.contains("isn't an authenticator secret"), "{bad}");
    let kept = money_server::vault::Vault::load(data.path()).unwrap().amazon.unwrap().otp_secret;
    assert_eq!(kept.as_deref(), Some("JBSWY3DPEHPK3PXP"));
    assert_eq!(alice.call::<Stores>(calls::STATUS, &()).unwrap(), s);

    // A server with no browser has nowhere to sign in to Costco, and says so.
    assert!(!s.costco.browser);
    let none = alice.call::<BrowserPage>(calls::COSTCO_SIGN_IN, &()).unwrap_err();
    assert!(none.contains("no browser"), "{none}");

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

/// A photo of a paper receipt, read over the socket: the server's
/// receipt-ocr (a stand-in that prints what the real one saw of a Food Lion
/// receipt) and its reader give the items, sure of them; nothing is kept,
/// and a server with no receipt-ocr says so.
#[test]
fn a_paper_receipt_is_read_from_its_photo() {
    use money_core::setup::{self as calls, PaperReceipt, ReceiptPhoto};
    let rt = runtime();
    let (data, bin) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/paper/foodlion-2025-06-15");
    let readings = format!(r#"{{"readings":[$(cat {dir}.736.json),$(cat {dir}.1280.json),$(cat {dir}.1920.json)]}}"#);
    let ocr = bin.path().join("receipt-ocr");
    std::fs::write(&ocr, format!("#!/bin/sh\n[ -s \"$1\" ] || exit 2\ncat <<EOF\n{readings}\nEOF\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&ocr, std::fs::Permissions::from_mode(0o755)).unwrap();
    let server = serve(&rt, data.path(), |c| c.stores.ocr = Some(ocr.clone()));
    let mut alice = signed_in(&server, "alice");
    let photo = ReceiptPhoto {
        image: "/9j/4AAQSkZJRgABAQ==".into(),
    };
    let p: PaperReceipt = alice.call(calls::READ_RECEIPT, &photo).unwrap();
    assert_eq!(p.state, "sure", "{p:?}");
    assert_eq!((p.items.len(), p.discounts.len(), p.sum, p.tax, p.due), (9, 1, 3217, 23, Some(3240)));
    assert_eq!(p.items[6].cents, 1511, "the ground beef");
    assert!(
        std::fs::read_dir(data.path().join("receipt-photos")).unwrap().next().is_none(),
        "the photo is not kept"
    );
    let bad = alice.call::<PaperReceipt>(calls::READ_RECEIPT, &ReceiptPhoto { image: "not base64!".into() });
    assert!(bad.unwrap_err().contains("not base64"));

    let bare = serve(&rt, tempfile::tempdir().unwrap().path(), |_| {});
    let mut bob = signed_in(&bare, "bob");
    let none = bob.call::<PaperReceipt>(calls::READ_RECEIPT, &photo).unwrap_err();
    assert!(none.contains("no receipt-ocr"), "{none}");
}

/// A photo of a paper receipt taken offline (docs/plan-files.md): kept on
/// the device and pending; on the next connection its file goes up beside
/// its row, the server's importer reads it with receipt-ocr and writes the
/// receipt, and the photo is kept — on the server, and for another device
/// that opens it.
#[test]
fn a_receipt_photo_taken_offline_is_read_by_the_server_once_it_arrives() {
    use money_core::types::{AddPhoto, PhotoLine, ReceiptLine};
    let rt = runtime();
    let (data, bin) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/paper/foodlion-2025-06-15");
    let readings = format!(r#"{{"readings":[$(cat {dir}.736.json),$(cat {dir}.1280.json),$(cat {dir}.1920.json)]}}"#);
    let ocr = bin.path().join("receipt-ocr");
    std::fs::write(&ocr, format!("#!/bin/sh\n[ -s \"$1\" ] || exit 2\ncat <<EOF\n{readings}\nEOF\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&ocr, std::fs::Permissions::from_mode(0o755)).unwrap();
    let server = serve(&rt, data.path(), |c| c.stores.ocr = Some(ocr.clone()));

    // Offline.
    let l = login(&server, "alice");
    let mut alice = open(ark_peer::who(&l));
    let jpeg = [&b"\xff\xd8\xff\xe0 a food lion receipt "[..], &[7u8; 3 << 20][..]].concat();
    let file = alice.client.put_file(&jpeg, "image/jpeg").unwrap();
    alice.mutate(
        "receipts.add_photo",
        &AddPhoto {
            file: file.clone(),
            txn_id: None,
        },
    );
    let photos: Vec<PhotoLine> = alice.query("receipts.photos", &()).unwrap();
    assert_eq!(photos.iter().map(|p| p.state.as_str()).collect::<Vec<_>>(), ["reading"], "there at once");

    alice.connect(&server.url(), &l.token);
    let mut read = vec![];
    pump_until(&mut [&mut alice], 30, |ps| {
        read = ps[0].query::<Vec<PhotoLine>>("receipts.photos", &()).unwrap();
        read.first().is_some_and(|p| p.state != "reading" && p.state != "failed")
    });
    let p = &read[0];
    assert_eq!(p.state, "sure", "{p:?}");
    assert!(alice.client.files_unsent().is_empty(), "sent");
    pump_until(&mut [&mut alice], 10, |ps| {
        let rs: Vec<ReceiptLine> = ps[0].query("receipts.receipts", &()).unwrap();
        rs.iter().any(|r| r.id == p.receipt_id)
    });
    let receipts: Vec<ReceiptLine> = alice.query("receipts.receipts", &()).unwrap();
    let r = receipts.iter().find(|r| r.id == p.receipt_id).unwrap();
    assert_eq!((r.store.as_str(), r.total, r.items), ("paper", -3240, 10), "{r:?}");
    let kept = data.path().join("log").join("files").join(&file.hash);
    assert_eq!(std::fs::read(kept).unwrap(), jpeg, "the server keeps the photo");

    let mut bob = signed_in(&server, "bob");
    pump_until(&mut [&mut bob], 10, |ps| {
        let ps: Vec<PhotoLine> = ps[0].query("receipts.photos", &()).unwrap();
        !ps.is_empty()
    });
    assert_eq!(bob.client.open_file(&file), None, "fetched when opened");
    pump_until(&mut [&mut bob], 10, |ps| ps[0].client.file(&file).is_some());
    assert_eq!(bob.client.file(&file).unwrap(), jpeg);
}
