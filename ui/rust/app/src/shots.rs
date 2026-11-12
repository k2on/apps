//! A screenshot's steps, taken inside the app (ui/docs/shots.md, "In the
//! App"): the platform's shots mode hands each step of a shot here, and a
//! step reaches what it names in the views on the screen now, the ArkUI
//! views every toolkit draws, and sends the message that element's action
//! sends — the message a finger's tap on it sends, by the same runtime's
//! `send`. Nothing is drawn, touched or looked for on a screen: the views
//! are the screen.
//!
//! - [`nodes`]: what of one runtime's view a step can reach, each element
//!   by its label (what it says) and its [`Role`], and where its view hosts
//!   another runtime's ([`Part::Hosted`]); only what shows (a tab bar's
//!   selected tab, a stack's pushed page while it is pushed, a sheet while
//!   it is up).
//! - [`compose`]: every runtime's, put together as the screen nests them:
//!   a hosted runtime in its place, and while a sheet, a dialog or an alert
//!   is up, only what is in it (a finger reaches nothing under it).
//! - [`take`]: one [`Step`] taken against them.

use std::rc::Rc;

use crate::view::{Input, Layout, Part, Prim, View};

/// What kind of element a label names, for a step that says which of
/// several saying the same it means (a tab and a heading both "Budget").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A section of the app: a tab bar's tab, a sidebar's row.
    Tab,
    /// A button: a toolbar's, a page's, a sheet's, a dialog's.
    Button,
    /// A field that takes text.
    Field,
    /// A list's or a table's row.
    Row,
    /// One of a picker's or a menu's choices.
    Item,
    /// Words alone: a title, a heading. Nothing to tap; there to wait for.
    Text,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Tab => "tab",
            Role::Button => "button",
            Role::Field => "field",
            Role::Row => "row",
            Role::Item => "item",
            Role::Text => "text",
        }
    }

    pub fn of(name: &str) -> Option<Role> {
        [Role::Tab, Role::Button, Role::Field, Role::Row, Role::Item, Role::Text]
            .into_iter()
            .find(|r| r.name() == name)
    }
}

/// An element a step can reach: its label and role, and what it does,
/// each a message sent to the runtime whose view it is in.
pub struct Target {
    pub label: String,
    pub role: Role,
    /// The modal it is in (a sheet, a dialog, an alert), counted in the
    /// order they are met; none: 0.
    pub modal: u32,
    tap: Option<Rc<dyn Fn()>>,
    typed: Option<Rc<dyn Fn(String)>>,
    menu: Vec<(String, Rc<dyn Fn()>)>,
}

impl Target {
    /// Whether a tap does anything.
    pub fn taps(&self) -> bool {
        self.tap.is_some()
    }
}

/// What one runtime's view holds that a step can reach.
pub enum Node {
    Target(Target),
    /// Where the view hosts the screen `name` (`hosted("budget")`); `shown`
    /// unless it is in what is not on the screen (a tab not selected).
    Hosted {
        name: String,
        shown: bool,
    },
}

/// What of `view` a step can reach, each element's action sent with
/// `send` (the runtime's own `send`).
pub fn nodes<M: Clone + 'static>(view: &View<M>, send: Rc<dyn Fn(M)>) -> Vec<Node> {
    let mut w = Walk {
        send,
        out: vec![],
        modals: 0,
    };
    w.view(view, true, 0);
    w.out
}

struct Walk<M> {
    send: Rc<dyn Fn(M)>,
    out: Vec<Node>,
    modals: u32,
}

