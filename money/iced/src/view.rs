//! What the window draws: the sidebar of accounts, and beside it the page —
//! the month's budget and its inspector, an account's register, or where
//! the month's money went — with a panel over it when one is open.

use arkui::{format, glyphs, icon, layer, menu, outline, panel, style, table};
use iced::widget::{button, column, container, mouse_area, progress_bar, row, rule, scrollable, stack, text, text_input, Column, Space};
use iced::{Alignment, Element, Length, Padding};
use money_domain::budget::ready_to_assign;
use money_domain::dates::month_of;
use money_domain::*;

use crate::amount::{money, money_or_blank};
use crate::look::{self, Tone};
use crate::peer::Ledger;
use crate::{App, Choice, Choosing, Field, Message, Modal, Naming, Page, Pane, SplitRow, TxnForm};

pub const SIDEBAR: f32 = 250.0;
/// How tall a line of the budget is, a group's heading or a category:
/// one height, so a drag is arithmetic (`arkui::outline`).
pub const LINE: f32 = 54.0;
/// The register's geometry: the top above its table, the table's heading,
/// the gap between them, and every row — one height each, so where a row
/// is on screen is arithmetic (`App::category_at`).
/// The top above the register: an account's has its balances too.
pub const LEDGER_TOP: f32 = 160.0;
pub const LEDGER_BALANCES: f32 = 52.0;
pub const LEDGER_HEAD: f32 = 28.0;
pub const LEDGER_GAP: f32 = 14.0;
pub const TXN_ROW: f32 = 34.0;
/// The sidebar's scrollable.
const SIDEBAR_SCROLL: &str = "sidebar";

/// A section of the sidebar's accounts: its heading, and which belong.
type Section = (&'static str, fn(&AccountLine) -> bool);
const INSPECTOR: f32 = 310.0;
pub const PAD: f32 = 18.0;

/// What scrolls from the window's top under the account's face, which
/// floats at the top right: scrolled, it goes soft under the face's band
/// rather than being drawn through it (`arkui::edge`), on `ground`, the
/// colour of what it is on; at rest it is as it was. The inspector, the
/// receipt beside the register, Settings and Reflect; the budget's and
/// the register's lists start below their headings and never pass under it.
pub(crate) fn under_face<'a>(content: impl Into<Element<'a, Message>>, ground: fn(&iced::Theme) -> iced::Color) -> Element<'a, Message> {
    arkui::edge::faded(content, arkui::account::band(PAD), ground)
}

/// How far down Reflect or Settings is scrolled once its title, the
/// page's first line, has gone off the top: the padding over it and its
/// line (Reflect's is its arrows', 32 tall). Sooner, and the small title
/// came while the large one was still there to read under the band.
pub const TITLED: f32 = PAD + 32.0;

impl App {
    /// What the small header says on this page once its title has scrolled
    /// away (`arkui::compact`): Reflect's and Settings' titles. The budget
    /// and the registers keep their headings over their lists, so they
    /// have none.
    pub(crate) fn compact_title(&self) -> Option<String> {
        match self.page {
            Page::Reflect => Some(format!("Spending in {}", format::month(&self.peer.month))),
            Page::Settings => Some("Settings".into()),
            _ => None,
        }
    }

    /// The small header, at the top of the page beside the face, while it
    /// is in.
    fn view_compact(&self) -> Option<Element<'_, Message>> {
        let now = iced::time::Instant::now();
        if !self.compact.showing(now) {
            return None;
        }
        let title = self.compact_title()?;
        let small = arkui::compact::view(None, &title, self.compact.at(now), Message::PageTop);
        Some(
            container(small)
                .padding(Padding {
                    top: PAD,
                    left: SIDEBAR + 1.0 + PAD - 6.0,
                    right: PAD + arkui::account::HEIGHT + PAD,
                    bottom: 0.0,
                })
                .height(Length::Fixed(arkui::account::band(PAD)))
                .align_y(Alignment::Center)
                .into(),
        )
    }
}

/// The page's own ground, for [`under_face`] (`arkui::style::page_color`).
pub(crate) use arkui::style::page_color as page_ground;

/// A page's padding around a list that scrolls: on the top and the sides,
/// and none under it, so the list runs down to the window's foot. With
/// `PAD` under it too the list stopped short of the edge with its last
/// visible row cut off mid-line over an empty strip; the padding under the
/// last row is inside what scrolls instead, so that row scrolls clear.
pub(crate) fn page_pad() -> Padding {
    Padding {
        bottom: 0.0,
        ..Padding::new(PAD)
    }
}
const MONEY_COL: f32 = 120.0;
/// The goal's column, and the pill's.
const GOAL_COL: f32 = 110.0;
const PILL_COL: f32 = 140.0;
/// How far in from a row's edge its first and last things sit.
const ROW_INSET: f32 = 12.0;

pub(crate) fn bold<'a>(s: impl Into<String>, size: u32) -> iced::widget::Text<'a> {
    text(s.into()).size(size).font(arkui::hero::BOLD)
}

/// A page's title, as harken draws its own ([`arkui::line::title`]).
pub(crate) fn page_title<'a>(s: impl Into<String>) -> Element<'a, Message> {
    arkui::line::title(s).into()
}

pub(crate) fn dim<'a>(s: impl Into<String>, size: u32) -> iced::widget::Text<'a> {
    text(s.into()).size(size).style(style::dim)
}

pub(crate) fn fill() -> Space {
    Space::new().width(Length::Fill)
}

/// A button of the accent: something somebody is meant to press.
pub(crate) fn action<'a>(label: &str, msg: Message) -> Element<'a, Message> {
    button(text(label.to_string()).size(13))
        .on_press(msg)
        .padding([5, 12])
        .style(style::action)
        .into()
}

/// A button that is a word until pointed at.
pub(crate) fn quiet<'a>(label: &str, msg: Message) -> Element<'a, Message> {
    button(text(label.to_string()).size(13))
        .on_press(msg)
        .padding([5, 10])
        .style(look::choice(false))
        .into()
}

/// Words in the accent that do something, underlined when pointed at.
pub(crate) fn link<'a>(label: impl Into<String>, msg: Message) -> Element<'a, Message> {
    arkui::button::link(label, 12.0, msg)
}

/// The icon button both windows draw (`arkui::button::icon`).
fn glyph_button<'a>(shape: &'static [u8], msg: Message) -> Element<'a, Message> {
    arkui::button::icon(shape, msg)
}

/// What an account is, drawn beside it in the sidebar.
fn glyph(a: &AccountLine) -> &'static [u8] {
    match a.kind.as_str() {
        "cash" => glyphs::WALLET,
        "credit" => glyphs::CARD,
        "loan" => glyphs::DEBT,
        "investment" => glyphs::GROWTH,
        _ => glyphs::BANK,
    }
}

