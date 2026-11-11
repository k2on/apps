//! The screens money's shell hosts by name: the ones written once in Rust
//! (Budget, Accounts, Transactions, a register, the sheets over the page)
//! drawn from their views, and those not in Rust's view vocabulary yet
//! (ui/docs/app.md, "Escape Hatches") drawn in arkui-web's parts as
//! money/iced draws its own in arkui's: the budget's inspector with nothing
//! chosen, Insights, the sidebar's foot, Settings' panes.

use std::rc::Rc;

use arkui_web::figures::{self, Bar, Series};
use arkui_web::{amount, fields, glyph, money, parts, quiet, send, AnyView};
use leptos::html;
use leptos::prelude::*;
use money_app::shell::view::money_settings;
use money_app::shell::{tagged_account, ShellPage};
use money_core::budget::ready_to_assign;
use money_core::dates::add_months;
use money_core::*;

use crate::{MoneyWeb, Msg};

pub fn lines(app: &MoneyWeb) -> &[BudgetLine] {
    app.store.rows(&MoneyView::Lines { month: app.month.clone() })
}

pub fn summary(app: &MoneyWeb) -> Summary {
    app.store
        .first::<Summary>(&MoneyView::Summary { month: app.month.clone() })
        .cloned()
        .unwrap_or_default()
}

pub fn all_txns(app: &MoneyWeb) -> &[TxnLine] {
    app.store.rows(&MoneyView::Register { account: None })
}

