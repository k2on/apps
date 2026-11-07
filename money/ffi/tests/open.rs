//! money's peer through the bridge: opened on a disk with no server, the budget set
//! up and an account opened by name, seen in a typed view, and a refusal
//! said as a value.

use ark::format::Encode;
use ark_ffi::Refusal;
use money_domain::AccountLine;
use money_ffi::{money_domain, money_open};

/// Falsified by `config` registering the module alone (no domain): there
/// was no function `open_account`; and by `mutate` not throwing its
/// refusal: the empty name was no `Refusal::Refused`.
#[test]
fn money_opens_and_is_called_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let p = money_open(dir.path().to_str().unwrap().into(), None).unwrap();
    p.mutate("budget.setup".into(), "null".into()).unwrap();
    let accounts = p.view("accounts.accounts".into(), "null".into()).unwrap();
    assert!(accounts.rows().is_empty());
    let open = r#"{"name":"Checking","kind":"checking","on_budget":true,"balance":120000,"date":"2026-10-01"}"#;
    let id = p.mutate("accounts.open_account".into(), open.into()).unwrap();
    let o = p.outcome(id).unwrap();
    assert!(o.pending && o.refused.is_none(), "no server: pending, kept here");
    let t = accounts.take();
    assert!(!t.ops.is_empty(), "{t:?}");
    assert_eq!(t.rows.len(), 1);
    let opened = AccountLine::from_bytes(&t.rows[0]).unwrap();
    assert_eq!((opened.name.as_str(), opened.balance), ("Checking", 120_000));

    let nameless = r#"{"name":" ","kind":"checking","on_budget":true,"balance":0,"date":"2026-10-01"}"#;
    assert_eq!(
        p.check("accounts.open_account".into(), nameless.into()).as_deref(),
        Some("an account needs a name")
    );
    assert_eq!(
        p.mutate("accounts.open_account".into(), nameless.into()),
        Err(Refusal::Refused {
            why: "an account needs a name".into()
        })
    );
    let (hash, version) = money_domain().split_once(' ').map(|(h, v)| (h.to_string(), v.to_string())).unwrap();
    assert_eq!((hash.len(), version.as_str()), (64, money_domain::VERSION));
}
