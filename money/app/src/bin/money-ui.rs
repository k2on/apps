//! `money-ui`: the phones' screens written from money's views
//! (`nix run .#update-ui`, from the repository's root). Each view is run
//! over paths (`record()`) and its recording written as SwiftUI and
//! Compose; `checks.ui` fails when what is committed is not what this
//! writes.
//!
//!   money-ui write [ROOT]   the generated files, under ROOT (the repository's root by default)
//!   money-ui files          the files it writes, one a line

use std::path::Path;

use ark_ui_gen::{swift, Screen};
use money_app::accounts::view::accounts;

const ACCOUNTS: Screen = Screen {
    name: "AccountsView",
    holder: "AccountsScreen",
    msg: "AccountsMsg",
    record: "AccountsPage",
    source: "money/app/src/accounts/view.rs",
    mark: "AccountMark(kind: {})",
    package: "dev.money.android",
    imports: &[],
};

/// Every generated file and what it holds.
fn files() -> Vec<(&'static str, String)> {
    let recorded = accounts::record();
    vec![("money/ios/Money/Generated/AccountsView.swift", swift::emit(&recorded, &ACCOUNTS))]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["files"] => files().iter().for_each(|(p, _)| println!("{p}")),
        ["write", ref rest @ ..] if rest.len() <= 1 => {
            let root = Path::new(rest.first().copied().unwrap_or("."));
            for (p, text) in files() {
                let to = root.join(p);
                std::fs::create_dir_all(to.parent().expect("a file's directory")).expect("its directory made");
                std::fs::write(&to, text).expect("the file written");
                println!("{}", to.display());
            }
        }
        _ => {
            eprintln!("money-ui write [ROOT] | files");
            std::process::exit(2)
        }
    }
}
