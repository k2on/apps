//! The shell every Ark app is made of, as a standard Mac app is
//! (ui/docs/design.md): its sections ([`adaptive_tabs`]), a list beside
//! what it opens ([`split_view`]), a toolbar by placement ([`toolbar`]),
//! an inspector ([`inspector`]), search ([`searchable`]), a question and a
//! notice ([`confirmation_dialog`], [`alert`]), the Settings scene
//! ([`settings_panes`]) and the menu bar's commands ([`CommandMenu`]).
//!
//! Each is a part ([`Part`]): said once in a `#[view]`, written out as
//! SwiftUI and Compose by `ark-ui-gen`, drawn by iced (`arkui::view`), and
//! the browser's when it has a DOM client. Each adapts to the size class
//! inside itself — a sidebar or a tab bar, columns or a stack, a pane or a
//! sheet — from the widths `ui/theme.toml`'s `[layout]` names, so an app
//! never asks how wide it is.
//!
//! ```ignore
//! #[view]
//! pub fn shell(p: &ShellPage) -> View<ShellMsg> {
//!     adaptive_tabs(p.section, |tag| ShellMsg::Select { tag }, (
//!         section_tab("Budget", "wallet", "budget", toolbar("Budget", hosted("budget"), (
//!             bar_item("Assign", "plus", Placement::Primary, ShellMsg::Assign),
//!         ))),
//!         section_tab("Accounts", "layers", "accounts", split_view(
//!             hosted("accounts"), hosted("register"), p.showing_register, ShellMsg::Back,
//!         ))
//!         .badge(p.to_review),
//!     ))
//! }
//! ```

use crate::view::{DialogAction, Input, Part, Placement, Val, View};

/// The app's sections: a sidebar on a regular width and the Mac, a tab bar
/// when compact. `tabs` are [`section_tab`]s and [`tab_group`]s; `selected`
/// is the tag of the one shown, and choosing another sends `select` with
/// its tag. [`Tabs::footer`] is the sidebar's foot.
pub fn adaptive_tabs<M: 'static>(selected: impl Into<Val<String>>, select: fn(String) -> M, tabs: impl Into<View<M>>) -> Tabs<M> {
    Tabs {
        selected: selected.into(),
        select: Input::new(select),
        tabs: tabs.into(),
        footer: View::Empty,
        account: View::Empty,
    }
}

pub struct Tabs<M> {
    selected: Val<String>,
    select: Input<String, M>,
    tabs: View<M>,
    footer: View<M>,
    account: View<M>,
}

impl<M> Tabs<M> {
    /// What sits at the sidebar's foot: who is signed in, the household
    /// (`.tabViewSidebarBottomBar`). Never on a compact tab bar.
    pub fn footer(mut self, footer: impl Into<View<M>>) -> Self {
        self.footer = footer.into();
        self
    }

    /// Who is signed in ([`account_item`]): the last item of every
    /// section's toolbar, on every platform and at every width.
    pub fn account(mut self, account: impl Into<View<M>>) -> Self {
        self.account = account.into();
        self
    }
}

impl<M> From<Tabs<M>> for View<M> {
    fn from(t: Tabs<M>) -> View<M> {
        Part::Tabs {
            selected: t.selected,
            select: t.select,
            tabs: Box::new(t.tabs),
            footer: Box::new(t.footer),
            account: Box::new(t.account),
        }
        .into()
    }
}

/// One of the app's sections: its title, its glyph, its tag, what it shows.
/// A tag `parent/child` (`accounts/<id>`) is a child of the section
/// `parent`: a place of its own in the sidebar, and when compact the
/// parent's tab, lit, with the child pushed in it.
pub fn section_tab<M>(
    title: impl Into<Val<String>>,
    glyph: impl Into<Val<String>>,
    tag: impl Into<Val<String>>,
    content: impl Into<View<M>>,
) -> SectionTab<M> {
    SectionTab {
        title: title.into(),
        glyph: glyph.into(),
        tag: tag.into(),
        badge: None,
        content: content.into(),
    }
}

