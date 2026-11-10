//! The screens money's shell hosts by name that are not in Rust's view
//! vocabulary yet (ui/docs/app.md, "Escape Hatches"), drawn in arkui-web's
//! parts as money/iced draws its own in arkui's: the budget and its
//! inspector, a register and a transaction's inspector, Insights, the
//! sidebar's foot, Settings' panes; and what goes over the page — New
//! Transaction's sheet, the question before Approve All, Settings.

use std::rc::Rc;

use arkui_web::figures::{self, Bar, Series, Tint};
use arkui_web::menu::Entry;
use arkui_web::table::{self, column, Align, Slot, TableRow};
use arkui_web::{amount, fields, glyph, money, parts, quiet, send, AnyView};
use leptos::html;
use leptos::prelude::*;
use money_app::look;
use money_app::shell::view::money_settings;
use money_app::shell::{tagged_account, ShellMsg, ShellPage};
use money_core::budget::ready_to_assign;
use money_core::dates::add_months;
use money_core::words::Tone;
use money_core::*;

use crate::{Asking, DraftMsg, MoneyWeb, Msg};

fn style() -> ark_format::NumberStyle {
    ark_format::NumberStyle::us()
}

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

pub fn open_accounts(app: &MoneyWeb) -> Vec<&AccountLine> {
    app.store.rows::<AccountLine>(&MoneyView::Accounts).iter().filter(|a| !a.closed).collect()
}

fn tint(t: Tone) -> Tint {
    match t {
        Tone::Neutral => Tint::Neutral,
        Tone::Positive => Tint::Positive,
        Tone::Short | Tone::OnCard => Tint::Caution,
        Tone::Overspent => Tint::Negative,
    }
}

fn bar_tint(t: Tone) -> Tint {
    match t {
        Tone::Neutral | Tone::Positive => Tint::Positive,
        t => tint(t),
    }
}

/// A screen the shell hosts, by name.
pub fn hosted(app: &MoneyWeb, name: &'static str) -> AnyView {
    let section = app.shell.state().section.clone();
    match name {
        "budget" => budget(app),
        "budget.inspector" => budget_inspector(app),
        "accounts" => money_app::accounts::view::accounts::web(&app.accounts.screen_of(&app.store, app.now_ms())),
        "accounts.register" => match tagged_account(&section) {
            Some(hex) => register(app, Some(hex.to_string())),
            None => quiet("Select an Account"),
        },
        "transactions" => register(app, None),
        "transactions.inspector" => txn_inspector(app),
        "insights" => insights(app),
        "household" => household(),
        "settings.general" => settings_general(),
        "settings.household" => settings_household(),
        "settings.banks" => settings_banks(app),
        _ => quiet("Nothing Selected"),
    }
}

// * The budget

fn tile(l: &BudgetLine) -> AnyView {
    let (g, colour) = match l.card {
        Some(_) => ("credit-card", look::CARD_TILE),
        None => (look::category_glyph(&l.name), look::tile_of(&l.name)),
    };
    html::span()
        .class("ark-swatch")
        .attr("style", format!("background:{colour}"))
        .child(glyph(g, 11))
        .into_any()
}

/// The Auto-Assign menu: what each way would do to the month.
fn auto_entries() -> Vec<Entry> {
    vec![
        Entry::run(Some("target"), "Underfunded", Msg::Auto(Auto::Underfunded)),
        Entry::run(Some("repeat"), "Assigned Last Month", Msg::Auto(Auto::LastMonth)),
        Entry::run(Some("undo-2"), "Reset Assigned Amounts", Msg::Auto(Auto::Reset)),
    ]
}

