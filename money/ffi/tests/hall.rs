//! A hall: two people of one household dial a money server in the
//! process, and what one enters is the other's register once carried; a
//! bank import from anybody but the importer is refused, and the
//! importer's is the server's.

use ark_ffi::Standing;
use money_domain::{AccountLineView, TxnLineView};
use money_ffi::Hall;

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// Falsified by the hall's token dropping its roles: the importer's
/// import was rejected as the household's was.
#[test]
fn a_household_shares_one_budget() {
    let (a, b, c) = (dir(), dir(), dir());
    let hall = Hall::new();
    let path = |d: &tempfile::TempDir| d.path().to_str().unwrap().to_string();
    let alex = hall.device(path(&a), "alex".into(), "phone".into(), vec![]).unwrap();
    let sam = hall.device(path(&b), "sam".into(), "phone".into(), vec![]).unwrap();
    alex.mutate("setup".into(), "null".into()).unwrap();
    let open = r#"{"name":"Checking","kind":"checking","on_budget":true,"balance":50000,"date":"2026-10-01"}"#;
    alex.mutate("open_account".into(), open.into()).unwrap();
    hall.carry();
    let accounts = AccountLineView::new(sam.clone(), "accounts".into(), "null".into()).unwrap().rows();
    assert_eq!(accounts.len(), 1, "sam sees alex's account");
    let checking = accounts[0].id;

    let register = TxnLineView::new(alex.clone(), "register".into(), r#"{"account_id":null}"#.into()).unwrap();
    let before = register.rows().len();
    let coffee = format!(
        r#"{{"account_id":"{}","date":"2026-10-02","payee":"Blue Bottle","category_id":null,"memo":"","amount":-650,"cleared":false,"transfer_to":null}}"#,
        ark::hash::hex(&checking.0)
    );
    let id = sam.mutate("add_transaction".into(), coffee).unwrap();
    hall.carry();
    assert_eq!(sam.standing(id), Standing::Confirmed);
    let rows = register.rows();
    assert_eq!(rows.len(), before + 1);
    assert!(rows.iter().any(|t| t.payee == "Blue Bottle" && t.user_id == "sam"));

    let import = r#"{"bank":"chase","institution":"Chase","cursor":"c1","accounts":[],"upserts":[],"removed":[]}"#;
    assert!(alex.mutate("import".into(), import.into()).is_err(), "only the importer imports");
    let importer = hall.device(path(&c), "importer".into(), "plaid".into(), vec!["bank".into()]).unwrap();
    let id = importer.mutate("import".into(), import.into()).unwrap();
    hall.carry();
    assert_eq!(importer.standing(id), Standing::Confirmed);
}