pub struct SectionTab<M> {
    title: Val<String>,
    glyph: Val<String>,
    tag: Val<String>,
    badge: Option<Val<Option<i64>>>,
    content: View<M>,
}

impl<M> SectionTab<M> {
    /// A count at its end, while there is one: what waits there.
    pub fn badge(mut self, count: impl Into<Val<Option<i64>>>) -> Self {
        self.badge = Some(count.into());
        self
    }
}

impl<M> From<SectionTab<M>> for View<M> {
    fn from(t: SectionTab<M>) -> View<M> {
        Part::SectionTab {
            title: t.title,
            glyph: t.glyph,
            tag: t.tag,
            badge: t.badge,
            content: Box::new(t.content),
        }
        .into()
    }
}

/// Sections under a heading, in the sidebar alone (`TabSection`, hidden
/// from a compact tab bar): an app's own places, as Mail's mailboxes are.
pub fn tab_group<M>(title: &'static str, tabs: impl Into<View<M>>) -> View<M> {
    Part::TabGroup {
        title,
        tabs: Box::new(tabs.into()),
        place: None,
    }
    .into()
}

/// Sections under a heading that is a place of its own, tagged `tag`:
/// `index` is what it shows (its sections, a row each). In the sidebar,
/// the heading over its sections, as [`tab_group`]'s; short of one, a tab
/// of the tab bar with `glyph`, showing `index`, and a section of it
/// pushed over the index with the way back to it — Settings' panes on an
/// iPhone, the docs' sections on a phone.
pub fn tab_section<M>(title: &'static str, glyph: &'static str, tag: &'static str, index: impl Into<View<M>>, tabs: impl Into<View<M>>) -> View<M> {
    Part::TabGroup {
        title,
        tabs: Box::new(tabs.into()),
        place: Some((glyph, tag, Box::new(index.into()))),
    }
    .into()
}

/// What `content` opens, pushed onto the section's stack while `showing`,
/// at every width, `back` to leave it (a Back button, the edge swipe).
pub fn pushes<M>(content: impl Into<View<M>>, detail: impl Into<View<M>>, showing: impl Into<Val<bool>>, back: M) -> View<M> {
    Part::Push {
        content: Box::new(content.into()),
        detail: Box::new(detail.into()),
        showing: showing.into(),
        back,
    }
    .into()
}

/// The section a tag is shown in when compact: a child's parent
/// (`accounts/<id>` is `accounts`), any other tag itself.
pub fn compact_tag(tag: &str) -> &str {
    tag.split_once('/').map_or(tag, |(parent, _)| parent)
}

/// A list beside what it opens: columns on a regular width, a stack when
/// compact, the detail pushed while `showing` and `back` to leave it.
pub fn split_view<M>(content: impl Into<View<M>>, detail: impl Into<View<M>>, showing: impl Into<Val<bool>>, back: M) -> View<M> {
    Part::Split {
        content: Box::new(content.into()),
        detail: Box::new(detail.into()),
        showing: showing.into(),
        back,
    }
    .into()
}

/// A section's title and its toolbar's items ([`bar_item`]s), over its
/// content.
pub fn toolbar<M>(title: impl Into<Val<String>>, content: impl Into<View<M>>, items: impl Into<View<M>>) -> View<M> {
    Part::Toolbar {
        title: title.into(),
        items: Box::new(items.into()),
        content: Box::new(content.into()),
    }
    .into()
}

/// Who is signed in, as the toolbar's last item ([`Tabs::account`]): their
/// picture at `picture` (a whole URL; empty, their initial on a tinted
/// circle), said as "Account" and `name`; `tap` opens their account.
pub fn account_item<M>(name: impl Into<Val<String>>, picture: impl Into<Val<String>>, tap: M) -> View<M> {
    Part::Account {
        name: name.into(),
        picture: picture.into(),
        tap,
    }
    .into()
}