impl<M: Clone + 'static> Walk<M> {
    fn act(&self, m: &M) -> Rc<dyn Fn()> {
        let (send, m) = (self.send.clone(), m.clone());
        Rc::new(move || send(m.clone()))
    }

    fn input(&self, i: &Input<String, M>) -> Rc<dyn Fn(String)> {
        let (send, i) = (self.send.clone(), i.clone());
        Rc::new(move |t| send(i.call(t)))
    }

    /// An element, where `at` is whether it shows and the modal it is in.
    fn push(
        &mut self,
        at: (bool, u32),
        label: String,
        role: Role,
        tap: Option<&M>,
        typed: Option<&Input<String, M>>,
        menu: &[crate::view::MenuItem<M>],
    ) {
        let (shown, modal) = at;
        if !shown || label.is_empty() {
            return;
        }
        let tap = tap.map(|m| self.act(m));
        let typed = typed.map(|i| self.input(i));
        let menu = menu
            .iter()
            .filter(|i| i.shown.as_ref().is_none_or(|s| s.get()))
            .map(|i| (i.title.get(), self.act(&i.tap)))
            .collect();
        self.out.push(Node::Target(Target {
            label,
            role,
            modal,
            tap,
            typed,
            menu,
        }));
    }

    fn text(&mut self, shown: bool, modal: u32, label: String) {
        self.push((shown, modal), label, Role::Text, None, None, &[]);
    }

    fn modal(&mut self) -> u32 {
        self.modals += 1;
        self.modals
    }

    fn view(&mut self, v: &View<M>, shown: bool, modal: u32) {
        match v {
            View::Empty => {}
            View::List(vs) => vs.iter().for_each(|v| self.view(v, shown, modal)),
            // Recorded views are not drawn; a real one has none of these.
            View::Each { .. } | View::When { .. } | View::WhenSome { .. } => {}
            View::Part(p) => self.part(p, shown, modal),
            View::Layout(l) => self.layout(l, shown, modal),
        }
    }

    fn layout(&mut self, l: &Layout<M>, shown: bool, modal: u32) {
        match &l.prim {
            Prim::Stack { content, .. } | Prim::Scroll { content, .. } => self.view(content, shown, modal),
            Prim::Text(w) => self.text(shown, modal, w.text.get()),
            Prim::Spacer { .. } | Prim::Glyph { .. } | Prim::Image { .. } => {}
        }
    }

    fn part(&mut self, p: &Part<M>, shown: bool, modal: u32) {
        use Part as P;
        match p {
            P::Page { title, top, content, .. } => {
                self.text(shown, modal, title.get());
                self.view(top, shown, modal);
                self.view(content, shown, modal);
            }
            P::Detail { content } | P::Chips(content) => self.view(content, shown, modal),
            P::Section(s) => {
                let title = s.title.as_ref().map(|t| t.get()).unwrap_or_default();
                match &s.open {
                    Some((_, toggle)) => self.push((shown, modal), title, Role::Button, Some(toggle), None, &s.menu),
                    None => self.push((shown, modal), title, Role::Text, None, None, &s.menu),
                }
                let open = s.open.as_ref().is_none_or(|(o, _)| o.get());
                self.view(&s.rows, shown && open, modal);
            }
            P::Row(r) => {
                let label = r
                    .said
                    .as_ref()
                    .map(|s| s.get())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| r.title.get());
                self.push((shown, modal), label, Role::Row, r.tap.as_ref(), None, &r.menu);
            }
            P::MeterRow(r) => self.push((shown, modal), r.title.get(), Role::Row, r.tap.as_ref(), None, &r.menu),
            P::TableRow(r) => {
                let label = r
                    .said
                    .as_ref()
                    .map(|s| s.get())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| r.cells.get().into_iter().find(|c| !c.is_empty()).unwrap_or_default());
                self.push((shown, modal), label, Role::Row, r.tap.as_ref(), None, &r.menu);
            }
            P::Headline(h) => {
                self.text(shown, modal, h.label.get());
                if let Some((title, _, m)) = &h.action {
                    self.push((shown, modal), title.to_string(), Role::Button, Some(m), None, &[]);
                }
            }
            P::Sheet {
                title,
                dismiss,
                confirm,
                content,
            } => {
                let m = self.modal();
                self.text(shown, m, title.to_string());
                self.push((shown, m), "Cancel".into(), Role::Button, Some(dismiss), None, &[]);
                if let Some(c) = confirm.as_ref().filter(|c| c.enabled.get()) {
                    self.push((shown, m), c.title.into(), Role::Button, Some(&c.tap), None, &[]);
                }
                self.view(content, shown, m);
            }
            P::FormSection { header, content, .. } => {
                if let Some(h) = header {
                    self.text(shown, modal, h.get());
                }
                self.view(content, shown, modal);
            }
            P::Picker { label, pick, choices, .. } => {
                self.text(shown, modal, label.to_string());
                let mut found = vec![];
                collect_choices(choices, &mut found);
                for (title, tag) in found {
                    let pick = pick.clone();
                    let send = self.send.clone();
                    if shown && !title.is_empty() {
                        self.out.push(Node::Target(Target {
                            label: title,
                            role: Role::Item,
                            modal,
                            tap: Some(Rc::new(move || send(pick.call(tag.clone())))),
                            typed: None,
                            menu: vec![],
                        }));
                    }
                }
            }
            P::AmountEntry { label, on_typed, .. } => self.push((shown, modal), label.get(), Role::Field, None, Some(on_typed), &[]),
            P::AmountField { label, on, .. } => self.push((shown, modal), label.get(), Role::Field, None, Some(on), &[]),
            P::NameField { label, typed, .. } => self.push((shown, modal), label.to_string(), Role::Field, None, Some(typed), &[]),
            P::DateField { label, typed, .. } => self.push((shown, modal), label.to_string(), Role::Field, None, Some(typed), &[]),
            P::Prompt {
                title,
                typed,
                confirm,
                ok,
                cancel,
                ..
            } => {
                let m = self.modal();
                self.push((shown, m), title.to_string(), Role::Field, None, Some(typed), &[]);
                self.push((shown, m), confirm.to_string(), Role::Button, Some(ok), None, &[]);
                self.push((shown, m), "Cancel".into(), Role::Button, Some(cancel), None, &[]);
            }
            P::Chip { label, tap, .. } => self.push((shown, modal), label.get(), Role::Button, Some(tap), None, &[]),
            P::Toggle { title, on, change, .. } => {
                let to = change.call(!on.get());
                self.push((shown, modal), title.to_string(), Role::Button, Some(&to), None, &[]);
            }
            P::Button { title, enabled, tap } => {
                let tap = enabled.get().then_some(tap);
                self.push((shown, modal), title.to_string(), Role::Button, tap, None, &[]);
            }
            P::Table { rows, .. } => self.view(rows, shown, modal),
            P::TableHeading { title } => self.text(shown, modal, title.get()),
            P::FirstRunPage { title, content, .. } => {
                self.text(shown, modal, title.get());
                self.view(content, shown, modal);
            }
            P::AddressField { label, typed, .. } => self.push((shown, modal), label.to_string(), Role::Field, None, Some(typed), &[]),
            P::WideButton {
                title, enabled, busy, tap, ..
            } => {
                let tap = (enabled.get() && !busy.get()).then_some(tap);
                self.push((shown, modal), title.get(), Role::Button, tap, None, &[]);
            }
            P::Tabs {
                selected,
                select,
                tabs,
                footer,
                account,
            } => {
                self.tabs(tabs, &selected.get(), select, shown, modal);
                self.view(footer, shown, modal);
                self.view(account, shown, modal);
            }
            // Met only under `Tabs` ([`Walk::tabs`]); alone, its content.
            P::SectionTab { content, .. } => self.view(content, shown, modal),
            P::TabGroup { tabs, .. } => self.view(tabs, shown, modal),
            P::Push {
                content,
                detail,
                showing,
                back,
            }
            | P::Split {
                content,
                detail,
                showing,
                back,
            } => {
                let showing = showing.get();
                self.view(content, shown, modal);
                if showing {
                    self.push((shown, modal), "Back".into(), Role::Button, Some(back), None, &[]);
                }
                self.view(detail, shown && showing, modal);
            }
            P::Toolbar { title, items, content } => {
                self.text(shown, modal, title.get());
                self.view(items, shown, modal);
                self.view(content, shown, modal);
            }
            P::Account { tap, .. } => self.push((shown, modal), "Account".into(), Role::Button, Some(tap), None, &[]),
            P::BarItem { title, enabled, tap, .. } => {
                let tap = enabled.get().then_some(tap);
                self.push((shown, modal), title.to_string(), Role::Button, tap, None, &[]);
            }
            P::Searchable { prompt, typed, content, .. } => {
                self.push((shown, modal), prompt.to_string(), Role::Field, None, Some(typed), &[]);
                self.view(content, shown, modal);
            }
            P::Inspector {
                title,
                shown: open,
                toggle,
                content,
                inspector,
            } => {
                let open = open.get();
                // Its toggle: the toolbar's button, as the sheet's own
                // dismissal sends it on the iPhone.
                let to = toggle.call(!open);
                self.push((shown, modal), title.to_string(), Role::Button, Some(&to), None, &[]);
                self.view(content, shown, modal);
                self.view(inspector, shown && open, modal);
            }
            P::Hosted { name } => self.out.push(Node::Hosted {
                name: name.to_string(),
                shown,
            }),
            P::Confirm { title, actions, cancel, .. } => {
                let m = self.modal();
                self.text(shown, m, title.get());
                for a in actions {
                    self.push((shown, m), a.title.get(), Role::Button, Some(&a.tap), None, &[]);
                }
                self.push((shown, m), "Cancel".into(), Role::Button, Some(cancel), None, &[]);
            }
            P::Alert { title, ok, .. } => {
                let m = self.modal();
                self.text(shown, m, title.get());
                self.push((shown, m), "OK".into(), Role::Button, Some(ok), None, &[]);
            }
            P::SettingsPanes { select, panes, .. } => {
                let mut found = vec![];
                collect_panes(panes, &mut found);
                for (title, content) in found {
                    let to = select.call(title.to_string());
                    self.push((shown, modal), title.to_string(), Role::Row, Some(&to), None, &[]);
                    self.view(content, shown, modal);
                }
            }
            P::SettingsPane { content, .. } => self.view(content, shown, modal),
            P::Hero { label, .. } | P::Summary { label, .. } => self.text(shown, modal, label.to_string()),
            P::Tile { .. }
            | P::StatusBadge { .. }
            | P::Meter { .. }
            | P::Choice { .. }
            | P::BarChart { .. }
            | P::StatusLine { .. }
            | P::CodeImage { .. }
            | P::Photo { .. }
            | P::Paper { .. }
            | P::Writing { .. } => {}
        }
    }

    /// A tab bar's (or a sidebar's) sections: each a tab; only the selected
    /// one's content shows, the others' hosted screens not.
    fn tabs(&mut self, v: &View<M>, selected: &str, select: &Input<String, M>, shown: bool, modal: u32) {
        match v {
            View::List(vs) => vs.iter().for_each(|v| self.tabs(v, selected, select, shown, modal)),
            View::Part(Part::TabGroup { tabs, place, .. }) => {
                if let Some((title, _, index)) = place {
                    self.text(shown, modal, title.to_string());
                    self.view(index, shown, modal);
                }
                self.tabs(tabs, selected, select, shown, modal)
            }
            View::Part(Part::SectionTab { title, tag, content, .. }) => {
                let tag = tag.get();
                let to = select.call(tag.clone());
                self.push((shown, modal), title.get(), Role::Tab, Some(&to), None, &[]);
                // A child section (`parent/child`) shows in its parent's place too.
                let on = selected == tag || selected.split('/').next() == Some(tag.as_str()) && !tag.contains('/');
                self.view(content, shown && on, modal);
            }
            other => self.view(other, shown, modal),
        }
    }
}

