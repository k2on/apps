//! What the window draws: the sidebar of accounts, and beside it the page —
//! the month's budget and its inspector, an account's register, or where
//! the month's money went — with a panel over it when one is open.

use arkui::{glyphs, icon, layer, panel, style, table};
use iced::widget::{button, column, container, mouse_area, progress_bar, row, rule, scrollable, stack, text, text_input, Column, Space};
use iced::{Alignment, Element, Length, Padding};
use money_domain::budget::ready_to_assign;
use money_domain::dates::{month_name, month_of};
use money_domain::*;

use crate::amount::{money, money_or_blank};
use crate::look::{self, Tone};
use crate::peer::Ledger;
use crate::{App, Choice, Choosing, Field, Message, Modal, Page, TxnForm};

const SIDEBAR: f32 = 250.0;

/// A section of the sidebar's accounts: its heading, and which belong.
type Section = (&'static str, fn(&AccountLine) -> bool);
const INSPECTOR: f32 = 310.0;
const PAD: f32 = 18.0;
const MONEY_COL: f32 = 120.0;
/// Where the figures of a money column start: inside the assigned cell's
/// button padding, and inside an available pill's.
const ASSIGNED_INSET: f32 = 6.0;
const PILL_INSET: f32 = 9.0;

/// A heading of a money column, in from its edge as the figures under it are.
fn inset<'a>(heading: Element<'a, Message>, by: f32) -> Element<'a, Message> {
    container(heading)
        .width(Length::Fixed(MONEY_COL))
        .padding(Padding { left: by, ..Padding::ZERO })
        .into()
}

fn bold<'a>(s: impl Into<String>, size: u32) -> iced::widget::Text<'a> {
    text(s.into()).size(size).font(arkui::hero::BOLD)
}

fn dim<'a>(s: impl Into<String>, size: u32) -> iced::widget::Text<'a> {
    text(s.into()).size(size).style(style::dim)
}

fn fill() -> Space {
    Space::new().width(Length::Fill)
}

/// A button of the accent: something somebody is meant to press.
fn action<'a>(label: &str, msg: Message) -> Element<'a, Message> {
    button(text(label.to_string()).size(13))
        .on_press(msg)
        .padding([5, 12])
        .style(style::action)
        .into()
}

/// A button that is a word until pointed at.
fn quiet<'a>(label: &str, msg: Message) -> Element<'a, Message> {
    button(text(label.to_string()).size(13))
        .on_press(msg)
        .padding([5, 10])
        .style(look::choice(false))
        .into()
}

fn link<'a>(label: impl Into<String>, msg: Message) -> Element<'a, Message> {
    button(text(label.into()).size(12)).on_press(msg).padding(0).style(look::link).into()
}

fn glyph_button<'a>(shape: &'static [u8], msg: Message) -> Element<'a, Message> {
    button(icon::plain(shape, false)).on_press(msg).padding(4).style(style::quiet).into()
}

/// A line of the sidebar: a glyph, a label, what hangs off the right.
fn entry<'a>(glyph: Option<&'static [u8]>, label: String, trailing: Option<Element<'a, Message>>, lit: bool, msg: Message) -> Element<'a, Message> {
    let mut line = row![].spacing(8).align_y(Alignment::Center);
    if let Some(g) = glyph {
        line = line.push(icon::line(g, lit));
    }
    line = line.push(text(label).size(13).width(Length::Fill).style(move |theme| text::Style {
        color: Some(style::entry_text(theme, lit)),
    }));
    if let Some(t) = trailing {
        line = line.push(t);
    }
    mouse_area(
        container(line)
            .width(Length::Fill)
            .padding([5, 10])
            .style(move |theme| style::entry(theme, lit, true)),
    )
    .on_press(msg)
    .into()
}

/// An amount as the sidebar shows one: red below nothing, and in the lit
/// entry's own colour on it.
fn balance<'a>(cents: i64, lit: bool) -> Element<'a, Message> {
    text(money(cents))
        .size(12)
        .style(move |theme| text::Style {
            color: Some(match (lit, cents < 0) {
                (true, _) => style::entry_text(theme, true),
                (false, true) => arkui::theme::of(theme).danger.base.color,
                (false, false) => style::entry_text(theme, false).scale_alpha(0.75),
            }),
        })
        .into()
}

