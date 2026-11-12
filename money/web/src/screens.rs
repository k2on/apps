//! The screens money's shell hosts by name: the ones written once in Rust
//! (Budget, Accounts, Transactions, a register, the sheets over the page)
//! drawn from their views, and those not in Rust's view vocabulary yet
//! (ui/docs/app.md, "Escape Hatches") drawn in arkui-web's parts as
//! money/iced draws its own in arkui's: the budget's inspector with nothing
//! chosen, Insights, the sidebar's foot, Settings' panes.

use std::rc::Rc;

use arkui_web::{amount, glyph, parts, quiet, send, AnyView};
use leptos::html;
use leptos::prelude::*;
use money_app::shell::view::money_settings;
use money_app::shell::{tagged_account, ShellPage};
use money_core::*;

use crate::{MoneyWeb, Msg};

pub fn lines(app: &MoneyWeb) -> &[BudgetLine] {
    app.store.rows(&MoneyView::Lines { month: app.month() })
}

pub fn summary(app: &MoneyWeb) -> Summary {
    app.store
        .first::<Summary>(&MoneyView::Summary { month: app.month() })
        .cloned()
        .unwrap_or_default()
}

/// A screen the shell hosts, by name.
pub fn hosted(app: &MoneyWeb, name: &'static str) -> AnyView {
    let section = app.app().shell.state().section.clone();
    match name {
        "budget" => arkui_web::render(money_app::budget::view::budget::view(&app.page::<money_app::budget::Budget>()).map(crate::Msg::Budget)),
        "budget.inspector" => budget_inspector(app),
        "accounts" => arkui_web::render(money_app::accounts::view::accounts::view(&app.page::<money_app::accounts::Accounts>())),
        "accounts.register" => match tagged_account(&section) {
            Some(_) => match &app.app().register {
                Some(r) => arkui_web::render(money_app::register::view::register::view(&r.page(&app.seen(), app.now_ms())).map(Msg::Register)),
                None => quiet("Select an Account"),
            },
            None => quiet("Select an Account"),
        },
        "transactions" => arkui_web::render(
            money_app::transactions::view::transactions::view(&app.page::<money_app::transactions::Transactions>()).map(Msg::Transactions),
        ),
        "transactions.inspector" => quiet("No Transaction Selected"),
        "insights" => arkui_web::render(money_app::insights::view::insights::view(&app.page::<money_app::insights::Insights>()).map(Msg::Insights)),
        "household" => household(),
        _ => quiet("Nothing Selected"),
    }
}

// * The budget

fn figure_row(label: &str, cents: i64) -> AnyView {
    html::div()
        .class("ark-figure-row")
        .child((html::span().child(label.to_string()), amount(cents, false)))
        .into_any()
}

fn card(title: Option<String>, body: Vec<AnyView>) -> AnyView {
    html::section()
        .class("ark-card ark-panel")
        .child((title.map(|t| html::h3().class("ark-panel-title").child(t)), body))
        .into_any()
}

/// The budget's inspector with no category chosen: the month. A category
/// chosen is the shell's Category Details (`category_inspector`), the same
/// on every platform.
fn budget_inspector(app: &MoneyWeb) -> AnyView {
    let s = summary(app);
    let carried: i64 = lines(app).iter().map(|l| l.carried).sum();
    let body = vec![
        card(
            Some(format!("{}'s Summary", ark_format::month(&app.month()))),
            vec![
                figure_row("Left over from last month", carried),
                figure_row("Assigned this month", s.assigned),
                figure_row("Activity", s.activity),
                figure_row("Available", s.available),
                html::div().class("ark-row-divider").into_any(),
                figure_row("Income this month", s.income),
                figure_row("Overspent last month", s.overspent_last_month),
            ],
        ),
        card(
            Some("Auto-Assign".into()),
            vec![
                html::div()
                    .class("ark-caption")
                    .child("Every category at once, for this month.")
                    .into_any(),
                quiet_button("Underfunded", Msg::Auto(Auto::Underfunded)),
                quiet_button("Assigned last month", Msg::Auto(Auto::LastMonth)),
                quiet_button("Reset assigned amounts", Msg::Auto(Auto::Reset)),
            ],
        ),
    ];
    html::div().class("ark-inspector-body").child(body).into_any()
}

fn quiet_button(label: &str, m: Msg) -> AnyView {
    arkui_web::wired!(html::button(), arkui_web::tapped(label, &m))
        .class("ark-link")
        .on(leptos::ev::click, move |_| send(m.clone()))
        .child(label.to_string())
        .into_any()
}

fn household() -> AnyView {
    html::div()
        .class("money-household")
        .child((
            html::button().class("money-me").child((
                html::span().class("money-face").child(glyph("users", 13)),
                html::span().child("Demo Household"),
            )),
            arkui_web::wired!(html::button(), arkui_web::tapped("Settings", &Msg::Settings(true)))
                .class("ark-bar-button")
                .attr("title", "Settings")
                .attr("aria-label", "Settings")
                .on(leptos::ev::click, |_| send(Msg::Settings(true)))
                .child(glyph("settings", 15)),
        ))
        .into_any()
}

// * Over the page

/// What goes over the window: Settings, and the sheets (`money_app::sheets`).
pub fn over(app: &MoneyWeb, page: &ShellPage) -> AnyView {
    let settings = app.settings.then(|| {
        let hosts = |name: &'static str| hosted(app, name);
        let panes = arkui_web::shell(money_settings::view(page).map(Msg::Shell), &hosts);
        parts::modal(
            "Settings".into(),
            vec![html::div().class("money-settings-window").child(panes).into_any()],
            vec![parts::push_button("Done", true, true, Msg::Settings(false))],
            Rc::new(|| send(Msg::Settings(false))),
        )
    });
    // A receipt's photo is read from the replica's memory (`data:`).
    let sheets = arkui_web::render(money_app::sheets::view::sheets::view(&app.page::<money_app::sheets::Sheets>()).map(Msg::Sheets));
    (settings, sheets).into_any()
}

impl MoneyWeb {
    /// What a field suggests: the payees for a payee, and in the search
    /// field the payees and categories that match.
    pub fn suggestions(&self, field: &str, typed: &str) -> Vec<String> {
        let q = typed.to_lowercase();
        let payees: &[PayeeLine] = self.store.rows(&MoneyView::Payees);
        let mut out: Vec<String> = payees
            .iter()
            .filter(|p| p.name.to_lowercase().contains(&q) && p.name.to_lowercase() != q)
            .map(|p| p.name.clone())
            .collect();
        if field == "search" {
            out.extend(lines(self).iter().filter(|l| l.name.to_lowercase().contains(&q)).map(|l| l.name.clone()));
        }
        out.dedup();
        out.truncate(6);
        out
    }
}
