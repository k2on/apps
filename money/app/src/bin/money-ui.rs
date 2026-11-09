//! `money-ui`: the phones' screens written from money's views
//! (`nix run .#update-ui`, from the repository's root). Each view is run
//! over paths (`record()`) and its recording written as SwiftUI and
//! Compose; `checks.ui` fails when what is committed is not what this
//! writes.
//!
//!   money-ui write [ROOT]   the generated files, under ROOT (the repository's root by default)
//!   money-ui files          the files it writes, one a line

use std::path::Path;

use ark_ui_gen::{kotlin, swift, Screen};
use money_app::accounts::view::accounts;
use money_app::shell::view::{commands, money_settings, money_shell};

const ACCOUNTS: Screen = Screen {
    name: "AccountsView",
    holder: "AccountsScreen",
    source: "money/app/src/accounts/view.rs",
    package: "dev.money.android",
    root: "p",
    imports: &["uniffi.money_app.AccountsMsg"],
};

const KOTLIN_ACCOUNTS: Screen = Screen {
    name: "AccountsScreen",
    holder: "AccountsHolder",
    ..ACCOUNTS
};

/// The shell's main window, Settings and commands (money/app/src/shell).
const SHELL: Screen = Screen {
    name: "MoneyShellView",
    holder: "MoneyShellScreen",
    source: "money/app/src/shell/view.rs",
    package: "dev.money.android",
    root: "p",
    imports: &["uniffi.money_app.ShellMsg"],
};

const SETTINGS: Screen = Screen {
    name: "MoneySettingsView",
    ..SHELL
};

const COMMANDS: Screen = Screen {
    name: "MoneyCommands",
    ..SHELL
};

const KOTLIN_SHELL: Screen = Screen {
    name: "MoneyShell",
    holder: "MoneyShellHolder",
    ..SHELL
};

const KOTLIN_SETTINGS: Screen = Screen {
    name: "MoneySettings",
    ..KOTLIN_SHELL
};

const KOTLIN_COMMANDS: Screen = Screen {
    name: "moneyCommands",
    ..KOTLIN_SHELL
};

/// Every generated file and what it holds.
fn files() -> Vec<(&'static str, String)> {
    let recorded = accounts::record();
    let shell = money_shell::record();
    let settings = money_settings::record();
    let menus = commands();
    vec![
        ("money/ios/Money/Generated/AccountsView.swift", swift::emit(&recorded, &ACCOUNTS)),
        (
            "money/android/app/src/main/kotlin/dev/money/android/generated/AccountsScreen.kt",
            kotlin::emit(&recorded, &KOTLIN_ACCOUNTS),
        ),
        ("money/ios/Money/Generated/MoneyShellView.swift", swift::emit(&shell, &SHELL)),
        ("money/ios/Money/Generated/MoneySettingsView.swift", swift::emit(&settings, &SETTINGS)),
        ("money/ios/Money/Generated/MoneyCommands.swift", swift::emit_commands(&menus, &COMMANDS)),
        (
            "money/android/app/src/main/kotlin/dev/money/android/generated/MoneyShell.kt",
            kotlin::emit(&shell, &KOTLIN_SHELL),
        ),
        (
            "money/android/app/src/main/kotlin/dev/money/android/generated/MoneySettings.kt",
            kotlin::emit(&settings, &KOTLIN_SETTINGS),
        ),
        (
            "money/android/app/src/main/kotlin/dev/money/android/generated/MoneyCommands.kt",
            kotlin::emit_commands(&menus, &KOTLIN_COMMANDS),
        ),
    ]
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