fn heading<'a>(label: &str, total: Option<i64>) -> Element<'a, Message> {
    let mut r = row![text(label.to_uppercase()).size(10).style(style::faint(0.5)), fill()].align_y(Alignment::Center);
    if let Some(t) = total {
        r = r.push(text(money(t)).size(10).style(style::faint(0.5)));
    }
    container(r)
        .padding(Padding {
            top: 14.0,
            right: 10.0,
            bottom: 4.0,
            left: 10.0,
        })
        .into()
}

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let page: Element<'_, Message> = match self.page {
            Page::Budget => self.view_budget(),
            Page::Reflect => self.view_reflect(),
            Page::Ledger(l) => self.view_ledger(l),
        };
        let base = container(row![self.view_sidebar(), page].height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::page);
        let mut layers = stack![base];
        if let Some(m) = &self.modal {
            layers = layers
                .push(layer::dimmed(Message::CloseModal, Message::Swallow))
                .push(layer::centred(self.view_modal(m)));
        }
        layers.into()
    }

    // * The sidebar

    fn view_sidebar(&self) -> Element<'_, Message> {
        let accounts = &self.peer.accounts.rows;
        let inbox = self.peer.inbox.rows.len();
        let mut side = column![
            container(column![bold("money", 20), dim("The household's budget, shared by everyone in it", 11)].spacing(2)).padding([4, 10]),
            Space::new().height(10),
            entry(
                Some(glyphs::HOME),
                "Budget".into(),
                None,
                self.page == Page::Budget,
                Message::Go(Page::Budget)
            ),
            entry(
                Some(glyphs::LIBRARY),
                "Reflect".into(),
                None,
                self.page == Page::Reflect,
                Message::Go(Page::Reflect)
            ),
            entry(
                Some(glyphs::PLAYLIST),
                "All Accounts".into(),
                (inbox > 0).then(|| self.badge(inbox, self.page == Page::Ledger(Ledger::All))),
                self.page == Page::Ledger(Ledger::All) || self.page == Page::Ledger(Ledger::Inbox),
                Message::Go(Page::Ledger(Ledger::All)),
            ),
        ]
        .spacing(1);
        let sections: [Section; 3] = [
            ("Budget", |a| a.on_budget && !a.closed),
            ("Tracking", |a| !a.on_budget && !a.closed),
            ("Closed", |a| a.closed),
        ];
        for (name, keep) in sections {
            let these: Vec<&AccountLine> = accounts.iter().filter(|a| keep(a)).collect();
            if these.is_empty() {
                continue;
            }
            let total = (name != "Closed").then(|| these.iter().map(|a| a.balance).sum());
            side = side.push(heading(name, total));
            for a in these {
                let lit = self.page == Page::Ledger(Ledger::Account(a.id));
                let label = match a.mask.is_empty() {
                    true => a.name.clone(),
                    false => format!("{} \u{2022}\u{2022}{}", a.name, a.mask),
                };
                side = side.push(entry(
                    None,
                    label,
                    Some(balance(a.balance, lit)),
                    lit,
                    Message::Go(Page::Ledger(Ledger::Account(a.id))),
                ));
            }
        }
        side = side.push(container(link("+ Add Account", Message::AddAccount)).padding([10, 10]));

        let bank = self.peer.banks.rows.first();
        let synced = bank.map_or("not linked yet".to_string(), |b| {
            format!(
                "{} \u{2014} {} account{} through Plaid",
                b.institution,
                b.accounts,
                if b.accounts == 1 { "" } else { "s" }
            )
        });
        let foot = column![
            row![
                column![text("Bank sync").size(12), dim(synced, 11)].spacing(2).width(Length::Fill),
                quiet("Sync", Message::Sync)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            dim(self.note.clone(), 11),
        ]
        .spacing(8);

        container(column![
            scrollable(side.padding(Padding {
                top: PAD,
                right: 10.0,
                bottom: 10.0,
                left: 10.0,
            }))
            .height(Length::Fill)
            .style(style::bars),
            container(foot).padding([12, 20]),
        ])
        .width(Length::Fixed(SIDEBAR))
        .height(Length::Fill)
        .style(style::surface)
        .into()
    }

    /// A count in a bubble: what needs looking at.
    fn badge<'a>(&self, n: usize, lit: bool) -> Element<'a, Message> {
        container(text(n.to_string()).size(10).style(move |theme| text::Style {
            color: Some(match lit {
                true => arkui::theme::of(theme).primary.base.color,
                false => arkui::theme::of(theme).primary.base.text,
            }),
        }))
        .padding([1, 6])
        .style(move |theme| {
            let p = arkui::theme::of(theme);
            container::Style {
                background: Some(iced::Background::Color(match lit {
                    true => p.primary.base.text,
                    false => p.primary.base.color,
                })),
                border: iced::Border {
                    radius: 8.0.into(),
                    ..iced::Border::default()
                },
                ..container::Style::default()
            }
        })
        .into()
    }

    // * The budget

    fn view_budget(&self) -> Element<'_, Message> {
        let s = self.peer.summary();
        let this = month_of(&self.today);
        let mut month = row![
            glyph_button(glyphs::BACK, Message::Month(-1)),
            bold(month_name(&self.peer.month), 22),
            glyph_button(glyphs::CHEVRON, Message::Month(1)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if self.peer.month != this {
            month = month.push(link("Today", Message::ThisMonth));
        }
        let ready = container(
            row![column![
                bold(money(s.ready_to_assign), 22),
                text(match s.ready_to_assign {
                    r if r < 0 => "Assigned more than you have",
                    0 => "All money assigned",
                    _ => "Ready to Assign",
                })
                .size(12),
            ]
            .spacing(2),]
            .align_y(Alignment::Center),
        )
        .padding([10, 18])
        .style(look::ready(s.ready_to_assign));

        let mut notices = row![].spacing(14).align_y(Alignment::Center);
        let inbox = self.peer.inbox.rows.len();
        if inbox > 0 {
            notices = notices.push(link(
                format!(
                    "{inbox} transaction{} need{} a look \u{2192}",
                    if inbox == 1 { "" } else { "s" },
                    if inbox == 1 { "s" } else { "" }
                ),
                Message::Go(Page::Ledger(Ledger::Inbox)),
            ));
        }
        if s.overspent > 0 {
            notices = notices.push(text(format!("{} overspent", s.overspent)).size(12).style(Tone::Over.text()));
        }
        if s.underfunded > 0 {
            notices = notices.push(text(format!("{} underfunded", s.underfunded)).size(12).style(Tone::Short.text()));
        }

        let top = row![column![month, notices].spacing(6).width(Length::Fill), ready]
            .spacing(16)
            .align_y(Alignment::Center);

        let tools = row![
            link("+ Category Group", Message::NewGroup),
            link(
                if self.show_hidden {
                    "Hide hidden categories"
                } else {
                    "Show hidden categories"
                },
                Message::ShowHidden(!self.show_hidden)
            ),
        ]
        .spacing(18);

        let header = table::row(
            row![
                table::heading("CATEGORY", Length::Fill),
                inset(table::heading("ASSIGNED", Length::Fill), ASSIGNED_INSET),
                inset(table::heading("ACTIVITY", Length::Fill), ASSIGNED_INSET),
                inset(table::heading("AVAILABLE", Length::Fill), PILL_INSET),
            ]
            .align_y(Alignment::Center),
            false,
            false,
            false,
        );

        let mut body: Column<'_, Message> = Column::new();
        if let Some(name) = &self.new_group {
            body = body.push(
                container(
                    text_input("New category group", name)
                        .id(look::NEW_GROUP)
                        .on_input(Message::NewGroupTyped)
                        .on_submit(Message::NewGroupDone)
                        .size(13)
                        .padding([5, 8])
                        .style(look::input),
                )
                .padding([4, 8]),
            );
        }
        let mut odd = false;
        for g in &self.peer.groups.rows {
            if g.hidden && !self.show_hidden {
                continue;
            }
            let lines: Vec<&BudgetLine> = self
                .peer
                .lines
                .rows
                .iter()
                .filter(|l| l.group_id == g.id && (self.show_hidden || !l.hidden))
                .collect();
            body = body.push(self.group_row(g, &lines));
            if self.collapsed.contains(&g.id) {
                continue;
            }
            for l in lines {
                body = body.push(self.category_row(l, odd));
                odd = !odd;
            }
            if let Some((ng, name)) = &self.new_category {
                if *ng == g.id {
                    body = body.push(
                        container(
                            text_input("New category", name)
                                .id(look::NEW_CATEGORY)
                                .on_input(Message::NewCategoryTyped)
                                .on_submit(Message::NewCategoryDone)
                                .size(13)
                                .padding([5, 8])
                                .style(look::input),
                        )
                        .padding(Padding {
                            top: 2.0,
                            right: 8.0,
                            bottom: 2.0,
                            left: 32.0,
                        }),
                    );
                }
            }
        }

        let page = column![
            top,
            tools,
            column![
                header,
                rule::horizontal(1).style(style::hairline),
                scrollable(body).height(Length::Fill).style(style::bars)
            ],
        ]
        .spacing(12)
        .padding(PAD)
        .width(Length::Fill);

        row![page, self.view_inspector()].into()
    }

    fn group_row<'a>(&self, g: &GroupLine, lines: &[&BudgetLine]) -> Element<'a, Message> {
        let sum = |f: fn(&BudgetLine) -> i64| lines.iter().map(|l| f(l)).sum::<i64>();
        let collapsed = self.collapsed.contains(&g.id);
        let mut name = row![
            button(icon::faint(if collapsed { glyphs::CHEVRON } else { glyphs::DOWN }, false))
                .on_press(Message::Collapse(g.id))
                .padding(2)
                .style(style::quiet),
            bold(g.name.clone(), 13),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        name = name.push(
            button(icon::faint(glyphs::PLUS, false))
                .on_press(Message::NewCategory(g.id))
                .padding(2)
                .style(style::quiet),
        );
        let total = |c: i64, inset: f32| {
            container(text(money(c)).size(12).font(arkui::hero::BOLD))
                .width(Length::Fixed(MONEY_COL))
                .padding(Padding {
                    left: inset,
                    ..Padding::ZERO
                })
        };
        container(
            row![
                container(name).width(Length::Fill),
                total(sum(|l| l.assigned), ASSIGNED_INSET),
                total(sum(|l| l.activity), ASSIGNED_INSET),
                total(sum(|l| l.available), PILL_INSET),
            ]
            .align_y(Alignment::Center),
        )
        .padding([6, 4])
        .style(|theme| container::Style {
            background: Some(iced::Background::Color(arkui::theme::of(theme).background.base.text.scale_alpha(0.06))),
            ..container::Style::default()
        })
        .into()
    }

    fn category_row<'a>(&'a self, l: &'a BudgetLine, odd: bool) -> Element<'a, Message> {
        let selected = self.selected == Some(l.category_id);
        let tone = Tone::of(l.available, l.target, l.assigned);
        let mut name = column![text(l.name.clone()).size(13).style(move |theme| text::Style {
            color: Some(
                arkui::theme::of(theme)
                    .background
                    .base
                    .text
                    .scale_alpha(if l.hidden { 0.45 } else { 1.0 })
            ),
        })]
        .spacing(3);
        if l.target > 0 {
            let funded = (l.assigned as f32 / l.target as f32).clamp(0.0, 1.0);
            name = name.push(
                progress_bar(0.0..=1.0, funded)
                    .length(Length::Fixed(160.0))
                    .girth(Length::Fixed(4.0))
                    .style(look::progress(tone)),
            );
        }
        let assigned: Element<'a, Message> = match &self.assigning {
            Some((c, typed)) if *c == l.category_id => text_input("0.00", typed)
                .id(look::ASSIGNED)
                .on_input(Message::AssignedTyped)
                .on_submit(Message::AssignedDone)
                .size(13)
                .padding([3, 6])
                .width(Length::Fixed(MONEY_COL - 14.0))
                .style(look::input)
                .into(),
            _ => button(text(money(l.assigned)).size(13))
                .on_press(Message::EditAssigned(l.category_id))
                .padding([3, 6])
                .style(style::quiet)
                .into(),
        };
        let activity = text(money(l.activity)).size(13).style(style::dim);
        let available = container(text(money(l.available)).size(12)).padding([2, 9]).style(look::pill(tone));
        let line = row![
            container(name).width(Length::Fill).padding(Padding { left: 28.0, ..Padding::ZERO }),
            container(assigned).width(Length::Fixed(MONEY_COL)),
            container(activity).width(Length::Fixed(MONEY_COL)).padding(Padding {
                left: ASSIGNED_INSET,
                ..Padding::ZERO
            }),
            container(available).width(Length::Fixed(MONEY_COL)),
        ]
        .align_y(Alignment::Center);
        mouse_area(table::row(line, selected, false, odd).padding([5, 4]))
            .on_press(Message::Select(Some(l.category_id)))
            .into()
    }

    // * The inspector

    fn view_inspector(&self) -> Element<'_, Message> {
        let body = match self.selected.and_then(|c| self.peer.line(c)) {
            Some(l) => self.inspect_category(l),
            None => self.inspect_month(),
        };
        container(scrollable(body.spacing(12).padding(PAD)).height(Length::Fill).style(style::bars))
            .width(Length::Fixed(INSPECTOR))
            .height(Length::Fill)
            .style(style::surface)
            .into()
    }

    fn figure<'a>(label: &str, cents: i64) -> Element<'a, Message> {
        row![text(label.to_string()).size(12).width(Length::Fill), text(money(cents)).size(12)]
            .align_y(Alignment::Center)
            .into()
    }

    fn inspect_month(&self) -> Column<'_, Message> {
        let s = self.peer.summary();
        let lines = &self.peer.lines.rows;
        let carried: i64 = lines.iter().map(|l| l.carried).sum();
        let name = month_name(&self.peer.month);
        let summary = container(
            column![
                bold(format!("{name}'s Summary"), 14),
                rule::horizontal(1).style(style::hairline),
                App::figure("Left over from last month", carried),
                App::figure("Assigned this month", s.assigned),
                App::figure("Activity", s.activity),
                App::figure("Available", s.available),
                rule::horizontal(1).style(style::hairline),
                App::figure("Income this month", s.income),
                App::figure("Overspent last month", s.overspent_last_month),
            ]
            .spacing(8),
        )
        .padding(14)
        .style(look::card);
        let auto = container(
            column![
                bold("Auto-Assign", 14),
                dim("Every category at once, for this month.", 11),
                quiet("Underfunded", Message::Auto(Auto::Underfunded, vec![])),
                quiet("Assigned last month", Message::Auto(Auto::LastMonth, vec![])),
                quiet("Reset assigned amounts", Message::Auto(Auto::Reset, vec![])),
            ]
            .spacing(8),
        )
        .padding(14)
        .style(look::card);
        let shared = container(
            column![
                bold("One budget, one household", 14),
                dim(
                    "Everyone your household's sign-in lets in sees and changes this same budget, \
                     on every device, offline or not. Select a category to see its story.",
                    11
                ),
            ]
            .spacing(6),
        )
        .padding(14)
        .style(look::card);
        column![summary, auto, shared]
    }

    fn inspect_category<'a>(&'a self, l: &'a BudgetLine) -> Column<'a, Message> {
        let tone = Tone::of(l.available, l.target, l.assigned);
        let head = container(
            column![
                row![
                    column![bold(l.name.clone(), 16), dim(l.group.clone(), 11)].spacing(2).width(Length::Fill),
                    glyph_button(glyphs::CLOSE, Message::Select(None)),
                ]
                .align_y(Alignment::Center),
                row![
                    text("Available Balance").size(12).width(Length::Fill),
                    container(text(money(l.available)).size(14)).padding([3, 10]).style(look::pill(tone)),
                ]
                .align_y(Alignment::Center),
                rule::horizontal(1).style(style::hairline),
                App::figure("Cash left over from last month", l.carried),
                App::figure("Assigned this month", l.assigned),
                App::figure("Cash spending", l.activity),
                if l.available < 0 {
                    action("Cover overspending", Message::MoveMoney)
                } else {
                    quiet("Move money", Message::MoveMoney)
                },
            ]
            .spacing(8),
        )
        .padding(14)
        .style(look::card);

        let mut target = column![bold("Target", 14)].spacing(8);
        if l.target > 0 {
            let have = l.assigned;
            let funded = (have as f32 / l.target as f32).clamp(0.0, 1.0);
            target = target.push(
                row![
                    text(format!("{} a month", money(l.target))).size(12).width(Length::Fill),
                    text(format!("{:.0}%", funded * 100.0)).size(12).style(tone.text()),
                ]
                .align_y(Alignment::Center),
            );
            target = target.push(progress_bar(0.0..=1.0, funded).girth(Length::Fixed(6.0)).style(look::progress(tone)));
            if have < l.target {
                let need = l.target - have;
                target = target.push(action(
                    &format!("Assign {} to meet target", money(need)),
                    Message::AssignTo(l.category_id, l.target),
                ));
            } else {
                target = target.push(text("Funded this month").size(12).style(Tone::Funded.text()));
            }
        } else {
            target = target.push(dim("How much does it need each month?", 11));
        }
        target = target.push(
            row![
                text_input("Monthly target", &self.target)
                    .on_input(Message::TargetTyped)
                    .on_submit(Message::TargetDone)
                    .size(13)
                    .padding([5, 8])
                    .style(look::input),
                quiet("Save", Message::TargetDone),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
        let target = container(target).padding(14).style(look::card);

        let auto = container(
            column![
                bold("Auto-Assign", 14),
                self.auto_line("Assigned last month", l, l.assigned_last_month),
                self.auto_line("Spent last month", l, -l.spent_last_month),
                self.auto_line("Average spent", l, -l.average_spent),
            ]
            .spacing(8),
        )
        .padding(14)
        .style(look::card);

        let hidden = l.hidden;
        let manage = row![
            link(if hidden { "Unhide" } else { "Hide" }, Message::HideCategory(l.category_id, !hidden)),
            link("Delete category", Message::DeleteCategory(l.category_id)),
        ]
        .spacing(16);
        column![head, target, auto, manage]
    }

    fn auto_line<'a>(&self, label: &str, l: &BudgetLine, cents: i64) -> Element<'a, Message> {
        button(row![text(label.to_string()).size(12).width(Length::Fill), text(money(cents)).size(12)].align_y(Alignment::Center))
            .on_press(Message::AssignTo(l.category_id, cents.max(0)))
            .padding([5, 8])
            .width(Length::Fill)
            .style(look::choice(false))
            .into()
    }

    // * The registers

    fn view_ledger(&self, ledger: Ledger) -> Element<'_, Message> {
        let rows = &self.peer.register.rows;
        let all = !matches!(ledger, Ledger::Account(_));
        let (title, subtitle, account) = match ledger {
            Ledger::Account(id) => {
                let a = self.peer.account(id);
                let name = a.map(|a| a.name.clone()).unwrap_or_default();
                let sub = match a {
                    Some(a) if !a.bank.is_empty() => format!(
                        "{} \u{2022}\u{2022}{} \u{2014} linked through Plaid{}",
                        a.kind,
                        a.mask,
                        a.bank_balance.map(|b| format!(", the bank says {}", money(b))).unwrap_or_default()
                    ),
                    Some(a) => format!("{} \u{2014} {}", a.kind, if a.on_budget { "on budget" } else { "tracking" }),
                    None => String::new(),
                };
                (name, sub, a)
            }
            Ledger::All => ("All Accounts".into(), "Every transaction in the household".into(), None),
            Ledger::Inbox => ("Needs a Look".into(), "Imported and not yet approved, or for no category".into(), None),
        };

        let balances: Element<'_, Message> = match account {
            Some(a) => row![
                self.balance_box("Cleared Balance", a.cleared),
                text("+").size(16).style(style::dim),
                self.balance_box("Uncleared Balance", a.uncleared),
                text("=").size(16).style(style::dim),
                self.balance_box("Working Balance", a.balance),
            ]
            .spacing(14)
            .align_y(Alignment::Center)
            .into(),
            None => {
                let on: i64 = self.peer.accounts.rows.iter().filter(|a| a.on_budget).map(|a| a.balance).sum();
                row![self.balance_box("On Budget", on)].into()
            }
        };

        let unapproved: Vec<Id<Txn>> = rows.iter().filter(|t| !t.approved).map(|t| t.id).collect();
        let mut tools = row![action("+ Add Transaction", Message::AddTxn)].spacing(10).align_y(Alignment::Center);
        if let Some(a) = account {
            tools = tools.push(quiet("Reconcile", Message::Reconcile(a.id)));
            if !a.bank.is_empty() {
                tools = tools.push(quiet("Sync", Message::Sync));
            }
        }
        if !unapproved.is_empty() {
            tools = tools.push(quiet(&format!("Approve all ({})", unapproved.len()), Message::Approve(unapproved)));
        }
        if all {
            tools = tools.push(fill());
            tools = tools.push(
                button(text("All").size(12))
                    .on_press(Message::Go(Page::Ledger(Ledger::All)))
                    .padding([4, 10])
                    .style(look::choice(ledger == Ledger::All)),
            );
            tools = tools.push(
                button(text(format!("Needs a look ({})", self.peer.inbox.rows.len())).size(12))
                    .on_press(Message::Go(Page::Ledger(Ledger::Inbox)))
                    .padding([4, 10])
                    .style(look::choice(ledger == Ledger::Inbox)),
            );
        }

        let w = |n: f32| Length::Fixed(n);
        let mut head = row![].align_y(Alignment::Center);
        if all {
            head = head.push(table::heading("ACCOUNT", w(130.0)));
        }
        head = head
            .push(table::heading("DATE", w(96.0)))
            .push(table::heading("PAYEE", Length::FillPortion(3)))
            .push(table::heading("CATEGORY", Length::FillPortion(3)))
            .push(table::heading("MEMO", Length::FillPortion(2)))
            .push(table::heading("OUTFLOW", w(100.0)))
            .push(table::heading("INFLOW", w(100.0)))
            .push(table::heading("", w(90.0)));

        let mut body: Column<'_, Message> = Column::new();
        if let Some(f) = self.form.as_ref().filter(|f| f.editing.is_none()) {
            body = body.push(self.view_form(f, all));
        }
        for (i, t) in rows.iter().enumerate() {
            match &self.form {
                Some(f) if f.editing == Some(t.id) => body = body.push(self.view_form(f, all)),
                _ => body = body.push(self.txn_row(t, all, i % 2 == 1)),
            }
        }
        if rows.is_empty() {
            body = body.push(container(dim("Nothing here.", 13)).padding(20));
        }

        column![
            column![bold(title, 22), dim(subtitle, 12)].spacing(2),
            balances,
            tools,
            column![
                table::row(head, false, false, false),
                rule::horizontal(1).style(style::hairline),
                scrollable(body).height(Length::Fill).style(style::bars),
            ],
        ]
        .spacing(14)
        .padding(PAD)
        .width(Length::Fill)
        .into()
    }

    fn balance_box<'a>(&self, label: &str, cents: i64) -> Element<'a, Message> {
        column![
            bold(money(cents), 16).style(move |theme| text::Style {
                color: Some(match cents < 0 {
                    true => arkui::theme::of(theme).danger.base.color,
                    false => arkui::theme::of(theme).background.base.text,
                }),
            }),
            dim(label.to_string(), 11),
        ]
        .spacing(1)
        .into()
    }

    fn txn_row<'a>(&'a self, t: &'a TxnLine, all: bool, odd: bool) -> Element<'a, Message> {
        let w = |n: f32| Length::Fixed(n);
        let mut line = row![].align_y(Alignment::Center);
        if all {
            line = line.push(table::cell(t.account.clone(), w(130.0), false, false, true));
        }
        let needs = t.category == "Needs a category";
        let category: Element<'a, Message> = match (needs, t.transfer) {
            (true, _) => container(
                button(text("Needs a category").size(13))
                    .on_press(Message::Categorize(vec![t.id]))
                    .padding(0)
                    .style(|theme, status| {
                        let mut s = look::link(theme, status);
                        s.text_color = arkui::theme::of(theme).warning.base.color;
                        s
                    }),
            )
            .width(Length::FillPortion(3))
            .into(),
            _ => table::cell(
                t.category.clone(),
                Length::FillPortion(3),
                false,
                false,
                t.category_id == Some(ready_to_assign()),
            ),
        };
        line = line
            .push(table::cell(t.date.clone(), w(96.0), false, false, true))
            .push(table::cell(t.payee.clone(), Length::FillPortion(3), false, !t.approved, false))
            .push(category)
            .push(table::cell(t.memo.clone(), Length::FillPortion(2), false, false, true))
            .push(table::cell(money_or_blank((-t.amount).max(0)), w(100.0), false, false, false))
            .push(table::cell(money_or_blank(t.amount.max(0)), w(100.0), false, false, false));
        let (mark, next) = match t.cleared {
            Cleared::Uncleared => (glyphs::UNTICK, Some(true)),
            Cleared::Cleared => (glyphs::TICKED, Some(false)),
            Cleared::Reconciled => (glyphs::TICK, None),
        };
        let mut cleared = button(icon::tinted(mark, icon::SIZE, move |p| match t.cleared {
            Cleared::Uncleared => p.background.base.text.scale_alpha(0.35),
            Cleared::Cleared => p.success.base.color,
            Cleared::Reconciled => p.primary.base.color,
        }))
        .padding(2)
        .style(style::quiet);
        if let Some(c) = next {
            cleared = cleared.on_press(Message::ToggleCleared(t.id, c));
        }
        let mut end = row![cleared].spacing(4).align_y(Alignment::Center);
        if !t.approved {
            end = end.push(link("Approve", Message::Approve(vec![t.id])));
        }
        line = line.push(container(end).width(w(90.0)));
        mouse_area(table::row(line, false, false, odd).style(move |theme| {
            let mut s = style::row(theme, false, false, odd);
            if !t.approved {
                s.background = Some(iced::Background::Color(arkui::theme::of(theme).primary.base.color.scale_alpha(0.08)));
            }
            s
        }))
        .on_press(Message::EditTxn(t.id))
        .into()
    }

    /// The form a transaction is entered or corrected in, in the register's
    /// own columns.
    fn view_form<'a>(&'a self, f: &'a TxnForm, all: bool) -> Element<'a, Message> {
        let w = |n: f32| Length::Fixed(n);
        let field = |placeholder: &str, value: &'a str, which: Field| {
            text_input(placeholder, value)
                .on_input(move |v| Message::Form(which, v))
                .on_submit(Message::SaveTxn)
                .size(13)
                .padding([4, 6])
                .style(look::input)
        };
        let category_label = match (f.transfer_to, f.category) {
            (Some(a), _) => format!("Transfer : {}", self.peer.account(a).map(|a| a.name.as_str()).unwrap_or("?")),
            (None, Some(c)) if c == ready_to_assign() => "Ready to Assign".into(),
            (None, Some(c)) => self.peer.line(c).map(|l| l.name.clone()).unwrap_or_else(|| "a category".into()),
            (None, None) => "Choose a category".into(),
        };
        let mut line = row![].spacing(6).align_y(Alignment::Center);
        if all {
            let name = f.account.and_then(|a| self.peer.account(a)).map(|a| a.name.clone()).unwrap_or_default();
            line = line.push(container(text(name).size(12)).width(w(124.0)));
        }
        line = line
            .push(field("2026-10-26", &f.date, Field::Date).width(w(92.0)))
            .push(
                field(if f.transfer_to.is_some() { "(the transfer)" } else { "Payee" }, &f.payee, Field::Payee)
                    .id(crate::look::PAYEE)
                    .width(Length::FillPortion(3)),
            )
            .push(
                button(text(category_label).size(13))
                    .on_press(Message::ChooseCategory)
                    .padding([4, 6])
                    .width(Length::FillPortion(3))
                    .style(look::choice(false)),
            )
            .push(field("Memo", &f.memo, Field::Memo).width(Length::FillPortion(2)))
            .push(field("Outflow", &f.outflow, Field::Outflow).width(w(94.0)))
            .push(field("Inflow", &f.inflow, Field::Inflow).width(w(94.0)));
        let cleared = button(icon::tinted(
            if f.cleared { glyphs::TICKED } else { glyphs::UNTICK },
            icon::SIZE,
            move |p| match f.cleared {
                true => p.success.base.color,
                false => p.background.base.text.scale_alpha(0.4),
            },
        ))
        .on_press(Message::FormCleared(!f.cleared))
        .padding(2)
        .style(style::quiet);
        line = line.push(container(cleared).width(w(84.0)));

        let mut under = row![].spacing(10).align_y(Alignment::Center);
        if f.editing.is_none() && all {
            under = under.push(dim("Into", 12));
            for a in self.peer.accounts.rows.iter().filter(|a| !a.closed) {
                under = under.push(
                    button(text(a.name.clone()).size(12))
                        .on_press(Message::FormAccount(a.id))
                        .padding([3, 8])
                        .style(look::choice(f.account == Some(a.id))),
                );
            }
        }
        if !f.by.is_empty() {
            under = under.push(dim(format!("Entered by {}", f.by), 11));
        }
        under = under.push(fill());
        if let Some(id) = f.editing {
            under = under.push(link("Delete", Message::DeleteTxn(id)));
        }
        under = under.push(quiet("Cancel", Message::CancelTxn)).push(action("Save", Message::SaveTxn));

        container(column![line, under].spacing(8))
            .padding([8, 4])
            .style(|theme| container::Style {
                background: Some(iced::Background::Color(arkui::theme::of(theme).primary.base.color.scale_alpha(0.1))),
                ..container::Style::default()
            })
            .into()
    }

    // * Reflect

    fn view_reflect(&self) -> Element<'_, Message> {
        let s = self.peer.summary();
        let spending = &self.peer.spending.rows;
        let total: i64 = spending.iter().map(|g| g.spent).sum();
        let most = spending.first().map_or(1, |g| g.spent.max(1));
        let mut bars = column![].spacing(10);
        for g in spending {
            bars = bars.push(
                row![
                    text(g.group.clone()).size(13).width(Length::Fixed(160.0)),
                    progress_bar(0.0..=1.0, g.spent as f32 / most as f32)
                        .girth(Length::Fixed(14.0))
                        .style(look::progress(Tone::Funded)),
                    text(money(g.spent)).size(13).width(Length::Fixed(110.0)),
                    text(format!("{:.0}%", 100.0 * g.spent as f32 / total.max(1) as f32))
                        .size(12)
                        .style(style::dim)
                        .width(Length::Fixed(48.0)),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            );
        }
        if spending.is_empty() {
            bars = bars.push(dim("Nothing spent this month yet.", 13));
        }
        let month = row![
            glyph_button(glyphs::BACK, Message::Month(-1)),
            bold(format!("Spending in {}", month_name(&self.peer.month)), 22),
            glyph_button(glyphs::CHEVRON, Message::Month(1)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let tiles = row![
            self.tile("Income", s.income, Tone::Funded),
            self.tile("Spent", total, Tone::Zero),
            self.tile("Net", s.income - total, if s.income >= total { Tone::Funded } else { Tone::Over }),
        ]
        .spacing(12);
        column![month, tiles, container(bars).padding(16).width(Length::Fill).style(look::card)]
            .spacing(16)
            .padding(PAD)
            .width(Length::Fill)
            .into()
    }

    fn tile<'a>(&self, label: &str, cents: i64, tone: Tone) -> Element<'a, Message> {
        container(column![dim(label.to_string(), 11), bold(money(cents), 20).style(tone.text())].spacing(4))
            .padding(14)
            .width(Length::Fill)
            .style(look::card)
            .into()
    }

    // * Panels

    fn view_modal<'a>(&'a self, m: &'a Modal) -> Element<'a, Message> {
        let body: Element<'a, Message> = match m {
            Modal::AddAccount {
                name,
                kind,
                balance,
                on_budget,
            } => {
                let mut kinds = row![].spacing(6);
                for k in accounts::KINDS {
                    kinds = kinds.push(
                        button(text(k).size(12))
                            .on_press(Message::AccountKind(k))
                            .padding([4, 8])
                            .style(look::choice(*kind == k)),
                    );
                }
                column![
                    bold("Add an account", 16),
                    text_input("Its name", name)
                        .on_input(Message::AccountName)
                        .size(13)
                        .padding([6, 8])
                        .style(look::input),
                    dim("What sort of account?", 12),
                    kinds,
                    dim(
                        match *kind {
                            "credit" | "loan" => "How much is owed on it today?",
                            _ => "What is in it today?",
                        },
                        12
                    ),
                    text_input("0.00", balance)
                        .on_input(Message::AccountBalance)
                        .on_submit(Message::CreateAccount)
                        .size(13)
                        .padding([6, 8])
                        .style(look::input),
                    row![
                        button(text("Budget").size(12))
                            .on_press(Message::AccountOnBudget(true))
                            .padding([4, 10])
                            .style(look::choice(*on_budget)),
                        button(text("Tracking only").size(12))
                            .on_press(Message::AccountOnBudget(false))
                            .padding([4, 10])
                            .style(look::choice(!*on_budget)),
                    ]
                    .spacing(6),
                    row![
                        fill(),
                        quiet("Cancel", Message::CloseModal),
                        action("Add account", Message::CreateAccount)
                    ]
                    .spacing(8),
                ]
                .spacing(10)
                .into()
            }
            Modal::Reconcile { account, differs, balance } => {
                let cleared = self.peer.account(*account).map_or(0, |a| a.cleared);
                let mut c = column![
                    bold("Reconcile", 16),
                    text(format!("Is your cleared balance {}?", money(cleared))).size(13),
                    dim("Compare it with the bank's balance for the account, leaving out what is pending.", 11),
                ]
                .spacing(10);
                if *differs {
                    c = c
                        .push(dim("What does the bank say the balance is?", 12))
                        .push(
                            text_input("0.00", balance)
                                .on_input(Message::ReconcileTyped)
                                .on_submit(Message::ReconcileDone)
                                .size(13)
                                .padding([6, 8])
                                .style(look::input),
                        )
                        .push(row![fill(), quiet("Cancel", Message::CloseModal), action("Reconcile", Message::ReconcileDone)].spacing(8));
                } else {
                    c = c.push(
                        row![
                            fill(),
                            quiet("No", Message::ReconcileAgrees(false)),
                            action("Yes", Message::ReconcileAgrees(true))
                        ]
                        .spacing(8),
                    );
                }
                c.into()
            }
            Modal::Choose(choosing) => self.view_chooser(choosing),
        };
        panel::panel(container(body).padding(14), 440.0).into()
    }

    fn view_chooser<'a>(&'a self, choosing: &'a Choosing) -> Element<'a, Message> {
        let mut c = column![].spacing(8);
        let mut list = column![].spacing(1);
        let option = |label: String, trailing: Option<i64>, choice: Choice| -> Element<'a, Message> {
            let mut r = row![text(label).size(13).width(Length::Fill)].align_y(Alignment::Center);
            if let Some(cents) = trailing {
                let tone = Tone::of(cents, 0, 0);
                r = r.push(container(text(money(cents)).size(11)).padding([1, 7]).style(look::pill(tone)));
            }
            button(r)
                .on_press(Message::Chose(choice))
                .padding([5, 10])
                .width(Length::Fill)
                .style(style::quiet)
                .into()
        };
        let (skip, title) = match choosing {
            Choosing::Form => (None, "Category".to_string()),
            Choosing::Txns(ids) => (
                None,
                format!("Categorize {} transaction{}", ids.len(), if ids.len() == 1 { "" } else { "s" }),
            ),
            Choosing::Move { from, amount } => {
                let short = from.and_then(|f| self.peer.line(f)).is_some_and(|l| l.available < 0);
                c = c.push(
                    text_input("0.00", amount)
                        .on_input(Message::MoveTyped)
                        .size(13)
                        .padding([6, 8])
                        .style(look::input),
                );
                (*from, if short { "Cover it from".to_string() } else { "Move it to".to_string() })
            }
        };
        c = c.push(bold(title, 16));
        if let (Choosing::Form, Some(f)) = (choosing, &self.form) {
            if f.editing.is_none() {
                list = list.push(container(text("TRANSFER TO").size(10).style(style::faint(0.5))).padding([6, 10]));
                for a in self.peer.accounts.rows.iter().filter(|a| !a.closed && Some(a.id) != f.account) {
                    list = list.push(option(format!("Transfer : {}", a.name), None, Choice::Transfer(a.id)));
                }
            }
        }
        let rta = match choosing {
            Choosing::Move { .. } => Choice::Category(None),
            _ => Choice::Category(Some(ready_to_assign())),
        };
        list = list.push(container(text("INFLOW").size(10).style(style::faint(0.5))).padding([6, 10]));
        list = list.push(option("Ready to Assign".into(), Some(self.peer.summary().ready_to_assign), rta));
        for g in self.peer.groups.rows.iter().filter(|g| !g.hidden) {
            list = list.push(container(text(g.name.to_uppercase()).size(10).style(style::faint(0.5))).padding([6, 10]));
            for l in self
                .peer
                .lines
                .rows
                .iter()
                .filter(|l| l.group_id == g.id && !l.hidden && Some(l.category_id) != skip)
            {
                list = list.push(option(l.name.clone(), Some(l.available), Choice::Category(Some(l.category_id))));
            }
        }
        if matches!(choosing, Choosing::Txns(_)) {
            list = list.push(option("No category".into(), None, Choice::Category(None)));
        }
        c.push(scrollable(list).height(Length::Fixed(380.0)).style(style::bars))
            .push(row![fill(), quiet("Cancel", Message::CloseModal)])
            .into()
    }
}