/// The budget: what is ready to assign over every category under its
/// group, each its name and its bar toward its goal, what it was given,
/// what was spent and what it has.
fn budget(app: &MoneyWeb) -> AnyView {
    let s = summary(app);
    let mut said = vec![];
    if s.overspent > 0 {
        said.push(format!("{} overspent", s.overspent));
    }
    if s.underfunded > 0 {
        said.push(format!("{} underfunded", s.underfunded));
    }
    let caption = match (said.is_empty(), s.ready_to_assign) {
        (false, _) => said.join(" \u{b7} "),
        (true, 0) => "Every dollar has a job.".into(),
        (true, _) => "Give these dollars a job.".into(),
    };
    let (label, t) = match s.ready_to_assign {
        r if r < 0 => ("Assigned Too Much", Tint::Negative),
        0 => ("Ready to Assign", Tint::Neutral),
        _ => ("Ready to Assign", Tint::Positive),
    };
    let auto = arkui_web::menu::button(
        "auto-assign",
        "Auto-Assign",
        html::span()
            .class("ark-capsule")
            .child((glyph("wand-sparkles", 13), html::span().child("Auto-Assign")))
            .into_any(),
        "money-auto",
        Rc::new(auto_entries),
    );
    let head = figures::headline(label, money(s.ready_to_assign), t, &caption, Some(auto));
    let columns = [
        column("", "minmax(0,1fr)", Align::Start).slot(Slot::Title),
        column("Assigned", "var(--ark-amount-column-width)", Align::End).wide_only(),
        column("Activity", "var(--ark-amount-column-width)", Align::End).wide_only(),
        column("Available", "var(--ark-amount-column-width)", Align::End).slot(Slot::Trailing),
    ];
    let all = lines(app);
    let mut rows = vec![];
    let mut groups: Vec<(&str, Vec<&BudgetLine>)> = vec![];
    let mut sorted: Vec<&BudgetLine> = all.iter().filter(|l| !l.hidden && !l.is_ready_to_assign()).collect();
    sorted.sort_by_key(|l| (l.group_pos, l.pos));
    for l in sorted {
        match groups.last_mut() {
            Some((g, ls)) if *g == l.group => ls.push(l),
            _ => groups.push((&l.group, vec![l])),
        }
    }
    let categories: Vec<(Id<Category>, String)> = groups
        .iter()
        .flat_map(|(_, ls)| ls.iter().map(|l| (l.category_id, l.name.clone())))
        .collect();
    for (g, ls) in groups {
        let sum = |f: fn(&BudgetLine) -> i64| ls.iter().map(|l| f(l)).sum::<i64>();
        let mut head = TableRow::new(
            g,
            vec![
                html::span()
                    .class("money-group")
                    .child((glyph("chevron-down", 12), html::span().child(g.to_string())))
                    .into_any(),
                html::span().class("ark-quiet").child(money(sum(|l| l.assigned))).into_any(),
                html::span().class("ark-quiet").child(money(sum(|l| l.activity))).into_any(),
                html::span().class("ark-quiet").child(money(sum(|l| l.available))).into_any(),
            ],
        );
        head.heading = true;
        rows.push(head);
        for l in ls {
            rows.push(category_row(app, l, &categories));
        }
    }
    let body = table::table_with(&columns, rows, false, Some(head));
    html::div().class("money-budget").child(body).into_any()
}

fn category_row(app: &MoneyWeb, l: &BudgetLine, categories: &[(Id<Category>, String)]) -> TableRow {
    let tone = l.tone();
    let words = {
        let bar = l.meter().map(|m| figures::goal_bar(m.fraction() as f32, bar_tint(m.tone)));
        let story = matches!(tone, Tone::Overspent | Tone::Short | Tone::OnCard)
            .then(|| l.story(&style()))
            .filter(|s| !s.is_empty())
            .map(|s| html::div().class(format!("money-story ark-text-{}", tint_word(tint(tone)))).child(s));
        html::div()
            .class("money-category")
            .child((
                html::div().class("money-name").child((tile(l), html::span().child(l.name.clone()))),
                bar,
                story,
            ))
            .into_any()
    };
    let id = l.category_id;
    let move_to: Vec<Entry> = categories
        .iter()
        .filter(|(c, _)| *c != id)
        .map(|(c, n)| {
            Entry::run(
                None,
                n.clone(),
                Msg::Assign(*c, {
                    // A row's own number, with what this one holds over moved to it.
                    let to = lines(app).iter().find(|x| x.category_id == *c).map_or(0, |x| x.assigned);
                    to + l.available.max(0)
                }),
            )
        })
        .collect();
    let menu = vec![
        Entry::run(Some("panel-right"), "Show Details", Msg::Category(Some(id))),
        Entry::sub(
            Some("target"),
            "Assign",
            vec![
                Entry::run(
                    None,
                    format!("Underfunded ({})", money(l.underfunded)),
                    Msg::Assign(id, l.assigned + l.underfunded),
                ),
                Entry::run(
                    None,
                    format!("Assigned Last Month ({})", money(l.assigned_last_month)),
                    Msg::Assign(id, l.assigned_last_month),
                ),
                Entry::run(
                    None,
                    format!("Spent Last Month ({})", money(l.spent_last_month)),
                    Msg::Assign(id, l.spent_last_month.abs()),
                ),
                Entry::run(
                    None,
                    format!("Average Spent ({})", money(l.average_spent)),
                    Msg::Assign(id, l.average_spent.abs()),
                ),
                Entry::Separator,
                Entry::run(None, "Reset", Msg::Assign(id, 0)),
            ],
        ),
        Entry::sub(Some("arrow-left-right"), "Move Available To", move_to),
        Entry::Separator,
        Entry::run(Some("list"), "Show Transactions", Msg::Shell(ShellMsg::Search { text: l.name.clone() })),
    ];
    let mut r = TableRow::new(
        l.name.clone(),
        vec![
            words,
            amount(l.assigned, false),
            amount(l.activity, false),
            figures::status_badge(money(l.available), tint(tone)),
        ],
    );
    r.open = Some(Rc::new(move || send(Msg::Category(Some(id)))));
    r.menu = menu;
    r.selected = app.category == Some(id);
    r
}

