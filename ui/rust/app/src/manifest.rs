//! An app's manifest (ui/docs/app.md, "The App's Declaration"): what its
//! generated targets are written from that is said in its Rust, never in
//! its app.toml. An app declares itself once, at its crate's root, with
//! [`app!`](crate::app): its part of its host, each screen's runtime and
//! `#[view]`, and each screen's further views (Settings, the menu bar's
//! commands, a now-playing bar). Everything else follows by convention:
//!
//! - **the names its generated code is written under**: a screen's runtime
//!   object `<Screen>App` (its name in the declaration, `accounts` →
//!   `AccountsApp`); its page and message by their types' own names; its
//!   view's `<View>View` in Swift and `<View>Screen` in Kotlin (the shell's
//!   own `<View>`), and the holders over its runtime `<View>Screen` and
//!   `<View>Holder` (`<View>` its `#[view]` function's name in Pascal case:
//!   `money_shell` → `MoneyShell`); a further view's `<View>View` and
//!   `<View>`; the menu bar's `<App>Commands` and `<app>Commands`, `<App>`
//!   the part of its host's type (`Money`);
//! - **the shell**: the screen marked `#[shell]`, over it the one marked
//!   `#[over]`, its further views `settings` (Settings) and `commands` (the
//!   menu bar's), the shell page's fields `section`, `appearance`,
//!   `household` (who is signed in, also the name the shell hosts as the
//!   account) and `browse` (a page shown in the system's browser while it
//!   is set), and the message `PageClosed { url }` that says it closed;
//!   what each name the shell's views host (`hosted("budget")`) draws: the
//!   screen of that name, a further view by its `<screen>.<view>` name
//!   (`player.bar`), or the screen its last part names
//!   (`accounts.register`);
//! - **what crosses to the phones**: every type reached from the screens'
//!   pages and messages, the route and the request (`ark::twin::Reach`,
//!   which a twin's derive writes), with the first run's and the shell's
//!   records every app's.
//!
//! The manifest is printed as JSON by the app's generated `<id>-gen
//! manifest` (ark-ui-gen's command line over the declaration), which
//! ui/prebuild.py reads (`ARK_MANIFEST`) where app.toml's `[screens]`,
//! `[views]`, `[shell]` and `[ffi]` once were.

use std::fmt::Debug;

pub use ark::reach;
pub use ark::twin::{Reach, Reached};

use crate::host::HostedApp;
use crate::shell::CommandMenu;
use crate::view::{Part, View};

/// What an app declares of itself ([`app!`](crate::app)): its screens and
/// their views, which is its shell and which goes over it.
pub trait Declared: HostedApp {
    /// The screen that is the shell: the window's, and each element's walk
    /// id is under it (`shots::id_of`). `shell` where none is marked.
    const SHELL: &'static str;
    /// The screen drawn over every section (its sheets and dialogs); none
    /// is empty.
    const OVER: &'static str;
    /// Every screen's view and every further view, to `d`, in the order
    /// declared.
    fn declare<D: Declare>(d: &mut D);
    /// What crosses to the phones: its route and its request, the screens'
    /// pages and messages, and what they reach (written out where the
    /// types are known: whether a type reaches is asked of the type).
    fn reach(r: &mut Reached);
}

/// A view declared: whose runtime it reads and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decl {
    /// Its screen's name in the declaration (`accounts`): its walk name.
    pub screen: &'static str,
    /// A further view's name (`settings`, `bar`); empty for the screen's own.
    pub name: &'static str,
    /// The screen's runtime, its page and its message, as Rust names them
    /// (`money_app::accounts::Accounts`).
    pub app: &'static str,
    pub page: &'static str,
    pub msg: &'static str,
    /// The `#[view]` (or the menus' function) in the app's crate:
    /// `accounts::view::accounts`.
    pub path: String,
    /// A screen the phone runs for as long as it shows it, beside its
    /// host's (`#[beside]`).
    pub beside: bool,
}

/// What takes an app's declared views ([`Declared::declare`]).
pub trait Declare {
    /// A `#[view]`, by its recording (`accounts::record`).
    fn view<M: Debug + 'static>(&mut self, decl: Decl, record: fn() -> View<M>);
    /// The menu bar's commands (`commands()`).
    fn commands<M: Debug + 'static>(&mut self, decl: Decl, menus: fn() -> Vec<CommandMenu<M>>);
}

