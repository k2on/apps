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
/// when compact. `tabs` are [`section_tab`]s; `selected` is the tag of the
/// one shown, and choosing another sends `select` with its tag.
pub fn adaptive_tabs<M: 'static>(selected: impl Into<Val<String>>, select: fn(String) -> M, tabs: impl Into<View<M>>) -> View<M> {
    Part::Tabs {
        selected: selected.into(),
        select: Input::new(select),
        tabs: Box::new(tabs.into()),
    }
    .into()
}

/// One of the app's sections: its title, its glyph, its tag, what it shows.
pub fn section_tab<M>(title: &'static str, glyph: &'static str, tag: &'static str, content: impl Into<View<M>>) -> SectionTab<M> {
    SectionTab {
        title,
        glyph,
        tag,
        badge: None,
        content: content.into(),
    }
}

pub struct SectionTab<M> {
    title: &'static str,
    glyph: &'static str,
    tag: &'static str,
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

/// The Settings scene's panes ([`settings_pane`]s).
pub fn settings_panes<M>(panes: impl Into<View<M>>) -> View<M> {
    Part::SettingsPanes {
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
