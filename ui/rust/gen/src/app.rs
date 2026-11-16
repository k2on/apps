//! An app's screens as the generator takes them, and the command line nix
//! runs over them (ui/docs/app.md, "Generated, Not Committed").
//!
//! An app never has a generator of its own. nix writes a one-line main for
//! it (`fn main() { ark_ui_gen::app::run::<money_app::App>(…) }`, over its
//! crate's declaration: [`run`]), links it with the app's crate, and runs
//! it while it builds the app's native projects, which then hold the
//! generated SwiftUI and Compose, and for the app's manifest; nothing
//! generated is committed.
//!
//!   <generator> manifest                the app's manifest, as JSON
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

/// The command line over an app's declaration (`ark_app::app!`): every
/// view it declares written under the names its manifest gives them, the
/// first run's among them, and `manifest`, the manifest itself as JSON
/// (what ui/prebuild.py reads). `namespace` is its library's UniFFI
/// namespace (`money_ffi`), `dir` its crate's directory from the
/// repository's root (`money/app`), which each file's header names.
///
///   <generator> manifest                the manifest, as JSON
///   <generator> write DIR --package P   every view's Swift and Kotlin
///   <generator> files                   the files it writes
pub fn run<H: ark_app::manifest::Declared>(namespace: &'static str, dir: &str) {
    use ark_app::manifest::{Decl, Declare, Manifest};

    let manifest = Manifest::of::<H>();
    if std::env::args().nth(1).as_deref() == Some("manifest") {
        println!("{}", manifest.json());
        return;
    }
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let names = |d: &Decl| {
        let n = manifest.names(d.screen, d.name).expect("every view declared is in the manifest");
        Names {
            swift: leak(n.swift.clone()),
            swift_holder: leak(n.swift_holder.clone()),
            kotlin: leak(n.kotlin.clone()),
            kotlin_holder: leak(n.kotlin_holder.clone()),
            source: leak(format!("{dir}/{}", n.source)),
            msg: leak(d.msg.rsplit("::").next().unwrap_or(d.msg).to_string()),
            screen: d.screen,
        }
    };
    struct Collect<'a> {
        names: &'a dyn Fn(&Decl) -> Names,
        own: Vec<Generated>,
        further: Vec<Generated>,
    }
    impl Declare for Collect<'_> {
        fn view<M: Debug + 'static>(&mut self, d: Decl, record: fn() -> View<M>) {
            let g = Generated::view(record, (self.names)(&d));
            match d.name.is_empty() {
                true => self.own.push(g),
                false => self.further.push(g),
            }
        }
        fn commands<M: Debug + 'static>(&mut self, d: Decl, menus: fn() -> Vec<CommandMenu<M>>) {
            self.further.push(Generated::commands(menus, (self.names)(&d)));
        }
    }
    let mut c = Collect {
        names: &names,
        own: vec![],
        further: vec![],
    };
    H::declare(&mut c);
    // Its screens', the first run's (every app's), then the further views.
    let mut screens = c.own;
    screens.push(Generated::view(
        ark_app::welcome::welcome::record,
        Names {
            swift: "WelcomeView",
            swift_holder: "FirstRunScreen",
            kotlin: "WelcomeScreen",
            kotlin_holder: "FirstRunHolder",
            source: "ui/rust/app/src/welcome.rs",
            msg: "WelcomeMsg",
            screen: "welcome",
        },
    ));
    screens.extend(c.further);
    main(App { namespace, screens })
}
