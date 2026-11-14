//! An app's screens as the generator takes them: the registry an app's
//! crate hands over (`money_app::screens()`), and the command line nix runs
//! over it (ui/docs/app.md, "Generated, Not Committed").
//!
//! An app never has a generator of its own. nix writes a tiny main for it
//! (`fn main() { ark_ui_gen::app::main(money_app::screens()) }`), links it
//! with the app's crate under its `gen` feature, and runs it while it
//! builds the app's native projects, which then hold the generated SwiftUI
//! and Compose; nothing generated is committed.
//!
//!   <generator> write DIR --package P   every screen's Swift and Kotlin, into DIR
//!   <generator> files                   the files it writes, one a line

use std::fmt::Debug;
use std::path::Path;

use ark_app::shell::CommandMenu;

use crate::{kotlin, swift, Screen, View};

/// How a screen writes its two files from its two `Screen`s.
type Write = Box<dyn Fn(&Screen, &Screen) -> (String, String)>;

/// What one of an app's screens writes, Swift's and Kotlin's.
pub struct Generated {
    /// The Swift file's view, and its holder (the hand-written
    /// `ObservableObject` over the runtime it reads).
    pub swift: (&'static str, &'static str),
    /// The Kotlin file's composable, and its holder.
    pub kotlin: (&'static str, &'static str),
    /// The Rust file it is written from, for the header.
    pub source: &'static str,
    /// The message type its actions send, as UniFFI names it.
    pub msg: &'static str,
    /// Its walk name (app.toml's `[screens]` key).
    pub screen: &'static str,
    write: Write,
}

impl Generated {
    /// A `#[view]`'s recording (`accounts::record`).
    pub fn view<M: Debug + 'static>(record: fn() -> View<M>, names: Names) -> Generated {
        Generated::of(names, move |s, k| {
            let v = record();
            (swift::emit(&v, s), kotlin::emit(&v, k))
        })
    }

    /// An app's menu bar (`commands()`).
    pub fn commands<M: Debug + 'static>(menus: fn() -> Vec<CommandMenu<M>>, names: Names) -> Generated {
        Generated::of(names, move |s, k| {
            let m = menus();
            (swift::emit_commands(&m, s), kotlin::emit_commands(&m, k))
        })
    }

    fn of(n: Names, write: impl Fn(&Screen, &Screen) -> (String, String) + 'static) -> Generated {
        Generated {
            swift: (n.swift, n.swift_holder),
            kotlin: (n.kotlin, n.kotlin_holder),
            source: n.source,
            msg: n.msg,
            screen: n.screen,
            write: Box::new(write),
        }
    }
}

/// A screen's names in each language.
#[derive(Clone, Copy, Debug)]
pub struct Names {
    pub swift: &'static str,
    pub swift_holder: &'static str,
    pub kotlin: &'static str,
    pub kotlin_holder: &'static str,
    pub source: &'static str,
    pub msg: &'static str,
    /// Its walk name (app.toml's `[screens]` key): each element's id is
    /// under it.
    pub screen: &'static str,
}

/// An app's every generated screen, and its UniFFI namespace (its
/// generated library's: `money_ffi`), which the Kotlin imports its
/// messages from.
pub struct App {
    pub namespace: &'static str,
    pub screens: Vec<Generated>,
}

/// Each screen's two files, under the names they are written as: `(file,
/// text)`, `AccountsView.swift` and `AccountsScreen.kt`.
pub fn files(app: &App, package: &str) -> Vec<(String, String)> {
    // The emitters take a screen's words as `&'static str`; a package said
    // at run time lives as long as the run.
    let package: &'static str = Box::leak(package.to_string().into_boxed_str());
    let mut out = vec![];
    for g in &app.screens {
        let import: &'static str = Box::leak(format!("uniffi.{}.{}", app.namespace, g.msg).into_boxed_str());
        let imports: &'static [&'static str] = Box::leak(vec![import].into_boxed_slice());
        let s = Screen {
            name: g.swift.0,
            holder: g.swift.1,
            source: g.source,
            package,
            root: "p",
            imports,
            screen: g.screen,
        };
        let k = Screen {
            name: g.kotlin.0,
            holder: g.kotlin.1,
            ..s.clone()
        };
        let (sw, kt) = (g.write)(&s, &k);
        out.push((format!("{}.swift", g.swift.0), sw));
        out.push((format!("{}.kt", g.kotlin.0), kt));
    }
    out
}

/// The command line over `app` (the module's head says it).
pub fn main(app: App) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["files"] => files(&app, "app").iter().for_each(|(p, _)| println!("{p}")),
        ["write", dir, "--package", package] => {
            let dir = Path::new(dir);
            std::fs::create_dir_all(dir).expect("the directory made");
            for (p, text) in files(&app, package) {
                std::fs::write(dir.join(&p), text).expect("the file written");
                println!("{p}");
            }
        }
        _ => {
            eprintln!("write DIR --package P | files");
            std::process::exit(2)
        }
    }
}