/// A picker's choices: each its title and the tag choosing it sends.
fn collect_choices<M>(v: &View<M>, out: &mut Vec<(String, String)>) {
    match v {
        View::List(vs) => vs.iter().for_each(|v| collect_choices(v, out)),
        View::Part(Part::Choice { title, tag, .. }) => out.push((title.get(), tag.get())),
        _ => {}
    }
}

/// The Settings scene's panes: each its title and what it shows.
fn collect_panes<'a, M>(v: &'a View<M>, out: &mut Vec<(&'static str, &'a View<M>)>) {
    match v {
        View::List(vs) => vs.iter().for_each(|v| collect_panes(v, out)),
        View::Part(Part::SettingsPane { title, content, .. }) => out.push((title, content)),
        _ => {}
    }
}

/// One runtime's nodes, under its name (app.toml's `[screens]` key, or a
/// screen run beside the app's, `register`).
pub struct Screen {
    pub name: String,
    pub nodes: Vec<Node>,
}

/// What a step can reach on the screen now, of every runtime's view: the
/// screens no other hosts, in order, each hosted one in its place (a
/// hosted name `a.b` is the screen `a.b`, else `b`: `accounts.register`, a
/// register run beside the app's), and while anything modal is up, only
/// the last met's.
pub fn compose(mut screens: Vec<Screen>) -> Vec<Target> {
    let names: Vec<String> = screens.iter().map(|s| s.name.clone()).collect();
    let resolve = |name: &str| -> Option<usize> {
        names
            .iter()
            .position(|n| n == name)
            .or_else(|| name.rsplit_once('.').and_then(|(_, b)| names.iter().position(|n| n == b)))
    };
    let mut hosted = vec![false; screens.len()];
    for s in &screens {
        for n in &s.nodes {
            if let Node::Hosted { name, .. } = n {
                if let Some(i) = resolve(name) {
                    hosted[i] = true;
                }
            }
        }
    }
    let mut taken: Vec<Option<Vec<Node>>> = screens.iter_mut().map(|s| Some(std::mem::take(&mut s.nodes))).collect();
    let mut out = vec![];
    let mut modals = 0;
    for (i, _) in hosted.iter().enumerate().filter(|(_, h)| !**h) {
        expand(i, &mut taken, &resolve, &mut out, &mut modals);
    }
    let top = out.iter().map(|t| t.modal).max().unwrap_or(0);
    if top > 0 {
        out.retain(|t| t.modal == top);
    }
    out
}