/// An amount as the sidebar shows one: red below nothing, the same on the
/// cursor's entry as off it.
fn balance<'a>(cents: i64, lit: bool) -> Element<'a, Message> {
    text(money(cents))
        .size(12)
        .style(move |theme| text::Style {
            color: Some(match cents < 0 {
                true => arkui::theme::of(theme).danger.base.color,
                false => style::entry_text(theme, lit).scale_alpha(0.75),
            }),
        })
        .into()
}

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let page: Element<'_, Message> = match self.page {
            Page::Budget => self.view_budget(),
            Page::Reflect => self.view_reflect(),
            Page::Settings => self.view_settings(),
            Page::Ledger(l) => self.view_ledger(l),
        };
        // A swipe's arrow over the page, not the sidebar.
        let page = self.history.view(page, iced::time::Instant::now());
        // Nothing under the page: it runs to the window's foot. What the
        // last thing done came to, and a search or a count as it is typed,
        // are said for a moment in a pill laid over it (`arkui::toast`,
        // below) — a line under it, kept there by a note that was nearly
        // always there, cut off whatever the page had at its foot.
        // The sidebar is a surface, a step off the page's ground, with a
        // hairline where it meets the page, as harken's is.
        let base = container(row![self.view_sidebar(), rule::vertical(1).style(style::edge), page].height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::page);
        // Where the pointer is, so a press knows where a drag starts.
        let mut layers = stack![mouse_area(base).on_move(Message::Pointer)];
        // While a row or a heading is held, one layer over everything follows
        // the pointer and hears it let go, wherever that is.
        if self.drag.is_some() {
            layers = layers.push(
                mouse_area(container(text("")).width(Length::Fill).height(Length::Fill))
                    .on_move(Message::Dragged)
                    .on_release(Message::Dropped)
                    .on_scroll(|_| Message::Swallow),
            );
        }
        // The account at the top right, over the page, as harken's: its menu
        // opens Settings and signs out.
        if let Some(account) = self.account() {
            layers = layers.push(
                container(arkui::account::button(&account, self.account_picture(), Message::AccountMenu))
                    .width(Length::Fill)
                    .padding(PAD)
                    .align_x(Alignment::End),
            );
        }
        // The page's small header, once its title has scrolled away, in the
        // band beside the face.
        if let Some(small) = self.view_compact() {
            layers = layers.push(small);
        }
        // What the keys can do next, at the bottom right, over the page.
        if let Some(prefix) = self.which.showing() {
            let entries = arkui::which::entries(&[arkui::vim::GRAMMAR, crate::KEYS], &[arkui::vim::GROUPS, crate::GROUPS], prefix);
            if !entries.is_empty() {
                layers = layers.push(arkui::which::view(prefix, &entries, arkui::toast::MARGIN));
            }
        }
        if let Some(m) = &self.modal {
            layers = layers
                .push(layer::dimmed(Message::CloseModal, Message::Swallow))
                .push(layer::centred(self.view_modal(m)));
        }
        // The category dropdown, under the cell it is for; a click away closes it.
        if let Some(d) = &self.dropdown {
            let messages = arkui::command::Messages {
                input: look::DROP_INPUT,
                list: look::DROP_LIST,
                typed: Box::new(Message::DropTyped),
                at: Box::new(Message::DropAt),
                scrolled: Box::new(Message::DropScrolled),
            };
            let groups = self.drop_groups();
            // Above the cell, it stands on the cell: its bottom where its
            // corner is, however tall what is found makes it.
            let at = match d.above {
                true => {
                    let tall = match arkui::command::rows(&groups) {
                        0 => arkui::command::EMPTY_HEIGHT,
                        _ => arkui::command::list_height(&groups),
                    };
                    iced::Point::new(d.at.x, (d.at.y - tall - 2.0).max(0.0))
                }
                false => d.at,
            };
            let panel = arkui::command::list(&d.search, groups, messages, crate::DROP_WIDTH);
            layers = layers
                .push(layer::backdrop(Message::CloseDrop, Message::Swallow))
                .push(layer::pinned(panel, at));
        }
        if let Some(m) = &self.ctx.menu {
            layers = layers
                .push(layer::backdrop(Message::CloseMenu, Message::Swallow))
                .push(layer::pinned(menu::view(m, true, Message::MenuAt, Message::MenuActivate), m.origin));
        }
        // What the last thing done came to, or what is being typed: for a
        // moment, at the foot of the window, over everything — a modal's
        // refusal is said over the modal. It hears nothing, so what is
        // under it still does.
        let pending = self.keys.pending();
        if let Some(words) = arkui::toast::says(&self.toast, &pending) {
            layers = layers.push(arkui::toast::view(words, arkui::toast::MARGIN));
        }
        // The window's root: arkui's own needs of the platform.
        arkui::window::view(layers)
    }

    // * The sidebar

    /// The sidebar's places, in order: the pages, All Accounts, the accounts
    /// under their headings, then the saved filters — every register one is
    /// a filter's. What the keys walk is what is drawn.
    pub fn places(&self) -> Vec<(&'static [u8], String, Option<&'static str>, Page)> {
        let pages = [
            (glyphs::HOME, "Budget".to_string(), None, Page::Budget),
            (glyphs::LIBRARY, "Reflect".to_string(), None, Page::Reflect),
            (glyphs::PLAYLIST, "All Accounts".to_string(), None, Page::Ledger(Ledger::All)),
            (glyphs::WRENCH, "Settings".to_string(), None, Page::Settings),
        ];
        let sections: [Section; 3] = [
            ("On Budget", |a| a.on_budget && !a.closed),
            ("Tracking", |a| !a.on_budget && !a.closed),
            ("Closed", |a| a.closed),
        ];
        let accounts = sections.into_iter().flat_map(|(name, keep)| {
            self.peer.accounts.rows.iter().filter(move |a| keep(a)).map(move |a| {
                let label = match a.mask.is_empty() {
                    true => a.name.clone(),
                    false => format!("{} \u{2022}\u{2022}{}", a.name, a.mask),
                };
                (glyph(a), label, Some(name), Page::Ledger(Ledger::Account(a.id)))
            })
        });
        // The saved filters, after the accounts: registers like theirs.
        let saved = self
            .peer
            .filters
            .rows
            .iter()
            .map(|f| (glyphs::SEARCH, f.name.clone(), Some("Filters"), Page::Ledger(Ledger::Saved(f.id))));
        pages.into_iter().chain(accounts).chain(saved).collect()
    }

    /// The sidebar's cursor: the page shown. The inbox is All Accounts,
    /// filtered, and a register changed here is what it was changed from,
    /// so their rows stay the ones lit.
    pub fn sidebar_at(&self) -> usize {
        let here = match self.page {
            Page::Ledger(Ledger::Inbox) => Page::Ledger(Ledger::All),
            Page::Ledger(Ledger::Adhoc) => match self.peer.base {
                Ledger::Inbox | Ledger::Adhoc => Page::Ledger(Ledger::All),
                b => Page::Ledger(b),
            },
            p => p,
        };
        self.places().iter().position(|p| p.3 == here).unwrap_or(0)
    }

    fn view_sidebar(&self) -> Element<'_, Message> {
        let cursor = self.sidebar_at();
        let focused = self.pane == Pane::Sidebar && self.keys_free();
        let inbox = self.peer.inbox.rows.len();
        let items = self
            .places()
            .into_iter()
            .enumerate()
            .map(|(i, (glyph, label, heading, page))| {
                let lit = arkui::sidebar::lit(i, cursor, focused);
                let trailing = match page {
                    Page::Ledger(Ledger::All) => (inbox > 0).then(|| self.badge(inbox, lit)),
                    Page::Ledger(Ledger::Account(id)) => self.peer.account(id).map(|a| balance(a.balance, lit)),
                    Page::Ledger(Ledger::Saved(id)) => self.peer.saved(id).map(|f| balance(f.total, lit)),
                    _ => None,
                };
                arkui::sidebar::Item {
                    glyph,
                    label,
                    heading,
                    trailing,
                    press: Message::Go(page),
                }
            })
            .collect();
        arkui::sidebar::view(items, cursor, focused, SIDEBAR, PAD, SIDEBAR_SCROLL)
    }

    /// A count in a bubble: what needs looking at.
    fn badge<'a>(&self, n: usize, _lit: bool) -> Element<'a, Message> {
        container(text(n.to_string()).size(10).style(move |theme| text::Style {
            color: Some(arkui::theme::of(theme).primary.base.text),
        }))
        .padding([1, 6])
        .style(move |theme| {
            let p = arkui::theme::of(theme);
            container::Style {
                background: Some(iced::Background::Color(p.primary.base.color)),
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
            page_title(format::month(&self.peer.month)),
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

        let heading = |label: &str, width: f32| right(text(label.to_string()).size(11).style(style::faint(0.55)).into(), width);
        let header = container(
            row![
                text("Category").size(11).style(style::faint(0.55)).width(Length::Fill),
                heading("Assigned", MONEY_COL),
                heading("Activity", MONEY_COL),
                Space::new().width(Length::Fixed(GOAL_COL)),
                heading("Available", PILL_COL),
            ]
            .align_y(Alignment::Center),
        )
        .padding([6.0, ROW_INSET + 4.0]);

        let mut new_group: Column<'_, Message> = Column::new();
        if let Some(name) = &self.new_group {
            new_group = new_group.push(
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
        // The categories under their groups, the cursor on the categories
        // alone (`arkui::outline`): `j` and `k` walk past the headings, `J`
        // and `K` carry what is selected, `{` and `}` its group.
        let groups = self
            .shown()
            .into_iter()
            .map(|s| {
                let foot = match &self.new_category {
                    Some((ng, name)) if *ng == s.group.id => Some(
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
                        })
                        .into(),
                    ),
                    _ => None,
                };
                outline::Group {
                    head: self.group_row(s.group, &s.lines),
                    rows: s.rows,
                    foot,
                }
            })
            .collect();
        let body = outline::view(
            groups,
            self.lit(),
            self.budget_keys(),
            outline::Look {
                line: LINE,
                zebra: false,
                rules: true,
            },
            self.drag.as_ref(),
            self.settle.as_ref(),
            iced::time::Instant::now(),
            Message::Grab,
            Message::Menu,
            |l, lit| self.category_row(l, lit),
        );

        let page = column![
            top,
            tools,
            column![
                header,
                rule::horizontal(1).style(style::hairline),
                new_group,
                arkui::scroll::elastic(
                    scrollable(container(body).padding(Padding {
                        bottom: PAD,
                        ..Padding::ZERO
                    }))
                    .id(look::BUDGET)
                    .on_scroll(|v| Message::PageScrolled(v.absolute_offset().y))
                    .height(Length::Fill)
                    .style(style::bars)
                )
            ],
        ]
        .spacing(12)
        .padding(page_pad())
        .width(Length::Fill);

        row![page, self.view_inspector()].into()
    }

    /// A name, or the box it is being renamed in.
    fn name_or_box<'a>(&self, named: Naming, name: Element<'a, Message>) -> Element<'a, Message> {
        match &self.renaming {
            Some((n, typed)) if *n == named => text_input("Name", typed)
                .id(look::RENAME)
                .on_input(Message::RenameTyped)
                .on_submit(Message::RenameDone)
                .size(13)
                .padding([3, 6])
                .width(Length::Fixed(240.0))
                .style(look::input)
                .into(),
            _ => name,
        }
    }

    /// A group's heading, a card the rows hang under: its name, and what
    /// its categories were given and spent. The cards' group is the
    /// cards' — its categories come and go with them, so it offers no new
    /// one — and says what of their debt is not covered.
    fn group_row<'a>(&self, g: &GroupLine, lines: &[&BudgetLine]) -> Element<'a, Message> {
        let sum = |f: fn(&BudgetLine) -> i64| lines.iter().map(|l| f(l)).sum::<i64>();
        let collapsed = self.collapsed.contains(&g.id);
        let cards = g.id == money_domain::cards::card_group();
        let mut name = row![button(icon::faint(if collapsed { glyphs::CHEVRON } else { glyphs::DOWN }, false))
            .on_press(Message::Collapse(g.id))
            .padding(2)
            .style(style::quiet),]
        .spacing(8)
        .align_y(Alignment::Center);
        if cards {
            name = name.push(bold(g.name.clone(), 14));
        } else {
            name = name.push(self.name_or_box(Naming::Group(g.id), bold(g.name.clone(), 14).into())).push(
                button(icon::faint(glyphs::PLUS, false))
                    .on_press(Message::NewCategory(g.id))
                    .padding(2)
                    .style(style::quiet),
            );
        }
        let total = |c: i64| right(bold(money(c), 14).into(), MONEY_COL);
        let short = sum(|l| l.underfunded);
        let note: Element<'a, Message> = match cards && short > 0 {
            true => right(
                text(format!("{} not covered", money(short))).size(12).style(Tone::Short.text()).into(),
                GOAL_COL + PILL_COL,
            ),
            false => Space::new().width(Length::Fixed(GOAL_COL + PILL_COL)).into(),
        };
        let card = container(
            row![
                container(name).width(Length::Fill),
                total(sum(|l| l.assigned)),
                total(sum(|l| l.activity)),
                note,
            ]
            .align_y(Alignment::Center),
        )
        .padding([0.0, ROW_INSET])
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .style(look::group_card);
        container(card).padding([6, 0]).height(Length::Fill).into()
    }

    /// A category's line: its tile and name, what it was given, what was
    /// spent, how far along its goal is, and what is left in a pill.
    /// `lit` is the accent under it, so every colour on it asks
    /// `style::on_row` — the pill too, which on the accent is the accent's
    /// text on a wash of it.
    fn category_row<'a>(&'a self, l: &'a BudgetLine, lit: bool) -> Element<'a, Message> {
        let tone = tone(l);
        let hidden = l.hidden;
        let label = text(l.name.clone()).size(14).style(move |theme| text::Style {
            color: Some(style::on_row(theme, lit, false, hidden)),
        });
        let name = row![tile(l), self.name_or_box(Naming::Category(l.category_id), label.into())]
            .spacing(12)
            .align_y(Alignment::Center);
        let assigned: Element<'a, Message> = match &self.assigning {
            Some((c, typed)) if *c == l.category_id => text_input("0.00", typed)
                .id(look::ASSIGNED)
                .on_input(Message::AssignedTyped)
                .on_submit(Message::AssignedDone)
                .size(14)
                .padding([3, 6])
                .width(Length::Fixed(MONEY_COL - 14.0))
                .style(look::input)
                .into(),
            // Worked out by its formula: the amount, with the formula's mark, and
            // pressing it says why it does not open.
            _ if l.ruled => button(
                row![
                    icon::faint(glyphs::REPEAT, lit).width(12.0).height(12.0),
                    text(money(l.assigned)).size(14).style(move |theme| text::Style {
                        color: Some(style::on_row(theme, lit, false, false)),
                    })
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .on_press(Message::EditAssigned(l.category_id))
            .padding([3, 0])
            .style(style::quiet)
            .into(),
            _ => button(text(money(l.assigned)).size(14).style(move |theme| text::Style {
                color: Some(style::on_row(theme, lit, false, false)),
            }))
            .on_press(Message::EditAssigned(l.category_id))
            .padding([3, 0])
            .style(style::quiet)
            .into(),
        };
        // Nothing spent is a dash, quiet, rather than a row of zeros.
        let activity = match l.activity {
            0 => text("\u{2014}").size(14).style(move |theme| text::Style {
                color: Some(style::on_row(theme, lit, false, true)),
            }),
            a => text(money(a)).size(14).style(move |theme| text::Style {
                color: Some(style::on_row(theme, lit, false, false)),
            }),
        };
        let goal = text(progress(l)).size(13).style(move |theme| text::Style {
            color: Some(style::on_row(theme, lit, false, true)),
        });
        let available = container(text(money(l.available)).size(14).font(arkui::hero::BOLD))
            .padding([4, 12])
            .style(look::pill(tone, lit));
        container(
            row![
                container(name).width(Length::Fill),
                right(assigned, MONEY_COL),
                right(activity.into(), MONEY_COL),
                right(goal.into(), GOAL_COL),
                right(available.into(), PILL_COL),
            ]
            .align_y(Alignment::Center),
        )
        .padding([0.0, ROW_INSET])
        .into()
    }

    // * The inspector

    fn view_inspector(&self) -> Element<'_, Message> {
        let body = match self.selected.and_then(|c| self.peer.line(c)) {
            Some(l) => self.inspect_category(l),
            None => self.inspect_month(),
        };
        // Its top clear of the account button, which floats over the corner.
        let pad = Padding {
            top: PAD + arkui::account::HEIGHT + 6.0,
            ..Padding::new(PAD)
        };
        container(arkui::scroll::elastic(
            scrollable(under_face(body.spacing(12).padding(pad), style::surface_color))
                .height(Length::Fill)
                .style(style::bars),
        ))
        .width(Length::Fixed(INSPECTOR))
        .height(Length::Fill)
        .style(style::surface)
        .into()
    }

    pub(crate) fn figure<'a>(label: &str, cents: i64) -> Element<'a, Message> {
        row![text(label.to_string()).size(12).width(Length::Fill), text(money(cents)).size(12)]
            .align_y(Alignment::Center)
            .into()
    }

    fn inspect_month(&self) -> Column<'_, Message> {
        let s = self.peer.summary();
        let lines = &self.peer.lines.rows;
        let carried: i64 = lines.iter().map(|l| l.carried).sum();
        let name = format::month(&self.peer.month);
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
        let tone = tone(l);
        let spent = match l.card {
            Some(_) => "Spending covered, less payments",
            None => "Cash spending",
        };
        let mut figures = column![
            row![
                column![bold(l.name.clone(), 16), dim(l.group.clone(), 11)].spacing(2).width(Length::Fill),
                glyph_button(glyphs::CLOSE, Message::Select(None)),
            ]
            .align_y(Alignment::Center),
            row![
                text("Available Balance").size(12).width(Length::Fill),
                container(text(money(l.available)).size(14))
                    .padding([3, 10])
                    .style(look::pill(tone, false)),
            ]
            .align_y(Alignment::Center),
            rule::horizontal(1).style(style::hairline),
            App::figure("Cash left over from last month", l.carried),
            App::figure("Assigned this month", l.assigned),
            App::figure(spent, l.activity),
        ]
        .spacing(8);
        if l.credit_overspent > 0 {
            // Debt now, not next month's money: amber, as YNAB has it.
            figures = figures.push(
                row![
                    text("Spent on a card, not covered").size(12).width(Length::Fill),
                    text(money(l.credit_overspent)).size(12).style(Tone::Short.text()),
                ]
                .align_y(Alignment::Center),
            );
        }
        figures = figures.push(if l.ruled {
            dim("Assigned by its formula this month: money is not moved to or from it by hand.", 11).into()
        } else if l.available < 0 {
            action("Cover overspending", Message::MoveMoney)
        } else {
            quiet("Move money", Message::MoveMoney)
        });
        let head = container(figures).padding(14).style(look::card);

        if l.card.is_some() {
            return column![head, self.inspect_card(l)];
        }

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
            if have < l.target && l.ruled {
                target = target.push(dim("Its formula assigns it: what its filters add up to decides.", 11));
            } else if have < l.target {
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

        let mut kinds = column![
            bold("Kinds", 14),
            dim(
                "Kinds of it, from every store: Eggs under Groceries. Their spending is this category's.",
                11
            ),
        ]
        .spacing(6);
        for n in self.peer.tree.rows.iter().filter(|n| n.depth >= 2 && self.is_under(n, l.category_id)) {
            let indent = 12.0 * (n.depth - 2) as f32;
            kinds = kinds.push(row![Space::new().width(Length::Fixed(indent)), text(n.name.clone()).size(12)]);
        }
        kinds = kinds.push(match &self.new_kind {
            Some((parent, typed)) if *parent == l.category_id => row![
                text_input("A kind, as Eggs", typed)
                    .id(look::NEW_KIND)
                    .on_input(Message::NewKindTyped)
                    .on_submit(Message::NewKindDone)
                    .size(13)
                    .padding([5, 8])
                    .style(look::input),
                quiet("Add", Message::NewKindDone),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
            _ => link("+ Add a kind", Message::NewKind(l.category_id)),
        });
        let kinds = container(kinds).padding(14).style(look::card);

        let hidden = l.hidden;
        let manage = row![
            link(if hidden { "Unhide" } else { "Hide" }, Message::HideCategory(l.category_id, !hidden)),
            link("Delete category", Message::DeleteCategory(l.category_id)),
        ]
        .spacing(16);
        match l.ruled {
            // Its formula before the hand's buttons, which it has no use for.
            true => column![head, self.inspect_formula(l), target, kinds, manage],
            false => column![head, target, auto, self.inspect_formula(l), kinds, manage],
        }
    }

    /// A card's payment category: what the card owes, what of it the
    /// category does not cover, and the money to cover it. Its name is the
    /// card's, and it goes when the card is closed: nothing to rename or
    /// delete here.
    fn inspect_card<'a>(&'a self, l: &'a BudgetLine) -> Element<'a, Message> {
        let mut card = column![
            bold("Card balance", 14),
            App::figure("Owed at the end of the month", l.debt),
            row![
                text("Not covered").size(12).width(Length::Fill),
                text(money(l.underfunded))
                    .size(12)
                    .style(if l.underfunded > 0 { Tone::Short } else { Tone::Funded }.text()),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(8);
        if l.underfunded > 0 {
            card = card.push(action(
                &format!("Assign {} to cover the debt", money(l.underfunded)),
                Message::AssignTo(l.category_id, l.assigned + l.underfunded),
            ));
        } else if l.debt > 0 {
            card = card.push(text("Enough set aside to pay it").size(12).style(Tone::Funded.text()));
        } else {
            card = card.push(dim("Nothing owed on it.", 11));
        }
        card = card.push(dim(
            "Spending on the card moves what its category had here; what a category could not cover is debt to assign money to.",
            11,
        ));
        let hidden = l.hidden;
        column![
            container(card).padding(14).style(look::card),
            link(if hidden { "Unhide" } else { "Hide" }, Message::HideCategory(l.category_id, !hidden)),
        ]
        .spacing(12)
        .into()
    }

    /// Whether a category of the tree is under `id`, at any depth.
    fn is_under(&self, n: &CategoryNode, id: Id<Category>) -> bool {
        let mut at = n.parent_id;
        for _ in 0..money_domain::budget::DEEPEST {
            match at {
                Some(p) if p == id => return true,
                Some(p) => at = self.peer.node(p).and_then(|n| n.parent_id),
                None => return false,
            }
        }
        false
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

    /// How tall the top above the register is: taller by the balances when
    /// it is one account's.
    pub fn ledger_top(&self) -> f32 {
        match self.register_account() {
            Some(_) => LEDGER_TOP + LEDGER_BALANCES,
            None => LEDGER_TOP,
        }
    }

    /// The register for a filter: its name and what it adds up to, the
    /// account's balances and tools when it is exactly one account's, the
    /// household's tools on every account's, the bar of its conditions,
    /// and its transactions.
    fn view_ledger(&self, ledger: Ledger) -> Element<'_, Message> {
        let rows = &self.peer.register.rows;
        let account = self.register_account().and_then(|a| self.peer.account(a));
        // The account column, unless every row is one account's.
        let all = account.is_none();
        let (title, subtitle) = self.register_title();

        // An account's name is renamed where it stands, as a category's is,
        // and so is a saved filter's.
        let title: Element<'_, Message> = match (ledger, account) {
            (Ledger::Account(id), _) => self.name_or_box(Naming::Account(id), page_title(title)),
            (Ledger::Saved(id), _) => self.name_or_box(Naming::Filter(id), page_title(title)),
            _ => page_title(title),
        };
        // What the conditions come to, beside the title: the bar under it is
        // as wide as its conditions, and the receipt's panel takes the rest.
        let head_line = row![column![title, dim(subtitle, 12)].spacing(2), fill(), self.view_filter_total()].align_y(Alignment::Center);
        let bar = row![self.view_filter_bar()].align_y(Alignment::Center);

        // One slot whatever the register, so the bar under it keeps its
        // place (and the text box its focus) as the filter changes.
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
            None => Space::new().height(Length::Fixed(0.0)).into(),
        };

        let unapproved: Vec<Id<Txn>> = rows.iter().filter(|t| !t.approved).map(|t| t.id).collect();
        let mut tools = row![action("+ Add Transaction", Message::AddTxn)].spacing(10).align_y(Alignment::Center);
        if let Some(a) = account {
            tools = tools.push(quiet("Reconcile", Message::Reconcile(a.id)));
            tools = tools.push(quiet("Rename", Message::Rename(Naming::Account(a.id))));
            if !a.bank.is_empty() {
                tools = tools.push(quiet("Sync", Message::Sync));
            }
        } else if ledger == Ledger::All {
            // Every account's register is where accounts are added, and every
            // bank asked for what it has.
            tools = tools.push(quiet("Add Account", Message::AddAccount));
            tools = tools.push(quiet("Connect a Bank", Message::Banks(crate::banks::Msg::Connect)));
            if self.banks_wanting() > 0 {
                let n = self.banks_wanting();
                tools = tools.push(quiet(&format!("Banks need a look ({n})"), Message::Go(Page::Settings)));
            }
            if !self.peer.banks.rows.is_empty() {
                tools = tools.push(quiet("Sync", Message::Sync));
            }
        }
        if !unapproved.is_empty() {
            tools = tools.push(quiet(&format!("Approve all ({})", unapproved.len()), Message::Approve(unapproved)));
        }
        tools = tools.push(fill());
        for t in self.filter_tools() {
            tools = tools.push(t);
        }

        let w = |n: f32| Length::Fixed(n);
        let mut head = row![].align_y(Alignment::Center);
        if all {
            head = head.push(table::heading("Account", w(130.0)));
        }
        head = head
            .push(table::heading("Date", w(96.0)))
            .push(table::heading("Payee", Length::FillPortion(3)))
            .push(table::heading("Category", Length::FillPortion(3)))
            .push(table::heading("Memo", Length::FillPortion(2)))
            .push(table::heading("Outflow", w(100.0)))
            .push(table::heading("Inflow", w(100.0)))
            .push(table::heading("", w(116.0)));

        let mut body: Column<'_, Message> = Column::new();
        if let Some(f) = self.form.as_ref().filter(|f| f.editing.is_none()) {
            body = body.push(self.view_form(f, all));
        }
        let lit = self.txn_lit();
        let focused = self.pane == Pane::Page && self.keys_free();
        for (i, t) in rows.iter().enumerate() {
            match &self.form {
                Some(f) if f.editing == Some(t.id) => body = body.push(self.view_form(f, all)),
                _ => {
                    let on_cursor = lit.is_some_and(|(a, b)| a <= i && i <= b);
                    body = body.push(self.txn_row(t, all, i % 2 == 1, on_cursor, focused));
                }
            }
        }
        if rows.is_empty() {
            body = body.push(container(dim("Nothing here matches.", 13)).padding(20));
        }

        // The top is one height, and so is every row, so where a row's
        // category is on screen is arithmetic (`App::category_at`): the
        // dropdown opens under it, keys or pointer.
        let register = column![
            container(column![head_line, balances, tools, bar].spacing(12))
                .height(Length::Fixed(self.ledger_top()))
                .clip(true),
            column![
                table::row(head, false, false, false).height(Length::Fixed(LEDGER_HEAD)),
                rule::horizontal(1).style(style::hairline),
                arkui::scroll::elastic(
                    scrollable(body.padding(Padding {
                        bottom: PAD,
                        ..Padding::ZERO
                    }))
                    .id(look::REGISTER)
                    .on_scroll(|v| Message::RegisterScrolled(v.absolute_offset().y))
                    .height(Length::Fill)
                    .style(style::bars),
                ),
            ],
        ]
        .spacing(LEDGER_GAP)
        .padding(page_pad())
        .width(Length::Fill);
        // The selected transaction's receipt beside it, as the inspector is
        // beside the budget.
        row![register, self.view_receipt_side()].into()
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

    /// A transaction's row. `on_cursor` is the register's cursor or its
    /// visual selection, lit while the register has the keys; a click puts
    /// the cursor on it, and on the cursor's row opens it to edit.
    fn txn_row<'a>(&'a self, t: &'a TxnLine, all: bool, odd: bool, on_cursor: bool, focused: bool) -> Element<'a, Message> {
        let lit = on_cursor && focused;
        let w = |n: f32| Length::Fixed(n);
        let mut line = row![].align_y(Alignment::Center);
        if all {
            line = line.push(table::cell(t.account.clone(), w(130.0), lit, false, true));
        }
        let needs = t.category == "Needs a category";
        // The category opens the dropdown, for this row — or for the whole
        // selection, when the row is in it.
        let label = text(t.category.clone())
            .size(table::CELL_SIZE)
            .wrapping(text::Wrapping::None)
            .style(move |theme| text::Style {
                color: Some(match (needs, lit) {
                    (true, false) => arkui::theme::of(theme).warning.base.color,
                    _ => style::on_row(theme, lit, false, t.category_id == Some(ready_to_assign())),
                }),
            });
        // While its category is being chosen, the cell is the box it is typed
        // into, the list hanging under it.
        let choosing = matches!(&self.dropdown, Some(d) if matches!(d.choosing, Choosing::Txns(_))) && self.txn_at == Some(t.id);
        let category: Element<'a, Message> = match (t.transfer, choosing) {
            (_, true) => container(self.drop_box(&t.category))
                .width(Length::FillPortion(3))
                .padding(Padding {
                    right: table::SPACING,
                    ..Padding::ZERO
                })
                .into(),
            (true, false) => table::cell(t.category.clone(), Length::FillPortion(3), lit, false, true),
            (false, false) => container(mouse_area(label).on_press(Message::CategorizeRow(t.id)))
                .width(Length::FillPortion(3))
                .padding(Padding {
                    right: table::SPACING,
                    ..Padding::ZERO
                })
                .clip(true)
                .into(),
        };
        line = line
            .push(table::cell(t.date.clone(), w(96.0), lit, false, true))
            .push(table::cell(t.payee.clone(), Length::FillPortion(3), lit, !t.approved, false))
            .push(category)
            .push(table::cell(t.memo.clone(), Length::FillPortion(2), lit, false, true))
            .push(table::cell(money_or_blank((-t.amount).max(0)), w(100.0), lit, false, false))
            .push(table::cell(money_or_blank(t.amount.max(0)), w(100.0), lit, false, false));
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
        // A receipt found for it, or a photo of one taken: opened with it.
        if !t.receipt.is_empty() || t.photos > 0 {
            let read = !t.receipt.is_empty();
            end = end.push(
                button(icon::tinted(glyphs::RECEIPT, icon::SIZE, move |p| match read {
                    true => p.background.base.text.scale_alpha(0.7),
                    false => p.background.base.text.scale_alpha(0.35),
                }))
                .on_press(Message::EditTxn(t.id))
                .padding(2)
                .style(style::quiet),
            );
        }
        if !t.approved {
            end = end.push(link("Approve", Message::Approve(vec![t.id])));
        }
        line = line.push(container(end).width(w(116.0)));
        mouse_area(
            table::row(line, on_cursor, focused, odd)
                .center_y(Length::Fixed(TXN_ROW))
                .style(move |theme| style::row(theme, on_cursor, focused, odd)),
        )
        .on_press(Message::TxnClick(t.id))
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
            (None, Some(c)) => self.peer.category_name(c).unwrap_or_else(|| "a category".into()),
            (None, None) if f.split.is_some() => "Split".into(),
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
            .push(match &self.dropdown {
                Some(d) if d.choosing == Choosing::Form => container(self.drop_box(&category_label)).width(Length::FillPortion(3)).into(),
                // A split's categories are its lines'.
                _ if f.split.is_some() => Element::from(
                    button(text(category_label.clone()).size(13))
                        .padding([4, 6])
                        .width(Length::FillPortion(3))
                        .style(look::choice(false)),
                ),
                _ => Element::from(
                    button(text(category_label).size(13))
                        .on_press(Message::ChooseCategory)
                        .padding([4, 6])
                        .width(Length::FillPortion(3))
                        .style(look::choice(false)),
                ),
            })
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
        line = line.push(container(cleared).width(w(110.0)));

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
        if !f.bank.is_empty() {
            under = under.push(dim(format!("The bank: {}", f.bank), 11));
        }
        if !f.by.is_empty() {
            under = under.push(dim(format!("Entered by {}", f.by), 11));
        }
        under = under.push(fill());
        // A transfer's money only moved, and is not split.
        if f.transfer_to.is_none() {
            under = under.push(match f.split.is_some() {
                true => link("Unsplit", Message::Split(false)),
                false => link("Split", Message::Split(true)),
            });
        }
        if let Some(id) = f.editing {
            under = under.push(link("Delete", Message::DeleteTxn(id)));
        }
        under = under.push(quiet("Cancel", Message::CancelTxn)).push(action("Save", Message::SaveTxn));

        let mut parts = column![line].spacing(8);
        if let Some(rows) = &f.split {
            parts = parts.push(self.view_split(f, rows));
        }
        parts = parts.push(under);
        container(parts)
            .padding([8, 4])
            .style(|theme| container::Style {
                background: Some(iced::Background::Color(arkui::theme::of(theme).primary.base.color.scale_alpha(0.1))),
                ..container::Style::default()
            })
            .into()
    }

    /// A split's lines, under the form's: each a category and what of the
    /// amount is for it, and what is left of the amount to give one.
    fn view_split<'a>(&'a self, f: &'a TxnForm, rows: &'a [SplitRow]) -> Element<'a, Message> {
        let whole = (crate::amount::parse(&f.outflow).unwrap_or(0) - crate::amount::parse(&f.inflow).unwrap_or(0)).abs();
        let given: i64 = rows.iter().filter_map(|r| crate::amount::parse(&r.amount)).map(i64::abs).sum();
        let mut lines = column![].spacing(6);
        for (i, r) in rows.iter().enumerate() {
            let label = match r.category {
                Some(c) => self.peer.category_name(c).unwrap_or_else(|| "a category".into()),
                None => "Choose a category".into(),
            };
            let mut line = row![
                Space::new().width(Length::Fixed(110.0)),
                button(text(label).size(12))
                    .on_press(Message::SplitCategory(i))
                    .padding([3, 6])
                    .width(Length::Fixed(240.0))
                    .style(look::choice(false)),
                text_input("0.00", &r.amount)
                    .on_input(move |v| Message::SplitAmount(i, v))
                    .on_submit(Message::SaveTxn)
                    .size(12)
                    .padding([3, 6])
                    .width(Length::Fixed(94.0))
                    .style(look::input),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if rows.len() > 2 {
                line = line.push(glyph_button(glyphs::CLOSE, Message::SplitRemove(i)));
            }
            lines = lines.push(line);
        }
        let left = whole - given;
        let said = match left {
            0 => dim("Every cent has a category", 11),
            l if l > 0 => text(format!("{} left to split", money(l))).size(11).style(Tone::Short.text()),
            l => text(format!("{} too much", money(-l))).size(11).style(Tone::Over.text()),
        };
        lines
            .push(
                row![Space::new().width(Length::Fixed(110.0)), link("+ Add a line", Message::SplitAdd), said]
                    .spacing(16)
                    .align_y(Alignment::Center),
            )
            .into()
    }

    // * Receipts

    /// A receipt's charges, as the panel beside the register has room for:
    /// each its day and amount, its card, and what paid it under them, which
    /// opens the dropdown of what could have.
    pub(crate) fn view_charges<'a>(&'a self, charges: &'a [ChargeLine]) -> Element<'a, Message> {
        let mut list = column![dim("Charged", 11)].spacing(10);
        for c in charges {
            let found = c.txn_id.is_some();
            let label = match found {
                true => format!("{} \u{2014} {}", c.payee, c.account),
                false => "Find what paid it".into(),
            };
            let pay = button(text(label).size(12).style(move |theme| text::Style {
                color: Some(match found {
                    true => arkui::theme::of(theme).background.base.text,
                    false => arkui::theme::of(theme).warning.base.color,
                }),
            }))
            .on_press(Message::ChoosePayer(PaidFor {
                receipt_id: c.receipt_id.clone(),
                charge_id: Some(c.id.clone()),
            }))
            .padding([3, 6])
            .width(Length::Fill)
            .style(look::choice(false));
            let what = match c.amount > 0 {
                true => format!("refund {}", money(c.amount)),
                false => money(-c.amount),
            };
            list = list.push(
                column![
                    row![text(c.date.clone()).size(12), fill(), text(what).size(12)].align_y(Alignment::Center),
                    text(c.card.clone()).size(11).style(style::dim),
                    pay,
                ]
                .spacing(3),
            );
        }
        container(list).padding([6, 0]).into()
    }

    // * Settings

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
                    text(format::percent(g.spent, total))
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
            page_title(format!("Spending in {}", format::month(&self.peer.month))),
            glyph_button(glyphs::CHEVRON, Message::Month(1)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let mut tiles = row![
            self.tile("Income", s.income, Tone::Funded),
            self.tile("Spent", total, Tone::Zero),
            self.tile("Net", s.income - total, if s.income >= total { Tone::Funded } else { Tone::Over }),
        ]
        .spacing(12);
        // Paying a card is not spending (what it bought was), so it is not
        // in the bars; what the cards owe beyond what is set aside is.
        let cards: Vec<&BudgetLine> = self.peer.lines.rows.iter().filter(|l| l.card.is_some()).collect();
        if !cards.is_empty() {
            let short: i64 = cards.iter().map(|l| l.underfunded).sum();
            tiles = tiles.push(self.tile("Card debt not covered", short, if short > 0 { Tone::Short } else { Tone::Funded }));
        }
        // All of it scrolls, its padding inside: a month of many categories
        // is taller than the window, and was cut off at the foot.
        let page = column![month, tiles, container(bars).padding(16).width(Length::Fill).style(look::card)]
            .spacing(16)
            .padding(PAD)
            .width(Length::Fill);
        arkui::scroll::elastic(
            scrollable(under_face(page, page_ground))
                .id(look::PAGE_TOP)
                .on_scroll(|v| Message::TopScrolled(v.absolute_offset().y))
                .height(Length::Fill)
                .style(style::bars),
        )
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
        if let Modal::Photo(_) = m {
            return panel::panel(container(self.view_photo()).padding(14), 720.0).into();
        }
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
            Modal::Disconnect(bank) => self.view_disconnect(bank),
            Modal::Photo(_) => self.view_photo(),
            Modal::SaveFilter { name } => self.view_save_filter(name),
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
                r = r.push(container(text(money(cents)).size(11)).padding([1, 7]).style(look::pill(tone, false)));
            }
            button(r)
                .on_press(Message::Chose(choice))
                .padding([5, 10])
                .width(Length::Fill)
                .style(style::quiet)
                .into()
        };
        let (skip, title) = match choosing {
            Choosing::Form | Choosing::SplitLine(_) => (None, "Category".to_string()),
            Choosing::Payer(_) => (None, "Paid by".to_string()),
            Choosing::Items(items) => (None, format!("File {} item{}", items.len(), if items.len() == 1 { "" } else { "s" })),
            Choosing::Filter => (None, "Categories".to_string()),
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
                list = list.push(container(text("Transfer to").size(10).style(style::faint(0.5))).padding([6, 10]));
                for a in self.peer.accounts.rows.iter().filter(|a| !a.closed && Some(a.id) != f.account) {
                    list = list.push(option(format!("Transfer : {}", a.name), None, Choice::Transfer(a.id)));
                }
            }
        }
        let rta = match choosing {
            Choosing::Move { .. } => Choice::Category(None),
            _ => Choice::Category(Some(ready_to_assign())),
        };
        list = list.push(container(text("Inflow").size(10).style(style::faint(0.5))).padding([6, 10]));
        list = list.push(option("Ready to Assign".into(), Some(self.peer.summary().ready_to_assign), rta));
        for g in self.peer.groups.rows.iter().filter(|g| !g.hidden) {
            list = list.push(container(text(g.name.clone()).size(10).style(style::faint(0.5))).padding([6, 10]));
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
        c.push(arkui::scroll::elastic(scrollable(list).height(Length::Fixed(380.0)).style(style::bars)))
            .push(row![fill(), quiet("Cancel", Message::CloseModal)])
            .into()
    }
}