/// What a screen reader hears of the account item: "Account" and the name.
pub fn account_label(name: &str) -> String {
    match name.is_empty() {
        true => "Account".into(),
        false => format!("Account, {name}"),
    }
}

/// The initial the account item shows with no picture: the name's first
/// letter, capitalized; `?` for none.
pub fn account_initial(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}

/// A toolbar's button, where `placement` puts it.
pub fn bar_item<M>(title: &'static str, glyph: &'static str, placement: Placement, tap: M) -> BarItem<M> {
    BarItem {
        title,
        glyph,
        placement,
        enabled: Val::Is(true),
        tap,
    }
}

pub struct BarItem<M> {
    title: &'static str,
    glyph: &'static str,
    placement: Placement,
    enabled: Val<bool>,
    tap: M,
}

impl<M> BarItem<M> {
    /// Pressable only while `on` holds.
    pub fn enabled(mut self, on: impl Into<Val<bool>>) -> Self {
        self.enabled = on.into();
        self
    }
}

impl<M> From<BarItem<M>> for View<M> {
    fn from(b: BarItem<M>) -> View<M> {
        Part::BarItem {
            title: b.title,
            glyph: b.glyph,
            placement: b.placement,
            enabled: b.enabled,
            tap: b.tap,
        }
        .into()
    }
}

/// `content`, searchable: `text` is what is typed, and each change sends
/// `typed`.
pub fn searchable<M: 'static>(prompt: &'static str, text: impl Into<Val<String>>, typed: fn(String) -> M, content: impl Into<View<M>>) -> View<M> {
    Part::Searchable {
        prompt,
        text: text.into(),
        typed: Input::new(typed),
        content: Box::new(content.into()),
    }
    .into()
}

/// `content` with an inspector at its trailing edge (a sheet when
/// compact), shown while `shown`; shown or put away, `toggle`.
pub fn inspector<M: 'static>(
    title: &'static str,
    shown: impl Into<Val<bool>>,
    toggle: fn(bool) -> M,
    content: impl Into<View<M>>,
    inspector: impl Into<View<M>>,
) -> View<M> {
    Part::Inspector {
        title,
        shown: shown.into(),
        toggle: Input::new(toggle),
        content: Box::new(content.into()),
        inspector: Box::new(inspector.into()),
    }
    .into()
}

/// A screen each platform's shell draws itself, named: a hand-written
/// screen until it is moved into Rust, or a view generated apart (money's
/// Accounts).
pub fn hosted<M>(name: &'static str) -> View<M> {
    Part::Hosted { name }.into()
}

/// A question before something that cannot be undone: its [`Confirming::action`]s,
/// then Cancel, which sends `cancel`.
pub fn confirmation_dialog<M>(title: impl Into<Val<String>>, message: impl Into<Val<String>>, cancel: M) -> Confirming<M> {
    Confirming {
        title: title.into(),
        message: message.into(),
        actions: vec![],
        cancel,
    }
}

pub struct Confirming<M> {
    title: Val<String>,
    message: Val<String>,
    actions: Vec<DialogAction<M>>,
    cancel: M,
}

impl<M> Confirming<M> {
    /// An action, in the error's red when `destructive`.
    pub fn action(mut self, title: impl Into<Val<String>>, destructive: bool, tap: M) -> Self {
        self.actions.push(DialogAction {
            title: title.into(),
            destructive,
            tap,
        });
        self
    }
}

impl<M> From<Confirming<M>> for View<M> {
    fn from(c: Confirming<M>) -> View<M> {
        Part::Confirm {
            title: c.title,
            message: c.message,
            actions: c.actions,
            cancel: c.cancel,
        }
        .into()
    }
}

/// Something the person has to know, and OK.
pub fn alert<M>(title: impl Into<Val<String>>, message: impl Into<Val<String>>, ok: M) -> View<M> {
    Part::Alert {
        title: title.into(),
        message: message.into(),
        ok,
    }
    .into()
}