fn expand(i: usize, taken: &mut [Option<Vec<Node>>], resolve: &dyn Fn(&str) -> Option<usize>, out: &mut Vec<Target>, modals: &mut u32) {
    let Some(nodes) = taken[i].take() else { return };
    let base = *modals;
    for n in nodes {
        match n {
            Node::Target(mut t) => {
                if t.modal > 0 {
                    t.modal += base;
                    *modals = (*modals).max(t.modal);
                }
                out.push(t);
            }
            Node::Hosted { name, shown: true } => {
                if let Some(j) = resolve(&name) {
                    expand(j, taken, resolve, out, modals);
                }
            }
            Node::Hosted { .. } => {}
        }
    }
}

/// A shot's step, as shots.json writes it (.github/screens/apps.md).
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// `{"tap": "<label>"}`: the element that says it (`a|b`: either), of
    /// `role` when one is said; the last of several with `"where":
    /// "bottom"`, else the first. A field tapped takes what is typed next.
    Tap { label: String, role: Option<Role>, bottom: bool },
    /// `{"type": "<text>"}`: into the field tapped last, or the one field.
    Type { text: String },
    /// `{"press": "<label>", "item": "<title>"}`: of what holding it
    /// offers (a row's context menu), that item.
    Press { label: String, item: String },
    /// `{"await": "<label>"}`: nothing done; there once it says it.
    Await { label: String },
}