/// A screen the shell hosts, by name.
pub fn hosted(app: &MoneyWeb, name: &'static str) -> AnyView {
    let section = app.shell.state().section.clone();
    match name {
        "budget" => {
            // The window's month, whatever the page was last told.
            let state = money_app::budget::State {
                month: app.month.clone(),
                ..app.budget.state().clone()
            };
            let data = ark_app::Data {
                store: &app.store,
                now_ms: app.now_ms(),
            };
            arkui_web::render(
                money_app::budget::view::budget::view(&<money_app::budget::Budget as ark_app::App>::screen(&state, &data)).map(crate::Msg::Budget),
            )
        }
        "budget.inspector" => budget_inspector(app),
        "accounts" => arkui_web::render(money_app::accounts::view::accounts::view(
            &app.accounts.screen_of(&app.store, app.now_ms()),
        )),
        "accounts.register" => match tagged_account(&section) {
            Some(_) => {
                arkui_web::render(money_app::register::view::register::view(&app.register.screen_of(&app.store, app.now_ms())).map(Msg::Register))
            }
            None => quiet("Select an Account"),
        },
        "transactions" => arkui_web::render(
            money_app::transactions::view::transactions::view(&app.transactions.screen_of(&app.store, app.now_ms())).map(Msg::Transactions),
        ),
        "transactions.inspector" => quiet("No Transaction Selected"),
        "insights" => insights(app),
        "household" => household(),
        "settings.general" => settings_general(),
        "settings.household" => settings_household(),
        "settings.banks" => settings_banks(app),
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
            Some(format!("{}'s Summary", ark_format::month(&app.month))),
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

// * Insights

fn insights(app: &MoneyWeb) -> AnyView {
    let all = all_txns(app);
    let month = app.month.clone();
    let income = |m: &str| -> i64 {
        all.iter()
            .filter(|t| t.date.starts_with(m) && !t.transfer && t.category_id == Some(ready_to_assign()) && t.amount > 0)
            .map(|t| t.amount)
            .sum()
    };
    let spent_in = |m: &str| -> Vec<&TxnLine> {
        all.iter()
            .filter(|t| t.date.starts_with(m) && !t.transfer && t.category_id.is_some_and(|c| c != ready_to_assign()))
            .collect()
    };
    let spending = |m: &str| -> i64 { -spent_in(m).iter().map(|t| t.amount).sum::<i64>() };
    let (came, went) = (income(&month), spending(&month));
    let kept = match came {
        0 => "\u{2014}".to_string(),
        c => format!("{}%", ((c - went) * 100 / c).max(0)),
    };
    let compact = arkui_web::shell::size_class() == arkui_web::shell::SizeClass::Compact;
    let tiles = vec![
        figures::stat_tile("Income", money(came), None),
        figures::stat_tile("Spending", money(went), None),
        figures::stat_tile("Kept", kept, Some("of income not spent")),
    ];
    let mut by: Vec<(String, i64)> = vec![];
    for t in spent_in(&month) {
        match by.iter_mut().find(|(n, _)| *n == t.category) {
            Some((_, v)) => *v -= t.amount,
            None => by.push((t.category.clone(), -t.amount)),
        }
    }
    by.retain(|(_, v)| *v > 0);
    by.sort_by_key(|b| std::cmp::Reverse(b.1));
    let mut bars: Vec<Bar> = by
        .iter()
        .take(8)
        .map(|(n, v)| Bar {
            label: n.clone(),
            value: *v,
            said: money(*v),
        })
        .collect();
    let other: i64 = by.iter().skip(8).map(|(_, v)| v).sum();
    if other > 0 {
        bars.push(Bar {
            label: "Other".into(),
            value: other,
            said: money(other),
        });
    }
    let by_category = match bars.is_empty() {
        true => html::div().class("ark-caption").child("Nothing spent this month yet.").into_any(),
        false => figures::labelled_bars(&bars),
    };
    let months: Vec<String> = (-2..=0).map(|d| add_months(&month, d)).collect();
    let groups: Vec<(String, Vec<i64>)> = months
        .iter()
        .map(|m| (ark_format::month_alone(m), vec![income(m), spending(m)]))
        .collect();
    let series = [Series { name: "Income".into() }, Series { name: "Spending".into() }];
    let in_out = figures::column_chart(&series, &groups, 180.0);
    let page = app.accounts.screen_of(&app.store, app.now_ms());
    let first = page.trend.first().copied().unwrap_or(page.net_worth);
    let change = page.net_worth - first;
    let worth = html::div()
        .class("ark-column")
        .child((
            html::div().class("money-worth").child(money(page.net_worth)),
            html::div()
                .class("ark-caption")
                .child(format!("{}{} over twelve weeks", if change >= 0 { "+" } else { "" }, money(change))),
            figures::line_chart(&page.trend, 150.0, true),
        ))
        .into_any();
    let cards = vec![
        figures::chart_card("Spending by Category", by_category),
        figures::chart_card("Income vs Spending", in_out),
        figures::chart_card("Net Worth", worth),
    ];
    let columns = match (compact, arkui_web::shell::sections()) {
        (true, _) => 1,
        (false, arkui_web::shell::Sections::Sidebar) => 3,
        _ => 2,
    };
    html::div()
        .class("ark-page")
        .child(html::div().class("money-insights").child((
            figures::grid(tiles, if compact { 1 } else { 3 }, 14.0),
            figures::grid(cards, columns, 14.0),
        )))
        .into_any()
}

// * The sidebar's foot, and Settings

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

fn settings_general() -> AnyView {
    // What the page says (`data-theme`); written as HTML, the address's.
    let theme = match arkui_web::page::writing() {
        true => arkui_web::query("theme"),
        false => web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
            .and_then(|e| e.get_attribute("data-theme")),
    }
    .unwrap_or_default();
    let chosen = match theme.as_str() {
        "light" => "Light",
        "dark" => "Dark",
        _ => "System",
    };
    let choice = |t: &'static str, v: &'static str| -> (String, arkui_web::Action) {
        (
            t.to_string(),
            Rc::new(move || {
                if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
                    let _ = match v {
                        "" => root.remove_attribute("data-theme"),
                        v => root.set_attribute("data-theme", v),
                    };
                }
                arkui_web::redraw();
            }),
        )
    };
    html::div()
        .class("money-settings")
        .child((
            fields::picker(
                "appearance",
                "Appearance",
                chosen.into(),
                vec![choice("System", ""), choice("Light", "light"), choice("Dark", "dark")],
            ),
            html::p()
                .class("ark-caption")
                .child("The demo household lives in this page alone: nothing is sent anywhere."),
        ))
        .into_any()
}

fn settings_household() -> AnyView {
    html::div()
        .class("money-settings")
        .child(card(
            Some("Who Shares This Budget".into()),
            vec![html::div()
                .class("ark-figure-row")
                .child((html::span().child("Demo Household"), html::span().class("ark-caption").child("Owner")))
                .into_any()],
        ))
        .into_any()
}

fn settings_banks(app: &MoneyWeb) -> AnyView {
    let banks: &[BankLine] = app.store.rows(&MoneyView::Banks);
    let rows: Vec<AnyView> = banks
        .iter()
        .map(|b| {
            html::div()
                .class("ark-figure-row")
                .child((
                    html::span().child(b.institution.clone()),
                    html::span().class("ark-caption").child(format!("{} accounts", b.accounts)),
                ))
                .into_any()
        })
        .collect();
    html::div()
        .class("money-settings")
        .child(card(
            Some("Connected Banks".into()),
            if rows.is_empty() { vec![quiet("No Banks Connected")] } else { rows },
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
    let sheets = arkui_web::render(money_app::sheets::view::sheets::view(&app.sheets.screen_of(&app.store, app.now_ms())).map(Msg::Sheets));
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
