//! A component's live parts: arkui-web drawing the same `ark_app::view`
//! parts an app's screen is made of, over a little state of their own, so
//! a reader presses the real thing. Written as HTML they are drawn as they
//! open and their presses wait for script (ui/docs/app.md, "The Browser's
//! Modes"); taken over, they answer.
use ark_app::shell::{confirmation_dialog, Confirming};
use ark_app::view::*;
use ark_app::writing::writing;

use crate::app::Msg;

/// What the live parts are at.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Live {
    pub on: bool,
    pub pick: String,
    pub sheet: bool,
    pub name: String,
    pub asking: bool,
    /// The row last pressed, said under the parts.
    pub pressed: String,
}

#[derive(Clone, Debug)]
pub enum LiveMsg {
    Switch(bool),
    Pick(String),
    Sheet(bool),
    Name(String),
    Ask(bool),
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

/// `component`'s live parts, or nothing where it has none in the browser.
pub fn view(component: &str, l: &Live) -> Option<View<Msg>> {
    let said = |v: View<Msg>| -> View<Msg> {
        let note = match l.pressed.is_empty() {
            true => "Press a row.".to_string(),
            false => format!("Pressed: {}.", l.pressed),
        };
        (v, writing(note)).into()
    };
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
                    .menu("Rename", "pencil", press("Rename"), false)
                    .menu("Duplicate", "copy", press("Duplicate"), false)
                    .menu("Archive", "archive", press("Archive"), true),
            )
            .into(),
        ),
        "picker" => option_picker(
            "Accent",
            l.pick.clone(),
            |t| live(LiveMsg::Pick(t)),
            (
                picker_choice("Gold", "star", "gold"),
                picker_choice("Green", "circle", "green"),
                picker_choice("Blue", "droplet", "blue"),
            ),
        ),
        "navigation" => chip_row((
            chip("Budget", "wallet", l.pick == "budget", live(LiveMsg::Pick("budget".into()))),
            chip("Accounts", "landmark", l.pick == "accounts", live(LiveMsg::Pick("accounts".into()))),
            chip("Insights", "chart-column", l.pick == "insights", live(LiveMsg::Pick("insights".into()))),
        )),
        "style" | "springs" => switch_row("Reduce Motion", "Only the fade, as the system's setting asks", l.on, |b| {
            live(LiveMsg::Switch(b))
        }),
        "dialog" => {
            let mut v: Vec<View<Msg>> = vec![
                form_button("Show Sheet", true, live(LiveMsg::Sheet(true))),
                form_button("Ask a Question", true, live(LiveMsg::Ask(true))),
            ];
            if l.sheet {
                v.push(
                    form_sheet(
                        "Rename",
                        live(LiveMsg::Sheet(false)),
                        form_section(name_field("Name", l.name.clone(), None::<String>, |n| live(LiveMsg::Name(n)))),
                    )
                    .into(),
                );
            }
            if l.asking {
                let ask: Confirming<Msg> = confirmation_dialog(
                    "Archive Harbour?",
                    "It leaves the sidebar; nothing is deleted.",
                    live(LiveMsg::Ask(false)),
                );
                v.push(ask.into());
            }
            v.into()
        }
        "account" => card_section(list_row("Signed In").subtitle("On this device").glyph("shield-check").value("Harbour")).into(),
        "text" | "writing" => writing(WRITING_SAMPLE),
        _ => return None,
    })
}