/// `content` at the right of a column `width` wide: figures line up on
/// their last digit.
fn right<'a>(content: Element<'a, Message>, width: f32) -> Element<'a, Message> {
    container(content)
        .width(Length::Fixed(width))
        .align_x(iced::alignment::Horizontal::Right)
        .into()
}

/// A category's tile: what it is for, drawn on a colour its name picks —
/// a card's payment category, a card on the cards' slate.
fn tile<'a>(l: &BudgetLine) -> Element<'a, Message> {
    let (glyph, colour) = match l.card {
        Some(_) => (glyphs::CARD, crate::palette::CARD_TILE),
        None => (
            category_glyph(&l.name),
            crate::palette::TILES[arkui::art::hash(&l.name) as usize % crate::palette::TILES.len()],
        ),
    };
    container(icon::tinted(glyph, 16.0, |_| crate::palette::TILE_GLYPH))
        .width(Length::Fixed(28.0))
        .height(Length::Fixed(28.0))
        .center_x(Length::Fixed(28.0))
        .center_y(Length::Fixed(28.0))
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(colour)),
            border: iced::Border {
                radius: 7.0.into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// What a category is for, by the words in its name; a tag when none says.
pub fn category_glyph(name: &str) -> &'static [u8] {
    let name = name.to_lowercase();
    let says = |words: &[&str]| words.iter().any(|w| name.contains(w));
    match () {
        _ if says(&["rent", "mortgage", "home", "household"]) => glyphs::HOME,
        _ if says(&["electric", "power", "utilit"]) => glyphs::ZAP,
        _ if says(&["water"]) => glyphs::DROPLET,
        _ if says(&["internet", "wifi"]) => glyphs::ONLINE,
        _ if says(&["phone", "mobile"]) => glyphs::PHONE,
        _ if says(&["insurance", "emergency"]) => glyphs::SHIELD,
        _ if says(&["grocer", "food", "supermarket"]) => glyphs::CART,
        _ if says(&["dining", "restaurant", "coffee", "eat"]) => glyphs::UTENSILS,
        _ if says(&["gas", "transit", "fuel", "commute"]) => glyphs::FUEL,
        _ if says(&["car", "repair", "maintenance"]) => glyphs::WRENCH,
        _ if says(&["entertain", "movie", "game", "fun"]) => glyphs::PLAY,
        _ if says(&["subscription", "streaming"]) => glyphs::REPEAT,
        _ if says(&["cloth", "shoe"]) => glyphs::SHIRT,
        _ if says(&["health", "medical", "doctor", "pharmacy"]) => glyphs::HEART,
        _ if says(&["vacation", "trip", "travel", "flight"]) => glyphs::SEND,
        _ if says(&["gift", "holiday", "birthday"]) => glyphs::GIFT,
        _ if says(&["laptop", "computer", "tech"]) => glyphs::LAPTOP,
        _ if says(&["saving", "invest", "retire"]) => glyphs::GROWTH,
        _ => glyphs::TAG,
    }
}

