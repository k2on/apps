//! arkui-web's own parts, live: what ArkUI's documentation shows of a part
//! in the browser (`ark_app::shell::specimen`, `Part::Specimen`), drawn
//! over a little state of this crate's own ([`State`], the page's like its
//! menus), so a reader presses the real thing. Each is a frame of its own,
//! drawn as a specimen ([`crate::specimen`]): what goes over a window — a
//! sheet, a question, a picker's menu, a row's menu with its submenu, the
//! command box, the keys panel — is drawn open in its frame, so the page
//! shows it without script (ui/docs/app.md, "The Browser's Modes"); taken
//! over, each answers: a press runs, a sheet closes and opens again. The
//! command box and the keys panel are over the page's app's own commands.

use std::rc::Rc;

use ark_app::shell::{confirmation_dialog, Confirming};
use ark_app::view::*;
use ark_app::writing::writing;
use leptos::html;
use leptos::prelude::{AnyView, ClassAttribute, ElementChild, IntoAny};

use crate::menu::Entry;
use crate::{ui, ui_mut, Action};

/// What the specimens are at. The sheet and the question start open, as
/// their pages show them.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub on: bool,
    pub pick: String,
    pub sheet: bool,
    pub name: String,
    pub asking: bool,
    /// What the command box has typed.
    pub query: String,
    /// The row last pressed, said under the parts.
    pub pressed: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            on: false,
            pick: String::new(),
            sheet: true,
            name: "Harbour".into(),
            asking: true,
            query: String::new(),
            pressed: String::new(),
        }
    }
}

/// What a specimen was told: taken by the page itself, never the app's
/// ([`crate::send`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Said {
    Switch(bool),
    Pick(String),
    Sheet(bool),
    Name(String),
    Ask(bool),
    Query(String),
    Press(String),
}

/// `said` taken into the specimens' state.
pub(crate) fn take(said: Said) {
    ui_mut(|u| {
        let s = &mut u.specimens;
        match said {
            Said::Switch(on) => s.on = on,
            Said::Pick(p) => s.pick = p,
            Said::Sheet(o) => s.sheet = o,
            Said::Name(n) => s.name = n,
            Said::Ask(a) => s.asking = a,
            Said::Query(q) => s.query = q,
            Said::Press(p) => s.pressed = p,
        }
    })
}

fn press(what: &str) -> Said {
    Said::Press(what.into())
}

/// A sample of every block the writing draws, for its own page.
pub const WRITING_SAMPLE: &str = "### A Heading\n\nA paragraph with *emphasis*, **strength**, `code` and [a link](https://github.com/k2on/apps).\n\n- A list\n- Of two items\n\n```rust\n#[view]\npub fn help(p: &HelpPage) -> View<HelpMsg> {\n    detail_page(writing(p.body)) // the Markdown is the record's\n}\n```\n\n> [!TIP]\n> A callout says what it is: a note, a tip, something important, a warning or a caution.\n";

/// The part `name`'s specimens, a frame each; none where it has none in
/// the browser.
pub fn draw(name: &str) -> Option<AnyView> {
    let frames = frames(name, &ui(|u| u.specimens.clone()));
    if frames.is_empty() {
        return None;
    }
    Some(
        html::div()
            .class("ark-specimens")
            .child(
                frames
                    .into_iter()
                    .map(|f| html::div().class("ark-specimen-frame").child(f))
                    .collect::<Vec<_>>(),
            )
            .into_any(),
    )
}

/// What was pressed last, said under the parts that answer a press.
fn note(s: &State, ask: &str) -> AnyView {
    let said = match s.pressed.is_empty() {
        true => ask.to_string(),
        false => format!("Pressed: {}.", s.pressed),
    };
    html::p().class("ark-specimen-note").child(said).into_any()
}