fn tint_word(t: Tint) -> &'static str {
    match t {
        Tint::Negative => "negative",
        Tint::Caution => "caution",
        Tint::Positive => "positive",
        _ => "quiet",
    }
}

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

// * Registers

fn date_cell(d: &str) -> String {
    ark_format::day_long(d)
}

/// A register: every account's, or one's (`account`, its hex id); what is
/// searched for kept, what waits for a look over it.
fn register(app: &MoneyWeb, account: Option<String>) -> AnyView {
    let q = app.search.to_lowercase();
    let all = all_txns(app);
    let shown: Vec<&TxnLine> = all
        .iter()
        .filter(|t| account.as_ref().is_none_or(|a| t.account_id.hex() == *a))
        .filter(|t| !app.review || !t.approved || (t.category_id.is_none() && !t.transfer))
        .filter(|t| {
            q.is_empty()
                || t.payee.to_lowercase().contains(&q)
                || t.category.to_lowercase().contains(&q)
                || t.memo.to_lowercase().contains(&q)
                || money(t.amount).contains(&q)
        })
        .collect();
    let waiting: Vec<Id<Txn>> = all
        .iter()
        .filter(|t| account.as_ref().is_none_or(|a| t.account_id.hex() == *a))
        .filter(|t| !t.approved)
        .map(|t| t.id)
        .collect();
    let notice = (!waiting.is_empty()).then(|| {
        let n = waiting.len();
        figures::notice_bar(
            "inbox",
            &format!("{n} new transaction{} from your banks", if n == 1 { "" } else { "s" }),
            vec![
                figures::capsule_button(None, if app.review { "Show All" } else { "Review" }, false, Msg::Review(!app.review)),
                figures::capsule_button(None, "Approve All", true, Msg::AskApprove),
            ],
        )
    });
    let one = account.is_some();
    let mut columns = vec![
        column("Date", "112px", Align::Start).slot(Slot::TrailingSub),
        column("Payee", "minmax(140px,1.3fr)", Align::Start).slot(Slot::Title),
        column("Category", "minmax(120px,1fr)", Align::Start).slot(Slot::Subtitle),
    ];
    if !one {
        columns.push(column("Account", "minmax(110px,1fr)", Align::Start).wide_only().slot(Slot::Subtitle));
    }
    columns.push(column("Memo", "minmax(80px,1fr)", Align::Start).wide_only());
    columns.push(column("Amount", "var(--ark-amount-column-width)", Align::End).slot(Slot::Trailing));
    columns.push(column("Cleared", "60px", Align::Center).wide_only());
    let categories: Vec<(Id<Category>, String, String)> = {
        let mut ls: Vec<&BudgetLine> = lines(app).iter().filter(|l| !l.hidden).collect();
        ls.sort_by_key(|l| (l.group_pos, l.pos));
        ls.into_iter().map(|l| (l.category_id, l.group.clone(), l.name.clone())).collect()
    };
    let rows: Vec<TableRow> = shown
        .into_iter()
        .map(|t| {
            let id = t.id;
            let cleared = t.cleared != Cleared::Uncleared;
            let mut cells = vec![
                html::span().child(date_cell(&t.date)).into_any(),
                html::span()
                    .class("money-payee")
                    .child((
                        (!t.approved).then(|| html::span().class("ark-unread-dot")),
                        html::span().child(t.payee.clone()),
                    ))
                    .into_any(),
                match (&t.category_id, t.category.is_empty()) {
                    _ if t.transfer => html::span().class("ark-quiet").child("Transfer").into_any(),
                    (None, _) | (_, true) => html::span().class("money-needs").child("Needs Category").into_any(),
                    _ => html::span().child(t.category.clone()).into_any(),
                },
            ];
            if !one {
                cells.push(html::span().class("ark-quiet").child(t.account.clone()).into_any());
            }
            cells.push(html::span().class("ark-quiet").child(t.memo.clone()).into_any());
            cells.push(arkui_web::signed(t.amount));
            let said = if cleared { "Cleared" } else { "Uncleared" };
            cells.push(
                arkui_web::wired!(html::button(), arkui_web::tapped(said, &Msg::Cleared(id, !cleared)))
                    .class(if cleared { "ark-check-button ark-on" } else { "ark-check-button" })
                    .attr("aria-label", if cleared { "Cleared" } else { "Uncleared" })
                    .on(leptos::ev::click, move |e: web_sys::MouseEvent| {
                        e.stop_propagation();
                        send(Msg::Cleared(id, !cleared))
                    })
                    .child(glyph(if cleared { "circle-check" } else { "circle" }, 14))
                    .into_any(),
            );
            // A category by its group: a submenu each, as a Mac's menu nests.
            let mut by_group: Vec<(String, Vec<Entry>)> = vec![];
            for (c, g, n) in &categories {
                let e = Entry::run(None, n.clone(), Msg::Categorize(id, Some(*c))).checked(t.category_id == Some(*c));
                match by_group.last_mut() {
                    Some((last, es)) if last == g => es.push(e),
                    _ => by_group.push((g.clone(), vec![e])),
                }
            }
            let mut menu = vec![Entry::run(Some("panel-right"), "Show Details", Msg::Txn(Some(id)))];
            if !t.approved {
                menu.push(Entry::run(Some("check"), "Approve", Msg::Approve(vec![id])));
            }
            menu.push(Entry::run(
                Some(if cleared { "circle" } else { "circle-check" }),
                if cleared { "Mark Uncleared" } else { "Mark Cleared" },
                Msg::Cleared(id, !cleared),
            ));
            menu.push(Entry::sub(
                Some("tag"),
                "Categorize",
                by_group.into_iter().map(|(g, es)| Entry::sub(None, g, es)).collect(),
            ));
            menu.push(Entry::Separator);
            menu.push(Entry::run(Some("copy"), "Copy Amount", Msg::Dismiss));
            let mut r = TableRow::new(t.payee.clone(), cells);
            r.open = Some(Rc::new(move || send(Msg::Txn(Some(id)))));
            r.menu = menu;
            r.selected = app.txn == Some(id);
            r
        })
        .collect();
    html::div()
        .class("ark-column ark-fill money-register")
        .child((notice, table::table(&columns, rows)))
        .into_any()
}

