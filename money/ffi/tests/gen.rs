//! The typed Kotlin and Swift the phones call money with are what the
//! generator writes from the domain now (`arkdb gen`): regenerating moves
//! nothing. `ARK_WRITE_GEN=1` writes them instead.

use ark::domain::Catalog;
use arkdb::gen::{kotlin, swift, walk};

const KOTLIN: &str = "../android/app/src/main/kotlin/dev/money/android/gen/Money.kt";
const SWIFT: &str = "../ios/Money/Gen/Money.swift";

fn held(path: &str, now: String) {
    let path = format!("{}/{path}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("ARK_WRITE_GEN").is_some() {
        std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
        return std::fs::write(&path, now).unwrap();
    }
    let was = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}; ARK_WRITE_GEN=1 writes it"));
    assert!(was == now, "{path} is not what the domain generates now; ARK_WRITE_GEN=1 writes it");
}

/// The view rows the domain hands UniFFI (`#[ark(ffi_view)]`), and what
/// they hold (`#[ark(ffi)]`): of these the generator writes only their
/// JSON, and their views are read typed.
const TYPED: &[&str] = &[
    "AccountLine",
    "BankLine",
    "BudgetLine",
    "Cleared",
    "GroupLine",
    "LinkedLine",
    "PayeeLine",
    "SpendingLine",
    "Summary",
    "TxnLine",
];

/// Falsified by a field added to `AddTransaction` (`note: String`): both
/// files differed from what was generated; and by `TxnLine` left out of
/// `TYPED`: the Swift declared `TxnLine` again and read the register as
/// JSON.
#[test]
fn the_phones_types_are_the_domains() {
    let w = walk("money", &Catalog::of(&money_domain::domain())).typed("uniffi.money_domain", TYPED);
    held(KOTLIN, kotlin::write(&w, "dev.money.android.gen"));
    held(SWIFT, swift::write(&w));
}