/// What a category's pill says by its colour, as YNAB's does: red for cash
/// overspent (next month pays for it), amber for overspending that was
/// all on a card (it is debt now) and for a card's payment that does not
/// cover what the card owes, else as its target reads it.
pub fn tone(l: &BudgetLine) -> Tone {
    match () {
        _ if l.available < 0 && l.credit_overspent >= -l.available => Tone::Short,
        _ if l.available >= 0 && l.card.is_some() && l.underfunded > 0 => Tone::Short,
        _ => Tone::of(l.available, l.target, l.assigned),
    }
}

/// How far along a category is: the share of its goal given this month,
/// as the inspector counts it; for one with no goal, the share of what it
/// had that was spent; blank for neither.
fn progress(l: &BudgetLine) -> String {
    let had = l.carried + l.assigned;
    match () {
        // A card's: what of its debt is not covered yet.
        _ if l.card.is_some() && l.underfunded > 0 => format!("{} short", money(l.underfunded)),
        _ if l.card.is_some() && l.debt > 0 => "Covered".into(),
        _ if l.card.is_some() => String::new(),
        _ if l.credit_overspent > 0 => format!("{} on card", money(l.credit_overspent)),
        _ if l.target > 0 => format!("{}% goal", (l.assigned.max(0) * 100 / l.target).min(999)),
        _ if l.activity < 0 && had > 0 => format!("{}% used", (-l.activity * 100 / had).min(999)),
        _ => String::new(),
    }
}

impl App {
    /// The box a category is typed into: the cell itself, what it says now
    /// greyed under what is typed, `<Enter>` choosing the row with the keys.
    fn drop_box<'a>(&'a self, now: &str) -> Element<'a, Message> {
        let query = self.dropdown.as_ref().map(|d| d.search.query.as_str()).unwrap_or_default();
        text_input(now, query)
            .id(look::DROP_INPUT)
            .on_input(Message::DropTyped)
            .on_submit(Message::DropChoose)
            .size(13)
            .padding([4, 6])
            .style(look::input)
            .into()
    }
}

/// A store as the receipts page names it.
pub(crate) fn store_name(store: &str) -> String {
    match store {
        "amazon" => "Amazon".into(),
        "target" => "Target".into(),
        "costco" => "Costco".into(),
        s => s.to_string(),
    }
}

/// How many, in thousandths, as a line says it: `2`, `11.482 gal`.
pub(crate) fn quantity(thousandths: i64, unit: &str) -> String {
    let n = format::quantity(thousandths);
    match unit {
        "" | "ea" => format!("\u{00d7}{n}"),
        u => format!("{n} {u}"),
    }
}
