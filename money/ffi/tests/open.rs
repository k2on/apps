//! money's peer through the bridge: opened alone on a disk, the budget set
//! up and an account opened by name, seen in a typed view, and a refusal
//! said as a value.

use ark_ffi::Taken;
use money_domain::AccountLineView;
use money_ffi::{money_domain, money_open, money_send, Sent};

/// Falsified by `config` registering the module alone (no domain): there
/// was no function `open_account`; and by `money_send` throwing its
/// refusal: the empty name was no `Sent::Refused`.
#[test]
fn money_opens_and_is_called_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let p = money_open(dir.path().to_str().unwrap().into(), None).unwrap();
    p.alone();
    p.mutate("setup".into(), "null".into()).unwrap();
    let accounts = AccountLineView::new(p.clone(), "accounts".into(), "null".into()).unwrap();
    assert!(accounts.rows().is_empty());
    let open = r#"{"name":"Checking","kind":"checking","on_budget":true,"balance":120000,"date":"2026-10-01"}"#;
    let Sent::Intent { id } = money_send(p.clone(), "open_account".into(), open.into()) else {
        panic!("opened")
    };
    assert_eq!(p.standing(id), ark_ffi::Standing::Confirmed, "alone, its own authority");
    let t = accounts.take();
    assert!(matches!(t.update, Taken::Patched { .. }), "{t:?}");
    assert_eq!(t.rows.len(), 1);
    assert_eq!((t.rows[0].name.as_str(), t.rows[0].balance), ("Checking", 120_000));

    let nameless = r#"{"name":" ","kind":"checking","on_budget":true,"balance":0,"date":"2026-10-01"}"#;
    assert_eq!(
        p.check("open_account".into(), nameless.into()).as_deref(),
        Some("an account needs a name")
    );
    assert_eq!(
        money_send(p.clone(), "open_account".into(), nameless.into()),
        Sent::Refused {
            why: "an account needs a name".into()
        }
    );
    let (hash, version) = money_domain().split_once(' ').map(|(h, v)| (h.to_string(), v.to_string())).unwrap();
    assert_eq!((hash.len(), version.as_str()), (64, money_domain::VERSION));
}