fn txn_inspector(app: &MoneyWeb) -> AnyView {
    let Some(t) = app.txn.and_then(|id| all_txns(app).iter().find(|t| t.id == id)) else {
        return quiet("No Transaction Selected");
    };
    let id = t.id;
    let fact = |label: &str, value: String| {
        html::div()
            .class("ark-figure-row")
            .child((html::span().class("ark-caption").child(label.to_string()), html::span().child(value)))
            .into_any()
    };
    let cleared = match t.cleared {
        Cleared::Uncleared => "Uncleared",
        Cleared::Cleared => "Cleared",
        Cleared::Reconciled => "Reconciled",
    };
    html::div()
        .class("ark-inspector-body")
        .child((
            html::div()
                .class("ark-inspector-head")
                .child((
                    html::div().child((
                        html::h2().class("ark-inspector-title").child(t.payee.clone()),
                        html::div().class("ark-caption").child(date_cell(&t.date)),
                    )),
                    arkui_web::shell::bar_button("Close", "x", true, Msg::Txn(None)),
                ))
                .into_any(),
            html::div().class("ark-headline").child(arkui_web::signed(t.amount)).into_any(),
            card(
                None,
                vec![
                    fact("Account", t.account.clone()),
                    fact(
                        "Category",
                        if t.category.is_empty() {
                            "Needs Category".into()
                        } else {
                            t.category.clone()
                        },
                    ),
                    fact("Memo", t.memo.clone()),
                    fact("Cleared", cleared.into()),
                    fact("Imported As", t.imported_payee.clone()),
                ],
            ),
            (!t.approved).then(|| figures::capsule_button(Some("check"), "Approve", true, Msg::Approve(vec![id]))),
        ))
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

/// What goes over the window: Settings, New Transaction's sheet, a question.
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
    let draft = app.draft.as_ref().map(|f| draft_sheet(app, f));
    let asking = app.asking.as_ref().map(|a| match a {
        Asking::ApproveAll(n) => {
            let ids: Vec<Id<Txn>> = all_txns(app).iter().filter(|t| !t.approved).map(|t| t.id).collect();
            parts::modal(
                format!("Approve {n} Transactions?"),
                vec![html::p()
                    .class("ark-dialog-message")
                    .child("They are imported from your banks and have not been looked at yet.")
                    .into_any()],
                vec![
                    parts::push_button("Cancel", true, false, Msg::Dismiss),
                    parts::push_button("Approve All", true, true, Msg::Approve(ids)),
                ],
                Rc::new(|| send(Msg::Dismiss)),
            )
        }
    });
    (settings, draft, asking).into_any()
}

