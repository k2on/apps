//! A component's live parts: arkui-web drawing the same `ark_app::view`
//! parts an app's screen is made of, over a little state of their own, so
//! a reader presses the real thing. Each is a frame of its own, drawn as a
//! specimen (`arkui_web::specimen`): what goes over a window — a sheet, a
//! question, a picker's menu, a row's menu with its submenu, the command
//! box, the keys panel — is drawn open in its frame, so the page shows it
//! without script (ui/docs/app.md, "The Browser's Modes"); taken over,
//! each answers: a press runs, a sheet closes and opens again.
use std::rc::Rc;

use ark_app::shell::{confirmation_dialog, Confirming};
use ark_app::view::*;
use ark_app::writing::writing;
use arkui_web::menu::Entry;
use arkui_web::AnyView;
use leptos::prelude::{ClassAttribute, ElementChild, IntoAny};

use crate::app::Msg;

/// What the live parts are at. The sheet and the question start open, as
/// their pages show them.
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
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

impl Default for Live {
    fn default() -> Self {
        Live {
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

#[derive(Clone, Debug)]
pub enum LiveMsg {
    Switch(bool),
    Pick(String),
    Sheet(bool),
    Name(String),
    Ask(bool),
    Query(String),
    Press(String),
}

impl Live {
    pub fn update(&mut self, m: LiveMsg) {
        match m {
            LiveMsg::Switch(on) => self.on = on,
            LiveMsg::Pick(p) => self.pick = p,
            LiveMsg::Sheet(s) => self.sheet = s,
            LiveMsg::Name(n) => self.name = n,
            LiveMsg::Ask(a) => self.asking = a,
            LiveMsg::Query(q) => self.query = q,
            LiveMsg::Press(p) => self.pressed = p,
        }
    }
}

fn live(m: LiveMsg) -> Msg {
    Msg::Live(m)
}

fn press(what: &str) -> Msg {
    live(LiveMsg::Press(what.into()))
}

/// A sample of every block the writing draws, for its own page.
pub const WRITING_SAMPLE: &str = "### A Heading\n\nA paragraph with *emphasis*, **strength**, `code` and [a link](https://github.com/k2on/apps).\n\n- A list\n- Of two items\n\n```rust\n#[view]\npub fn help(p: &HelpPage) -> View<HelpMsg> {\n    detail_page(writing(p.body)) // the Markdown is the record's\n}\n```\n\n> [!TIP]\n> A callout says what it is: a note, a tip, something important, a warning or a caution.\n";

/// `component`'s live parts, a frame each: none where it has none in the
/// browser.
pub fn frames(component: &str, l: &Live) -> Vec<AnyView> {
    // What was pressed last, said under the parts that answer a press.
    let note = |l: &Live, ask: &str| -> AnyView {
        let said = match l.pressed.is_empty() {
            true => ask.to_string(),
            false => format!("Pressed: {}.", l.pressed),
        };
        leptos::html::p().class("docs-note").child(said).into_any()
    };
    match component {
        // A row's menu at the pointer, written open with its cursor on an
        // entry with a submenu, as the desktop's picture has it.
        "menu" => vec![
            (
                arkui_web::menu::written("docs-menu", "Harbour", harbour(), vec![3, 0]),
                note(l, "Press an entry."),
            )
                .into_any(),
            (
                arkui_web::specimen(view(component, l).unwrap_or(View::Empty)),
                note(l, "Press the row, or open its menu."),
            )
                .into_any(),
        ],
        "dialog" => {
            let mut sheet: Vec<View<Msg>> = vec![form_button("Rename Harbour", true, live(LiveMsg::Sheet(true)))];
            if l.sheet {
                sheet.push(
                    form_sheet(
                        "Rename",
                        live(LiveMsg::Sheet(false)),
                        form_section(name_field("Name", l.name.clone(), None::<String>, |n| live(LiveMsg::Name(n)))),
                    )
                    .into(),
                );
            }
            let mut ask: Vec<View<Msg>> = vec![form_button("Archive Harbour", true, live(LiveMsg::Ask(true)))];
            if l.asking {
                let c: Confirming<Msg> = confirmation_dialog(
                    "Archive Harbour?",
                    "It leaves the sidebar; nothing is deleted.",
                    live(LiveMsg::Ask(false)),
                );
                ask.push(c.into());
            }
            vec![arkui_web::specimen(sheet.into()), arkui_web::specimen(ask.into())]
        }
        // The command box, written open over what it finds: every page.
        "command" => {
            let q = l.query.to_lowercase();
            let rows: Vec<(String, Option<String>, arkui_web::Action)> = crate::pages::pages("/")
                .into_iter()
                .filter(|p| q.is_empty() || p.title.to_lowercase().contains(&q))
                .map(|p| {
                    let tag = p.tag.clone();
                    (
                        format!("Go to {}", p.title),
                        None,
                        Rc::new(move || arkui_web::send(Msg::Go { tag: tag.clone() })) as arkui_web::Action,
                    )
                })
                .collect();
            let typed: Rc<dyn Fn(String)> = Rc::new(|q| arkui_web::send(live(LiveMsg::Query(q))));
            vec![arkui_web::keys::command_written(&l.query, rows, 0, typed)]
        }
        // A picker's menu under it, written open, the chosen one ticked.
        "picker" => {
            let chosen = if l.pick.is_empty() { "Gold".to_string() } else { l.pick.clone() };
            let choices = ["Gold", "Green", "Blue"]
                .into_iter()
                .map(|c| {
                    (
                        c.to_string(),
                        Rc::new(move || arkui_web::send(live(LiveMsg::Pick(c.into())))) as arkui_web::Action,
                    )
                })
                .collect();
            vec![arkui_web::specimen_of(|| {
                arkui_web::fields::picker("docs-accent", "Accent", chosen, choices)
            })]
        }
        "keys" => vec![arkui_web::keys::panel_written(&[
            ("j / k".into(), "The next page, the last (the sidebar)".into()),
            (": / ⌘K".into(), "The command box".into()),
        ])],
        c @ ("lists" | "cards") => view(c, l)
            .map(|v| vec![(arkui_web::specimen(v), note(l, "Press a row.")).into_any()])
            .unwrap_or_default(),
        c => view(c, l).map(|v| vec![arkui_web::specimen(v)]).unwrap_or_default(),
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

/// `component`'s parts as one view, or nothing where it has none.
fn view(component: &str, l: &Live) -> Option<View<Msg>> {
    let said = |v: View<Msg>| -> View<Msg> { v };
    Some(match component {
        "lists" => said(
            card_section((
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
        ),
        "cards" => said(
            (
                card_section((
                    list_row("Groceries").glyph("shopping-cart").amount(41_250).on_tap(press("Groceries")),
                    list_row("Dining Out").glyph("utensils").amount(8_840).on_tap(press("Dining Out")),
                ))
                .header("Everyday"),
                card_section(list_row("Fuel").glyph("fuel").amount(6_100).on_tap(press("Fuel"))).header("Getting Around"),
            )
                .into(),
        ),
        "headers" => hero_figure("Net Worth", 1_631_751, "Every open account, less what's owed"),
        "menu" => said(
            card_section(
                list_row("Harbour")
                    .subtitle("Right-click, or press the menu key, for its menu")
                    .tile("house")
                    .on_tap(press("Harbour"))
                    .menu("Open", "chevron-right", press("Open"), false)
                    .menu("Open Beside", "panel-right", press("Open Beside"), false)
                    .menu("Remove", "minus", press("Remove"), true),
            )
            .into(),
        ),
        "navigation" => chip_row((
            chip("Budget", "wallet", l.pick == "budget", live(LiveMsg::Pick("budget".into()))),
            chip("Accounts", "landmark", l.pick == "accounts", live(LiveMsg::Pick("accounts".into()))),
            chip("Insights", "chart-column", l.pick == "insights", live(LiveMsg::Pick("insights".into()))),
        )),
        "style" | "springs" => switch_row("Reduce Motion", "Only the fade, as the system's setting asks", l.on, |b| {
            live(LiveMsg::Switch(b))
        }),
        "account" => card_section(list_row("Signed In").subtitle("On this device").glyph("shield-check").value("Harbour")).into(),
        "text" | "writing" => writing(WRITING_SAMPLE),
        _ => return None,
    })
}
