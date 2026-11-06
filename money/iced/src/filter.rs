//! The register's filter: every register is the register for a filter
//! (`peer::Ledger`) — every account, what needs a look, one account, a
//! saved filter, or conditions not saved — and its bar says the conditions
//! as chips to change. Changing one makes the register one not saved
//! (unless it is then exactly a built-in one, or the saved one it started
//! as), which "Save as filter…" keeps under a name; the sidebar lists the
//! saved ones, and a formula's terms sum them (`formula.rs`).
//!
//! What a transaction must meet is the domain's alone
//! (`money_domain::filters`): this file only edits a `Filter` and opens
//! the register for it.

use arkui::{glyphs, icon, menu, style};
use iced::widget::{button, column, row, text, text_input};
use iced::{Alignment, Element, Length, Task};
use money_domain::dates::{add_months, first_day, last_day, month_name, month_of};
use money_domain::*;

use crate::look;
use crate::peer::Ledger;
use crate::view::{action, bold, dim, fill, link, quiet};
use crate::{App, Choosing, Message, Modal, Page};

/// One condition changed.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// An account in, or out.
    Account(Id<Account>),
    AllAccounts,
    OnBudget(bool),
    Text(String),
    /// A category in, or out; `None` is "no category".
    Category(Option<Id<Category>>),
    AnyCategory,
    Direction(Direction),
    NoTransfers(bool),
    NeedsLook(bool),
    Dates(Option<String>, Option<String>),
}

/// A chip of the bar, whose menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Accounts,
    Categories,
    Direction,
    Transfers,
    Dates,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Edit(Edit),
    Chip(Chip),
    /// The register for these conditions, not saved, changed from `base`.
    Open(Filter, Ledger),
    /// "Save as filter…", its name typed, saved.
    SaveAs,
    Name(String),
    SaveDone,
    /// A saved filter's conditions, as changed here, kept in it.
    SaveChanges,
    Delete(Id<SavedFilter>),
}

impl App {
    /// The conditions of the register on screen.
    pub fn register_filter(&self) -> Filter {
        match self.peer.ledger {
            Ledger::All => Filter::all(),
            Ledger::Inbox => Filter::needs_a_look(),
            Ledger::Account(a) => Filter::account(a),
            Ledger::Saved(id) => self.peer.saved(id).map(|f| f.filter.clone()).unwrap_or_default(),
            Ledger::Adhoc => self.peer.adhoc.clone(),
        }
    }

    /// The one account the register is exactly the register of, if it is:
    /// its balances, Reconcile, Rename and Sync are then its account's.
    pub fn register_account(&self) -> Option<Id<Account>> {
        self.register_filter().only_account()
    }

    /// The saved filter the register is, or was changed from.
    pub fn register_saved(&self) -> Option<Id<SavedFilter>> {
        match (self.peer.ledger, self.peer.base) {
            (Ledger::Saved(id), _) | (Ledger::Adhoc, Ledger::Saved(id)) => Some(id),
            _ => None,
        }
    }