fn draft_sheet(app: &MoneyWeb, f: &crate::Draft) -> AnyView {
    let mut groups: Vec<(String, Vec<Entry>)> = vec![];
    let mut ls: Vec<&BudgetLine> = lines(app).iter().filter(|l| !l.hidden).collect();
    ls.sort_by_key(|l| (l.group_pos, l.pos));
    let mut chosen_name = "Needs Category".to_string();
    for l in ls {
        if f.category == Some(l.category_id) {
            chosen_name = l.name.clone();
        }
        let e = Entry::run(None, l.name.clone(), Msg::Draft(DraftMsg::Category(Some(l.category_id)))).checked(f.category == Some(l.category_id));
        match groups.last_mut() {
            Some((g, es)) if *g == l.group => es.push(e),
            _ => groups.push((l.group.clone(), vec![e])),
        }
    }
    let category_entries: Vec<Entry> =
        std::iter::once(Entry::run(None, "Needs Category", Msg::Draft(DraftMsg::Category(None))).checked(f.category.is_none()))
            .chain(std::iter::once(Entry::Separator))
            .chain(groups.into_iter().map(|(g, es)| Entry::sub(None, g, es)))
            .collect();
    let accounts: Vec<(String, arkui_web::Action)> = open_accounts(app)
        .into_iter()
        .map(|a| {
            let id = a.id;
            (
                a.name.clone(),
                Rc::new(move || send(Msg::Draft(DraftMsg::Account(id)))) as arkui_web::Action,
            )
        })
        .collect();
    let account_name = f
        .account
        .and_then(|id| open_accounts(app).into_iter().find(|a| a.id == id).map(|a| a.name.clone()))
        .unwrap_or_default();
    let category_face = html::span()
        .class("ark-picker-face")
        .child((html::span().child(chosen_name), glyph("chevron-down", 12)))
        .into_any();
    let entries = Rc::new(move || category_entries.clone());
    let body = vec![
        fields::suggest_field("payee", "Payee", f.payee.clone(), Rc::new(|s| send(Msg::Draft(DraftMsg::Payee(s))))),
        html::div()
            .class("ark-form-row")
            .child((
                fields::text_field("Amount", f.amount.clone(), Rc::new(|s| send(Msg::Draft(DraftMsg::Amount(s))))),
                fields::date_field("date", "Date", f.date.clone(), Rc::new(|s| send(Msg::Draft(DraftMsg::Date(s))))),
            ))
            .into_any(),
        html::div()
            .class("ark-form-row")
            .child((
                html::div().class("ark-field").child((
                    html::span().class("ark-caption").child("Category"),
                    arkui_web::menu::button("category", "Category", category_face, "ark-picker", entries),
                )),
                fields::picker("account", "Account", account_name, accounts),
            ))
            .into_any(),
        fields::text_field("Memo", f.memo.clone(), Rc::new(|s| send(Msg::Draft(DraftMsg::Memo(s))))),
        f.problem.clone().map(|p| html::span().class("ark-problem").child(p)).into_any(),
    ];
    parts::modal(
        "New Transaction".into(),
        body,
        vec![
            parts::push_button("Cancel", true, false, Msg::Dismiss),
            parts::push_button("Add Transaction", true, true, Msg::SaveDraft),
        ],
        Rc::new(|| send(Msg::Dismiss)),
    )
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