/// The Settings scene's panes ([`settings_pane`]s): one shown at a time,
/// `selected` by its title, choosing another sends `select` with its.
pub fn settings_panes<M: 'static>(selected: impl Into<Val<String>>, select: fn(String) -> M, panes: impl Into<View<M>>) -> View<M> {
    Part::SettingsPanes {
        selected: selected.into(),
        select: Input::new(select),
        panes: Box::new(panes.into()),
    }
    .into()
}

/// One pane of Settings.
pub fn settings_pane<M>(title: &'static str, glyph: &'static str, content: impl Into<View<M>>) -> View<M> {
    Part::SettingsPane {
        title,
        glyph,
        content: Box::new(content.into()),
    }
    .into()
}

// * Commands

/// Where a menu of commands goes in the menu bar (and, off the Mac, in the
/// command box and the keys panel, under the same heading).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuPlace {
    /// File, after New.
    File,
    /// View, after the sidebar's own.
    View,
    /// A menu of its own, Go: the sections.
    Go,
}

impl MenuPlace {
    /// As the parts' enum spells it in Swift and Kotlin (`MenuPlace`).
    pub fn case(self) -> &'static str {
        match self {
            MenuPlace::File => "file",
            MenuPlace::View => "view",
            MenuPlace::Go => "go",
        }
    }

    /// Its heading in the menu bar and the command box.
    pub fn title(self) -> &'static str {
        match self {
            MenuPlace::File => "File",
            MenuPlace::View => "View",
            MenuPlace::Go => "Go",
        }
    }
}

/// A key with Command (Control off Apple's platforms), and Shift or Option
/// with it when they say so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    /// What the key types: `n`, `[`, `1`.
    pub key: &'static str,
    pub shift: bool,
    pub option: bool,
}

impl Shortcut {
    /// As the Mac's menu writes it: `⇧⌘N`.
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.option {
            s.push('⌥');
        }
        if self.shift {
            s.push('⇧');
        }
        s.push('⌘');
        s.push_str(&self.key.to_uppercase());
        s
    }
}

/// One command: its title, its shortcut, what it sends.
#[derive(Clone, Debug)]
pub struct CommandItem<M> {
    pub title: &'static str,
    pub shortcut: Option<Shortcut>,
    pub run: M,
}

/// A command with ⌘`key`.
pub fn command<M>(title: &'static str, key: &'static str, run: M) -> CommandItem<M> {
    CommandItem {
        title,
        shortcut: Some(Shortcut {
            key,
            shift: false,
            option: false,
        }),
        run,
    }
}

/// A command with no shortcut, found by name.
pub fn command_unbound<M>(title: &'static str, run: M) -> CommandItem<M> {
    CommandItem { title, shortcut: None, run }
}

impl<M> CommandItem<M> {
    /// With Shift too.
    pub fn shift(mut self) -> Self {
        if let Some(s) = &mut self.shortcut {
            s.shift = true;
        }
        self
    }

    /// With Option too.
    pub fn option(mut self) -> Self {
        if let Some(s) = &mut self.shortcut {
            s.option = true;
        }
        self
    }
}

/// A menu of the menu bar: where it goes, and its commands in order.
#[derive(Clone, Debug)]
pub struct CommandMenu<M> {
    pub place: MenuPlace,
    pub items: Vec<CommandItem<M>>,
}

pub fn command_menu<M>(place: MenuPlace, items: Vec<CommandItem<M>>) -> CommandMenu<M> {
    CommandMenu { place, items }
}

/// The command `key` with these modifiers runs, among `menus`.
pub fn command_for<'a, M>(menus: &'a [CommandMenu<M>], key: &str, shift: bool, option: bool) -> Option<&'a CommandItem<M>> {
    menus.iter().flat_map(|m| &m.items).find(|c| {
        c.shortcut
            .is_some_and(|s| s.key.eq_ignore_ascii_case(key) && s.shift == shift && s.option == option)
    })
}