fn frames(name: &str, s: &State) -> Vec<AnyView> {
    match name {
        // A row's menu at the pointer, written open with its cursor on an
        // entry with a submenu, as the desktop's picture has it.
        "menu" => vec![
            (
                crate::menu::written("specimen-menu", "Harbour", harbour(), vec![3, 0]),
                note(s, "Press an entry."),
            )
                .into_any(),
            (
                crate::specimen(view(name, s).unwrap_or(View::Empty)),
                note(s, "Press the row, or open its menu."),
            )
                .into_any(),
        ],
        "dialog" => {
            let mut sheet: Vec<View<Said>> = vec![form_button("Rename Harbour", true, Said::Sheet(true))];
            if s.sheet {
                sheet.push(
                    form_sheet(
                        "Rename",
                        Said::Sheet(false),
                        form_section(name_field("Name", s.name.clone(), None::<String>, Said::Name)),
                    )
                    .into(),
                );
            }
            let mut ask: Vec<View<Said>> = vec![form_button("Archive Harbour", true, Said::Ask(true))];
            if s.asking {
                let c: Confirming<Said> = confirmation_dialog("Archive Harbour?", "It leaves the sidebar; nothing is deleted.", Said::Ask(false));
                ask.push(c.into());
            }
            vec![crate::specimen(sheet.into()), crate::specimen(ask.into())]
        }
        // The command box, written open over what it finds: the page's
        // app's own commands.
        "command" => {
            let q = s.query.to_lowercase();
            let rows: Vec<(String, Option<String>, Action)> = crate::keys::listed()
                .into_iter()
                .filter(|(t, _, _)| q.is_empty() || t.to_lowercase().contains(&q))
                .collect();
            let typed: Rc<dyn Fn(String)> = Rc::new(|q| crate::send(Said::Query(q)));
            vec![crate::keys::command_written(&s.query, rows, 0, typed)]
        }
        // A picker's menu under it, written open, the chosen one ticked.
        "picker" => {
            let chosen = if s.pick.is_empty() { "Gold".to_string() } else { s.pick.clone() };
            let choices = ["Gold", "Green", "Blue"]
                .into_iter()
                .map(|c| (c.to_string(), Rc::new(move || crate::send(Said::Pick(c.into()))) as Action))
                .collect();
            vec![crate::specimen_of(|| crate::fields::picker("specimen-accent", "Accent", chosen, choices))]
        }
        // The keys every page has, and the app's own commands' shortcuts.
        "keys" => {
            let mut keys: Vec<(String, String)> = vec![
                ("j / k".into(), "The next row, the last".into()),
                (": / ⌘K".into(), "The command box".into()),
            ];
            keys.extend(crate::keys::listed().into_iter().filter_map(|(t, k, _)| k.map(|k| (k, t))));
            vec![crate::keys::panel_written(&keys)]
        }
        c @ ("lists" | "cards") => view(c, s)
            .map(|v| vec![(crate::specimen(v), note(s, "Press a row.")).into_any()])
            .unwrap_or_default(),
        "writing" => vec![
            crate::specimen(view("writing", s).unwrap_or(View::Empty)),
            crate::writing::code("md", WRITING_SAMPLE),
        ],
        c => view(c, s).map(|v| vec![crate::specimen(v)]).unwrap_or_default(),
    }
}

/// A row's menu: what it offers, a submenu among them.
fn harbour() -> Vec<Entry> {
    let run = |g: Option<&str>, t: &str| Entry::run(g, t.to_string(), press(t));
    vec![
        run(Some("chevron-right"), "Open"),
        run(Some("panel-right"), "Open Beside"),
        run(Some("house"), "Go to Coastal Places"),
        Entry::sub(
            Some("list-filter"),
            "Add to Collection",
            vec![
                run(None, "Favourites").checked(true),
                run(None, "Trips"),
                run(None, "Later"),
                Entry::Separator,
                run(Some("plus"), "New Collection…"),
            ],
        ),
        Entry::Separator,
        run(Some("minus"), "Remove").destructive(),
    ]
}

/// The part `name` as one view, or nothing where it has none.
fn view(name: &str, s: &State) -> Option<View<Said>> {
    Some(match name {
        "lists" => card_section((
            list_row("Checking")
                .subtitle("Everyday spending")
                .tile("landmark")
                .amount(412_050)
                .pushes()
                .on_tap(press("Checking")),
            list_row("Savings")
                .subtitle("For a rainy day")
                .tile("piggy-bank")
                .amount(1_250_000)
                .pushes()
                .on_tap(press("Savings")),
            list_row("Credit Card")
                .subtitle("Paid in full monthly")
                .tile("credit-card")
                .amount(-31_299)
                .pushes()
                .on_tap(press("Credit Card")),
        ))
        .header("Accounts")
        .into(),
        "cards" => (
            card_section((
                list_row("Groceries").glyph("shopping-cart").amount(41_250).on_tap(press("Groceries")),
                list_row("Dining Out").glyph("utensils").amount(8_840).on_tap(press("Dining Out")),
            ))
            .header("Everyday"),
            card_section(list_row("Fuel").glyph("fuel").amount(6_100).on_tap(press("Fuel"))).header("Getting Around"),
        )
            .into(),
        "headers" => vstack((
            text("Net Worth").style(TextStyle::Subheadline).ink(Ink::Quiet),
            text("$16,317.51").style(TextStyle::Figure).lines(1),
            text("Every open account, less what's owed").style(TextStyle::Footnote).ink(Ink::Quiet),
        ))
        .align(Align::Start)
        .padding(Edges::Horizontal, Space::Page)
        .into(),
        "menu" => card_section(
            list_row("Harbour")
                .subtitle("Right-click, or press the menu key, for its menu")
                .tile("house")
                .on_tap(press("Harbour"))
                .menu("Open", "chevron-right", press("Open"), false)
                .menu("Open Beside", "panel-right", press("Open Beside"), false)
                .menu("Remove", "minus", press("Remove"), true),
        )
        .into(),
        "navigation" => chip_row((
            chip("Budget", "wallet", s.pick == "budget", Said::Pick("budget".into())),
            chip("Accounts", "landmark", s.pick == "accounts", Said::Pick("accounts".into())),
            chip("Insights", "chart-column", s.pick == "insights", Said::Pick("insights".into())),
        )),
        "style" | "springs" => switch_row("Reduce Motion", "Only the fade, as the system's setting asks", s.on, Said::Switch),
        "account" => card_section(list_row("Signed In").subtitle("On this device").glyph("shield-check").value("Harbour")).into(),
        "text" | "writing" => writing(WRITING_SAMPLE),
        _ => return None,
    })
}