/// What came of a step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Took {
    /// Done: its message sent (or a field focused).
    Done,
    /// Nothing on the screen says it yet: asked again once the screen
    /// changes, until the shot's limit.
    NotYet,
    /// It cannot be done (it says why).
    Failed(String),
}

fn matching<'a>(targets: &'a [Target], label: &str, role: Option<Role>) -> Vec<&'a Target> {
    let wanted: Vec<&str> = label.split('|').collect();
    targets
        .iter()
        .filter(|t| wanted.contains(&t.label.as_str()) && role.is_none_or(|r| t.role == r))
        .collect()
}

/// `step` taken against what is on the screen now. `focus` is the field a
/// tap focused (its label), what `type` types into.
pub fn take(step: &Step, targets: &[Target], focus: &mut Option<String>) -> Took {
    match step {
        Step::Tap { label, role, bottom } => {
            let found = matching(targets, label, *role);
            let acting: Vec<&&Target> = found.iter().filter(|t| t.tap.is_some() || t.typed.is_some()).collect();
            let Some(t) = (if *bottom { acting.last() } else { acting.first() }) else {
                return Took::NotYet;
            };
            match (&t.tap, &t.typed) {
                (Some(tap), _) => {
                    if t.typed.is_some() {
                        *focus = Some(t.label.clone());
                    }
                    tap();
                }
                (None, Some(_)) => *focus = Some(t.label.clone()),
                (None, None) => unreachable!(),
            }
            Took::Done
        }
        Step::Type { text } => {
            let fields: Vec<&Target> = targets.iter().filter(|t| t.typed.is_some()).collect();
            let field = focus
                .as_ref()
                .and_then(|f| fields.iter().find(|t| &t.label == f))
                .or_else(|| (fields.len() == 1).then(|| &fields[0]));
            match field {
                Some(t) => {
                    (t.typed.as_ref().unwrap())(text.clone());
                    Took::Done
                }
                None if fields.is_empty() => Took::NotYet,
                None => Took::Failed(format!(
                    "no field is focused to type into, and there are {}: tap one first",
                    fields.iter().map(|t| t.label.as_str()).collect::<Vec<_>>().join(", ")
                )),
            }
        }
        Step::Press { label, item } => {
            let found = matching(targets, label, None);
            let Some(t) = found.iter().find(|t| !t.menu.is_empty()) else {
                return Took::NotYet;
            };
            match t.menu.iter().find(|(title, _)| title == item) {
                Some((_, act)) => {
                    act();
                    Took::Done
                }
                None => Took::Failed(format!(
                    "{label}'s menu has no {item}: {}",
                    t.menu.iter().map(|(title, _)| title.as_str()).collect::<Vec<_>>().join(", ")
                )),
            }
        }
        Step::Await { label } => match matching(targets, label, None).is_empty() {
            true => Took::NotYet,
            false => Took::Done,
        },
    }
}

/// What is on the screen, a line each: its role, its label, what it does
/// (`taps`, `types`, its menu), and the modal it is in. What a step can
/// name: written beside each shot, and said when a step finds nothing.
pub fn tree(targets: &[Target]) -> String {
    let mut s = String::new();
    for t in targets {
        let mut does = vec![];
        if t.tap.is_some() {
            does.push("taps".to_string());
        }
        if t.typed.is_some() {
            does.push("types".to_string());
        }
        if !t.menu.is_empty() {
            does.push(format!("menu: {}", t.menu.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>().join(", ")));
        }
        s.push_str(&format!("[{}] {}", t.role.name(), t.label));
        if !does.is_empty() {
            s.push_str(&format!(" ({})", does.join("; ")));
        }
        if t.modal > 0 {
            s.push_str(" (modal)");
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests;