/// Where a window opens, as its launch arguments say (every Ark app's, so
/// a screenshot or a test opens any screen with no steps): `-screen
/// <place>` (`budget`, `account:checking`, `transaction:receipt`, `settings`
/// …, the app's own words), `-inspector 1`, `-appearance light|dark`,
/// `-pane <title or word>`. Read as `-key value` pairs, as the Mac's and the
/// iPhone's argument domain reads them (`--key` too, the desktop's way);
/// anything else in the arguments is left to whoever asked for it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Launch {
    /// The place, its word before the colon (`account`), lower case.
    pub screen: Option<String>,
    /// What follows the colon (`checking`), as written.
    pub item: Option<String>,
    pub inspector: Option<bool>,
    pub appearance: Option<String>,
    pub pane: Option<String>,
}

impl Launch {
    /// The launch arguments of `args` (a process's, the program's own name
    /// first or not).
    pub fn parse<S: AsRef<str>>(args: &[S]) -> Launch {
        let mut l = Launch::default();
        let mut i = 0;
        while i < args.len() {
            let key = args[i].as_ref().trim_start_matches('-');
            let flag = args[i].as_ref().starts_with('-');
            let value = args.get(i + 1).map(|v| v.as_ref().to_string());
            let taken = flag && value.is_some();
            match (flag, key, value) {
                (true, "screen", Some(v)) => match v.split_once(':') {
                    Some((place, item)) => {
                        l.screen = Some(place.to_lowercase());
                        l.item = Some(item.trim_matches('"').to_string());
                    }
                    None => l.screen = Some(v.to_lowercase()),
                },
                (true, "inspector", Some(v)) => l.inspector = Some(matches!(v.as_str(), "1" | "true" | "YES" | "yes")),
                (true, "appearance", Some(v)) => l.appearance = Some(v.to_lowercase()),
                (true, "pane" | "settingsTab", Some(v)) => l.pane = Some(v),
                _ => {
                    i += 1;
                    continue;
                }
            }
            i += if taken { 2 } else { 1 };
        }
        l
    }

    /// Whether anything was asked for.
    pub fn is_empty(&self) -> bool {
        *self == Launch::default()
    }

    /// Where an address opens (ui/docs/app.md, Address: the path names the
    /// content, the query holds the UI's state), as a launch: what a
    /// screenshot's `route` or a link names. The path is a section
    /// (`/budget`, `/accounts`, `/transactions`, `/insights`, `/settings`),
    /// `/inbox` (To Review), or a row by its id or a word of its name
    /// (`/account/sapphire`, `/category/dining`, `/transaction/receipt`;
    /// a route's own `/account/<id>/<slug>` reads its id). The query is
    /// `inspector=1`, `appearance=dark`, `pane=banks` (Settings'), and
    /// `sheet=new-transaction` (the form over Transactions). A scheme and
    /// a host before the path (`money://account/x`) are passed over.
    pub fn address(address: &str) -> Launch {
        let rest = address.split_once("://").map_or(address, |(_, r)| r);
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        let mut l = Launch::default();
        match parts.as_slice() {
            [] => {}
            ["inbox", ..] => l.screen = Some("review".into()),
            [place] => l.screen = Some(place.to_lowercase()),
            [place, item, ..] => {
                l.screen = Some(place.to_lowercase());
                l.item = Some(unescape(item));
            }
        }
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = unescape(v);
            match k {
                "inspector" => l.inspector = Some(matches!(v.as_str(), "1" | "true" | "yes")),
                "appearance" => l.appearance = Some(v.to_lowercase()),
                "pane" => l.pane = Some(v),
                "sheet" if v == "new-transaction" => l.screen = Some("add-transaction".into()),
                _ => {}
            }
        }
        l
    }
}

/// An address's escapes (`%20`, `+`) undone.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