/// `name` among `marked` (each a mark and a screen's name), the first so
/// marked; `none` where none is.
#[doc(hidden)]
pub const fn marked(marked: &[(&'static str, &'static str)], mark: &str, none: &'static str) -> &'static str {
    let mut i = 0;
    while i < marked.len() {
        if same(marked[i].0, mark) {
            return marked[i].1;
        }
        i += 1;
    }
    none
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// `money_shell` as Swift and Kotlin name a type: `MoneyShell`.
pub fn pascal(name: &str) -> String {
    name.split('_')
        .map(|w| w[..1.min(w.len())].to_uppercase() + &w[1.min(w.len())..])
        .collect()
}

/// The last part of a Rust path: `Money` of `money_app::host::Money`.
fn last(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

/// A Rust path without its crate: `host::Money` of `money_app::host::Money`.
fn in_crate(path: &str) -> &str {
    path.split_once("::").map_or(path, |(_, p)| p)
}

/// A view's names in each toolkit, and its runtime's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Names {
    /// The view, and the holder it reads its runtime through.
    pub swift: String,
    pub swift_holder: String,
    pub kotlin: String,
    pub kotlin_holder: String,
    /// The runtime's UniFFI object (`AccountsApp`).
    pub runtime: String,
    /// Its source, from the app's crate's directory: `src/accounts/view.rs`.
    pub source: String,
    /// Commands rather than a view.
    pub commands: bool,
}

/// Every view an app declares, with its names, and what its shell hosts.
#[derive(Clone, Debug)]
pub struct Manifest {
    /// The part of its host (`money_app::host::Money`), its route and request.
    pub app: &'static str,
    pub route: &'static str,
    pub shell: &'static str,
    pub over: &'static str,
    pub views: Vec<(Decl, Names)>,
    /// Each name the shell's views host that is a screen's or a further
    /// view's, and which (`(name, screen)`, `(name, screen.view)`), in the
    /// order its views host them.
    pub hosted: Vec<(String, String)>,
    /// Every type that crosses to the phones, by its path.
    pub twins: Vec<&'static str>,
}

/// Each declared view, with its recording's hosted names.
#[derive(Default)]
struct Collect(Vec<(Decl, Vec<&'static str>, bool)>);

impl Declare for Collect {
    fn view<M: Debug + 'static>(&mut self, decl: Decl, record: fn() -> View<M>) {
        let mut names = vec![];
        hosted(&record(), &mut names);
        self.0.push((decl, names, false));
    }
    fn commands<M: Debug + 'static>(&mut self, decl: Decl, _: fn() -> Vec<CommandMenu<M>>) {
        self.0.push((decl, vec![], true));
    }
}

/// Every name `v` hosts (`hosted("budget")`), in order, every branch of a
/// recording.
fn hosted<M>(v: &View<M>, out: &mut Vec<&'static str>) {
    match v {
        View::Empty => {}
        View::List(vs) => vs.iter().for_each(|v| hosted(v, out)),
        View::Each { body, .. } => hosted(body, out),
        View::When { yes, no, .. } | View::WhenSome { yes, no, .. } => {
            hosted(yes, out);
            hosted(no, out)
        }
        View::Part(p) => {
            if let Part::Hosted { name } = p {
                out.push(name);
            }
            for (_, slot) in p.call().slots {
                hosted(slot, out);
            }
        }
        View::Layout(l) => l.slots().into_iter().for_each(|v| hosted(v, out)),
    }
}

impl Manifest {
    /// `H`'s manifest, from its declaration.
    pub fn of<H: Declared>() -> Manifest {
        let mut c = Collect::default();
        H::declare(&mut c);
        let part = last(std::any::type_name::<H>()).to_string();
        // Each screen's own view, whose name its holders are of.
        let own = |screen: &str| {
            c.0.iter()
                .find(|(d, _, _)| d.screen == screen && d.name.is_empty())
                .map(|(d, _, _)| pascal(last(&d.path)))
        };
        let mut views = vec![];
        for (d, _, commands) in &c.0 {
            let base = pascal(last(&d.path));
            let holder = own(d.screen).unwrap_or_else(|| base.clone());
            let module = d.path.rsplit_once("::").map_or("lib", |(m, _)| m);
            let (swift, kotlin) = match () {
                _ if *commands => (format!("{part}Commands"), format!("{}{}Commands", part[..1].to_lowercase(), &part[1..])),
                _ if !d.name.is_empty() || d.screen == H::SHELL => (format!("{base}View"), base.clone()),
                _ => (format!("{base}View"), format!("{base}Screen")),
            };
            views.push((
                d.clone(),
                Names {
                    swift,
                    swift_holder: format!("{holder}Screen"),
                    kotlin,
                    kotlin_holder: format!("{holder}Holder"),
                    runtime: format!("{}App", pascal(d.screen)),
                    source: format!("src/{}.rs", module.replace("::", "/")),
                    commands: *commands,
                },
            ));
        }
        // What the shell's views host, and what draws each.
        let screen = |n: &str| {
            c.0.iter()
                .any(|(d, _, _)| d.screen == n && d.name.is_empty() && !d.beside && n != H::SHELL)
        };
        let further = |n: &str| {
            c.0.iter()
                .any(|(d, _, cm)| !cm && !d.name.is_empty() && format!("{}.{}", d.screen, d.name) == n)
        };
        let mut hosted: Vec<(String, String)> = vec![];
        for (d, names, _) in c.0.iter().filter(|(d, _, _)| d.screen == H::SHELL) {
            let _ = d;
            for &n in names {
                let target = match () {
                    _ if screen(n) => n.to_string(),
                    _ if further(n) => n.to_string(),
                    _ if screen(last_dot(n)) => last_dot(n).to_string(),
                    _ => continue,
                };
                if !hosted.iter().any(|(h, _)| h == n) {
                    hosted.push((n.to_string(), target));
                }
            }
        }
        let mut r = Reached::default();
        H::reach(&mut r);
        reach!(&mut r, crate::welcome::WelcomePage);
        reach!(&mut r, crate::welcome::WelcomeMsg);
        reach!(&mut r, crate::welcome::AddDevicePage);
        reach!(&mut r, crate::welcome::AddDeviceMsg);
        reach!(&mut r, crate::shell::Household);
        Manifest {
            app: std::any::type_name::<H>(),
            route: std::any::type_name::<H::Route>(),
            shell: H::SHELL,
            over: H::OVER,
            views,
            hosted,
            twins: r.paths().to_vec(),
        }
    }

    /// The names of the view `name` of `screen` (empty: the screen's own).
    pub fn names(&self, screen: &str, name: &str) -> Option<&Names> {
        self.views.iter().find(|(d, _)| d.screen == screen && d.name == name).map(|(_, n)| n)
    }

    /// As JSON, in the shape ui/prebuild.py reads: app.toml's old
    /// `[screens]`, `[views]`, `[shell]` (with `hosted`) and `[ffi]`'s
    /// `app`, `route` and `twins` (each type's path).
    pub fn json(&self) -> String {
        let mut screens = Obj::default();
        let mut views = Obj::default();
        for (d, n) in &self.views {
            if d.name.is_empty() {
                let mut s = Obj::default();
                s.str("app", in_crate(d.app));
                s.str("runtime", &n.runtime);
                s.str("page", last(d.page));
                s.str("msg", last(d.msg));
                s.str("swift", &n.swift_holder);
                s.str("kotlin", &n.kotlin_holder);
                s.str("view", &d.path);
                s.str("swift-view", &n.swift);
                s.str("kotlin-view", &n.kotlin);
                s.str("source", &n.source);
                if d.beside {
                    s.raw("beside", "true");
                }
                screens.raw(d.screen, &s.end());
            } else {
                let mut v = Obj::default();
                v.str("screen", d.screen);
                v.str(if n.commands { "commands" } else { "view" }, &d.path);
                v.str("swift-view", &n.swift);
                v.str("kotlin-view", &n.kotlin);
                v.str("source", &n.source);
                views.raw(&format!("{}.{}", d.screen, d.name), &v.end());
            }
        }
        let mut out = Obj::default();
        out.raw("screens", &screens.end());
        out.raw("views", &views.end());
        let has = |name: &str| self.views.iter().any(|(d, _)| d.screen == self.shell && d.name == name);
        if self.views.iter().any(|(d, _)| d.screen == self.shell && d.name.is_empty()) {
            let mut sh = Obj::default();
            sh.str("screen", self.shell);
            for k in ["settings", "commands"] {
                if has(k) {
                    sh.str(k, &format!("{}.{k}", self.shell));
                }
            }
            if !self.over.is_empty() {
                sh.str("over", self.over);
            }
            // The shell page's fields and its message, every app's.
            for (k, v) in [
                ("account", "household"),
                ("section", "section"),
                ("appearance", "appearance"),
                ("household", "household"),
                ("browse", "browse"),
                ("closed", "PageClosed"),
            ] {
                sh.str(k, v);
            }
            let mut hosted = Obj::default();
            for (name, target) in &self.hosted {
                hosted.str(name, target);
            }
            sh.raw("hosted", &hosted.end());
            out.raw("shell", &sh.end());
        }
        let mut ffi = Obj::default();
        ffi.str("app", in_crate(self.app));
        ffi.str("route", in_crate(self.route));
        ffi.raw(
            "twins",
            &format!("[{}]", self.twins.iter().map(|t| quoted(t)).collect::<Vec<_>>().join(", ")),
        );
        out.raw("ffi", &ffi.end());
        out.end()
    }
}

/// `accounts.register`'s last part: `register`.
fn last_dot(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// A JSON object, written as it is filled.
#[derive(Default)]
struct Obj(Vec<String>);

impl Obj {
    fn str(&mut self, k: &str, v: &str) {
        self.raw(k, &quoted(v))
    }
    fn raw(&mut self, k: &str, v: &str) {
        self.0.push(format!("{}: {v}", quoted(k)))
    }
    fn end(self) -> String {
        format!("{{{}}}", self.0.join(", "))
    }
}

fn quoted(s: &str) -> String {
    let mut out = String::from('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An app's declaration, at its crate's root: its part of its host, named
/// `App` there (what every generated target runs: `money_app::App`), and
/// each screen it runs over a session — the field of the part that holds
/// its runtime, the runtime's type and its `#[view]` — with its further
/// views. A screen marked `#[shell]` is the shell, `#[over]` goes over every
/// section, and `#[beside]` is one the phone runs for as long as it shows
/// it rather than its host's (no field of the part). A further view named
/// `commands` is the menu bar's (`fn() -> Vec<CommandMenu<Msg>>`).
///
/// ```ignore
/// ark_app::app! {
///     pub type App = host::Money {
///         #[shell]
///         shell: shell::MoneyShell = shell::view::money_shell {
///             settings: shell::view::money_settings,
///             commands: shell::view::commands,
///         },
///         accounts: accounts::Accounts = accounts::view::accounts,
///         #[over]
///         sheets: sheets::Sheets = sheets::view::sheets,
///     }
/// }
/// ```
///
/// It writes `Has<Runtime>` for each screen held by a field, and
/// [`Declared`](crate::manifest::Declared): what the manifest is made of
/// (ui/docs/app.md, "The App's Declaration").
#[macro_export]
macro_rules! app {
    (
        $(#[$doc:meta])*
        $vis:vis type $alias:ident = $host:path {
            $(
                $(#[$mark:ident])?
                $name:ident : $ty:path = $($view:ident)::+
                $({ $($fname:ident : $($fview:ident)::+),* $(,)? })?
            ),* $(,)?
        }
    ) => {
        $(#[$doc])*
        $vis type $alias = $host;

        $( $crate::app!(@has $host, [$($mark)?], $name, $ty); )*

        impl $crate::manifest::Declared for $host {
            const SHELL: &'static str = $crate::manifest::marked(&[$((stringify!($($mark)?), stringify!($name))),*], "shell", "shell");
            const OVER: &'static str = $crate::manifest::marked(&[$((stringify!($($mark)?), stringify!($name))),*], "over", "");

            fn declare<D: $crate::manifest::Declare>(d: &mut D) {
                $(
                    d.view(
                        $crate::app!(@decl [$($mark)?], $name, $ty, "", $($view)::+),
                        $($view)::+::record,
                    );
                    $($( $crate::app!(@further d, $name, $ty, $fname, $($fview)::+); )*)?
                )*
            }

            fn reach(r: &mut $crate::manifest::Reached) {
                $crate::manifest::reach!(r, <$host as $crate::host::HostedApp>::Route);
                $crate::manifest::reach!(r, <$host as $crate::host::HostedApp>::Request);
                $(
                    $crate::manifest::reach!(r, <$ty as $crate::App>::Screen);
                    $crate::manifest::reach!(r, <$ty as $crate::App>::Msg);
                )*
            }
        }
    };
    (@has $host:path, [beside], $name:ident, $ty:path) => {};
    (@has $host:path, [$($mark:ident)?], $name:ident, $ty:path) => {
        impl $crate::host::Has<$ty> for $host {
            fn screen(&self) -> &$crate::host::Screen<$ty> {
                &self.$name
            }
            fn screen_mut(&mut self) -> &mut $crate::host::Screen<$ty> {
                &mut self.$name
            }
        }
    };
    (@decl [$($mark:ident)?], $name:ident, $ty:path, $further:expr, $($view:ident)::+) => {
        $crate::manifest::Decl {
            screen: stringify!($name),
            name: $further,
            app: ::std::any::type_name::<$ty>(),
            page: ::std::any::type_name::<<$ty as $crate::App>::Screen>(),
            msg: ::std::any::type_name::<<$ty as $crate::App>::Msg>(),
            path: stringify!($($view)::+).replace(' ', ""),
            beside: $crate::manifest::marked(&[(stringify!($($mark)?), "beside")], "beside", "") == "beside",
        }
    };
    (@further $d:ident, $name:ident, $ty:path, commands, $($fview:ident)::+) => {
        $d.commands($crate::app!(@decl [], $name, $ty, "commands", $($fview)::+), $($fview)::+);
    };
    (@further $d:ident, $name:ident, $ty:path, $fname:ident, $($fview:ident)::+) => {
        $d.view($crate::app!(@decl [], $name, $ty, stringify!($fname), $($fview)::+), $($fview)::+::record);
    };
}