    pub fn filter_msg(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::Edit(e) => return self.edit_filter(e),
            Msg::Chip(c) => return self.chip(c),
            Msg::Open(f, base) => return self.open_filter(f, base),
            Msg::SaveAs => {
                let name = self
                    .register_saved()
                    .and_then(|id| self.peer.saved(id))
                    .map(|f| format!("{} (changed)", f.name));
                self.modal = Some(Modal::SaveFilter {
                    name: name.unwrap_or_default(),
                });
                return iced::widget::operation::focus(look::FILTER_NAME);
            }
            Msg::Name(t) => {
                if let Some(Modal::SaveFilter { name }) = &mut self.modal {
                    *name = t;
                }
            }
            Msg::SaveDone => {
                let Some(Modal::SaveFilter { name }) = self.modal.clone() else {
                    return Task::none();
                };
                let s = SaveFilter {
                    name: name.trim().to_string(),
                    filter: self.register_filter(),
                };
                if self.author("save the filter", "filters.save_filter", &s) {
                    self.modal = None;
                    self.note = format!("saved as {}", s.name);
                    if let Some(f) = self.peer.filters.rows.iter().find(|f| f.name.eq_ignore_ascii_case(&s.name)) {
                        let id = f.id;
                        self.history.replace_next();
                        return self.update(Message::Go(Page::Ledger(Ledger::Saved(id))));
                    }
                }
            }
            Msg::SaveChanges => {
                let Some(id) = self.register_saved() else { return Task::none() };
                let c = ChangeFilter {
                    id,
                    filter: self.register_filter(),
                };
                if self.author("save the filter", "filters.change_filter", &c) {
                    self.note = "the filter's conditions are these now".into();
                    self.history.replace_next();
                    return self.update(Message::Go(Page::Ledger(Ledger::Saved(id))));
                }
            }
            Msg::Delete(id) => {
                if self.author("delete the filter", "filters.delete_filter", &FilterId { id }) {
                    self.note = "the filter is gone".into();
                    return self.update(Message::Go(Page::Ledger(Ledger::All)));
                }
            }
        }
        Task::none()
    }

    /// One condition of the register's filter changed: the register for
    /// the conditions now, in place of this one in the history.
    pub fn edit_filter(&mut self, e: Edit) -> Task<Message> {
        let mut f = self.register_filter();
        match e {
            Edit::Account(a) => match f.accounts.iter().position(|x| *x == a) {
                Some(i) => drop(f.accounts.remove(i)),
                None => f.accounts.push(a),
            },
            Edit::AllAccounts => (f.accounts, f.on_budget) = (Vec::new(), false),
            Edit::OnBudget(on) => f.on_budget = on,
            Edit::Text(t) => f.text = t,
            Edit::Category(c) => match f.categories.iter().position(|x| *x == c) {
                Some(i) => drop(f.categories.remove(i)),
                None => f.categories.push(c),
            },
            Edit::AnyCategory => f.categories.clear(),
            Edit::Direction(d) => f.direction = d,
            Edit::NoTransfers(no) => f.no_transfers = no,
            Edit::NeedsLook(on) => f.needs_look = on,
            Edit::Dates(from, through) => (f.from, f.through) = (from, through),
        }
        // The text as typed, spaces and all, while it is typed; the rest
        // in order.
        let text = f.text.clone();
        let mut f = f.tidy();
        f.text = text;
        let base = match self.peer.ledger {
            Ledger::Adhoc => self.peer.base,
            l => l,
        };
        let ledger = match base {
            Ledger::Saved(id) if self.peer.saved(id).is_some_and(|s| s.filter == f.tidy()) => base,
            _ => Ledger::built_in(&f.tidy()).unwrap_or(Ledger::Adhoc),
        };
        self.peer.adhoc = f;
        self.peer.base = base;
        self.history.replace_next();
        self.update(Message::Go(Page::Ledger(ledger)))
    }

    /// The register for `f`, not saved, changed from `base`: a formula's
    /// term opened for its month, say.
    pub fn open_filter(&mut self, f: Filter, base: Ledger) -> Task<Message> {
        let ledger = Ledger::built_in(&f).unwrap_or(Ledger::Adhoc);
        self.peer.adhoc = f;
        self.peer.base = base;
        self.update(Message::Go(Page::Ledger(ledger)))
    }

    /// A chip's menu at the pointer; the categories' is the dropdown.
    fn chip(&mut self, c: Chip) -> Task<Message> {
        let f = self.register_filter();
        let edit = |e: Edit| Message::Filter(Msg::Edit(e));
        let tick = |on: bool| if on { glyphs::TICKED } else { glyphs::UNTICK };
        let (title, entries): (&str, Vec<menu::Entry<Message>>) = match c {
            Chip::Categories => {
                let at = (
                    iced::Point::new(self.pointer.x.min(self.window.width - crate::DROP_WIDTH), self.pointer.y + 14.0),
                    false,
                );
                return self.open_dropdown(Choosing::Filter, at);
            }
            Chip::Accounts => {
                let mut entries = vec![
                    menu::Entry::run(tick(f.accounts.is_empty() && !f.on_budget), "Every account", edit(Edit::AllAccounts)),
                    menu::Entry::run(tick(f.on_budget), "Only on the budget", edit(Edit::OnBudget(!f.on_budget))),
                ];
                for a in self.peer.accounts.rows.iter() {
                    entries.push(menu::Entry::run(
                        tick(f.accounts.contains(&a.id)),
                        a.name.clone(),
                        edit(Edit::Account(a.id)),
                    ));
                }
                ("Accounts", entries)
            }
            Chip::Direction => (
                "Money",
                vec![
                    menu::Entry::run(
                        tick(f.direction == Direction::Either),
                        "In and out",
                        edit(Edit::Direction(Direction::Either)),
                    ),
                    menu::Entry::run(tick(f.direction == Direction::In), "Money in", edit(Edit::Direction(Direction::In))),
                    menu::Entry::run(tick(f.direction == Direction::Out), "Money out", edit(Edit::Direction(Direction::Out))),
                ],
            ),
            Chip::Transfers => (
                "Transfers",
                vec![
                    menu::Entry::run(tick(!f.no_transfers), "Transfers in", edit(Edit::NoTransfers(false))),
                    menu::Entry::run(tick(f.no_transfers), "Transfers left out", edit(Edit::NoTransfers(true))),
                ],
            ),
            Chip::Dates => {
                let this = month_of(&self.today);
                let month = |m: &str| (Some(first_day(m)), Some(last_day(m)));
                let year = &self.today[..4];
                let mut presets = vec![("Any time".to_string(), (None, None))];
                presets.push((month_name(&this), month(&this)));
                presets.push((month_name(&add_months(&this, -1)), month(&add_months(&this, -1))));
                if self.peer.month != this && self.peer.month != add_months(&this, -1) {
                    presets.push((month_name(&self.peer.month), month(&self.peer.month)));
                }
                presets.push((
                    "The last three months".into(),
                    (Some(first_day(&add_months(&this, -2))), Some(last_day(&this))),
                ));
                presets.push((format!("{year} so far"), (Some(format!("{year}-01-01")), None)));
                let entries = presets
                    .into_iter()
                    .map(|(label, (from, through))| {
                        let on = f.from == from && f.through == through;
                        menu::Entry::run(tick(on), label, edit(Edit::Dates(from, through)))
                    })
                    .collect();
                ("Dates", entries)
            }
        };
        self.ctx
            .open_menu(menu::Menu::open(title, entries, self.pointer, self.window, menu::Anchor::Pointer));
        Task::none()
    }

    /// What the register on screen is called, and what it says under that.
    pub fn register_title(&self) -> (String, String) {
        let account_line = |a: &AccountLine| match a.bank.is_empty() {
            false => format!(
                "{} \u{2022}\u{2022}{} \u{2014} linked through Plaid{}",
                a.kind,
                a.mask,
                a.bank_balance
                    .map(|b| format!(", the bank says {}", crate::amount::money(b)))
                    .unwrap_or_default()
            ),
            true => format!("{} \u{2014} {}", a.kind, if a.on_budget { "on budget" } else { "tracking" }),
        };
        let of = |l: Ledger| match l {
            Ledger::All => ("All Accounts".to_string(), "Every transaction in the household".to_string()),
            Ledger::Inbox => ("Needs a Look".into(), "Imported and not yet approved, or for no category".into()),
            Ledger::Account(id) => self.peer.account(id).map(|a| (a.name.clone(), account_line(a))).unwrap_or_default(),
            Ledger::Saved(id) => self.peer.saved(id).map(|f| (f.name.clone(), f.said.clone())).unwrap_or_default(),
            Ledger::Adhoc => ("Filtered".into(), String::new()),
        };
        match self.peer.ledger {
            Ledger::Adhoc => {
                let (base, _) = of(self.peer.base);
                (
                    format!("{base}, filtered"),
                    "Not saved \u{2014} \u{201c}Save as filter\u{2026}\u{201d} keeps it in the sidebar".into(),
                )
            }
            l => of(l),
        }
    }

    /// The bar: the conditions, each a chip, set ones lit; what the
    /// register adds up to; and keeping it.
    pub fn view_filter_bar(&self) -> Element<'_, Message> {
        let f = self.register_filter();
        let chip = |label: String, set: bool, m: Message| -> Element<'_, Message> {
            button(text(label).size(12).wrapping(text::Wrapping::None))
                .on_press(m)
                .padding([4, 10])
                .style(look::choice(set))
                .into()
        };
        let open = |c: Chip| Message::Filter(Msg::Chip(c));
        let accounts = match (&f.accounts[..], f.on_budget) {
            ([], false) => "Every account".to_string(),
            ([], true) => "Accounts on budget".into(),
            ([a], _) => self.peer.account(*a).map_or_else(|| "An account".into(), |a| a.name.clone()),
            (all, _) => format!("{} accounts", all.len()),
        };
        let categories = match &f.categories[..] {
            [] => "Any category".to_string(),
            [None] => "No category".into(),
            [Some(c)] if *c == money_domain::budget::ready_to_assign() => "Ready to Assign".into(),
            [Some(c)] => self.peer.line(*c).map_or_else(|| "A category".into(), |l| l.name.clone()),
            all => format!("{} categories", all.len()),
        };
        let direction = match f.direction {
            Direction::Either => "In and out",
            Direction::In => "Money in",
            Direction::Out => "Money out",
        };
        let dates = match (&f.from, &f.through) {
            (None, None) => "Any time".to_string(),
            (Some(a), Some(b)) if a.len() == 10 && a.ends_with("-01") && *b == last_day(&a[..7]) => month_name(&a[..7]),
            (Some(a), Some(b)) => format!("{a} to {b}"),
            (Some(a), None) => format!("From {a}"),
            (None, Some(b)) => format!("Through {b}"),
        };
        let search = text_input("Payee or bank text", &f.text)
            .id(look::FILTER_TEXT)
            .on_input(|t| Message::Filter(Msg::Edit(Edit::Text(t))))
            .size(12)
            .padding([4, 8])
            .width(Length::Fixed(160.0))
            .style(look::input);
        let mut chips = row![icon::faint(glyphs::SEARCH, false).width(14.0).height(14.0)]
            .spacing(6)
            .align_y(Alignment::Center);
        chips = chips
            .push(chip(accounts, !f.accounts.is_empty() || f.on_budget, open(Chip::Accounts)))
            .push(search)
            .push(chip(categories, !f.categories.is_empty(), open(Chip::Categories)))
            .push(chip(direction.into(), f.direction != Direction::Either, open(Chip::Direction)))
            .push(chip(
                if f.no_transfers { "No transfers" } else { "With transfers" }.into(),
                f.no_transfers,
                open(Chip::Transfers),
            ))
            .push(chip(
                "Needs a look".into(),
                f.needs_look,
                Message::Filter(Msg::Edit(Edit::NeedsLook(!f.needs_look))),
            ))
            .push(chip(dates, f.from.is_some() || f.through.is_some(), open(Chip::Dates)));
        chips.into()
    }

    /// What the register adds up to, as its title's figure: the sum, and
    /// how many.
    pub fn view_filter_total(&self) -> Element<'_, Message> {
        let t = self.peer.total();
        let n = t.count;
        column![
            bold(crate::amount::money(t.total), 16).style(move |theme| text::Style {
                color: Some(match t.total < 0 {
                    true => arkui::theme::of(theme).danger.base.color,
                    false => arkui::theme::of(theme).background.base.text,
                }),
            }),
            dim(format!("{n} transaction{}", if n == 1 { "" } else { "s" }), 11),
        ]
        .spacing(1)
        .align_x(Alignment::End)
        .into()
    }

    /// Keeping the register's filter: "Save as filter…" for one not saved,
    /// with saving into the saved one it was changed from; renaming and
    /// deleting a saved one.
    pub fn filter_tools(&self) -> Vec<Element<'_, Message>> {
        let mut out = Vec::new();
        match (self.peer.ledger, self.register_saved()) {
            (Ledger::Adhoc, saved) => {
                if let Some(s) = saved.and_then(|id| self.peer.saved(id)) {
                    out.push(quiet(&format!("Save to {}", s.name), Message::Filter(Msg::SaveChanges)));
                }
                out.push(action("Save as filter\u{2026}", Message::Filter(Msg::SaveAs)));
            }
            (Ledger::Saved(id), _) => {
                out.push(quiet("Rename", Message::Rename(crate::Naming::Filter(id))));
                out.push(link("Delete filter", Message::Filter(Msg::Delete(id))));
            }
            _ => {}
        }
        out
    }

    /// "Save as filter": its name, typed.
    pub fn view_save_filter<'a>(&'a self, name: &'a str) -> Element<'a, Message> {
        let f = self.register_filter();
        let mut c = column![
            bold("Save as filter", 16),
            dim(
                "It is kept in the sidebar, under Filters, with what it adds up to, and a formula can sum it.",
                11
            ),
            text_input("Its name, as ACME paychecks", name)
                .id(look::FILTER_NAME)
                .on_input(|t| Message::Filter(Msg::Name(t)))
                .on_submit(Message::Filter(Msg::SaveDone))
                .size(13)
                .padding([6, 8])
                .style(look::input),
        ]
        .spacing(10);
        if f == Filter::all() {
            c = c.push(text("Every transaction: All Accounts is this already.").size(11).style(style::dim));
        }
        c.push(
            row![
                fill(),
                quiet("Cancel", Message::CloseModal),
                action("Save", Message::Filter(Msg::SaveDone))
            ]
            .spacing(8),
        )
        .into()
    }
}
