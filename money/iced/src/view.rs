//! What the window draws: the sidebar of accounts, and beside it the page —
//! the month's budget and its inspector, an account's register, or where
//! the month's money went — with a panel over it when one is open.

use arkui::text::{self, text};
use arkui::text_input::text_input;
use arkui::{format, glyphs, icon, layer, menu, panel, style, table};
use iced::widget::{button, column, container, mouse_area, progress_bar, row, rule, scrollable, stack, Column, Space};
use iced::{Alignment, Element, Length, Padding};
use money_core::budget::ready_to_assign;
use money_core::*;

use crate::amount::money;
use crate::look::{self, Looks, Tone};
use crate::peer::Ledger;
use crate::{App, Choice, Choosing, Field, Message, Modal, Page, Pane, SplitRow, TxnForm};

/// How tall a line of the budget is, a group's heading or a category:
/// one height, so a drag is arithmetic (`arkui::outline`).
pub const LINE: f32 = 52.0;
/// The register's geometry: the bars over its table, the table's heading
/// and every row — one height each, so where a row is on screen is
/// arithmetic (`App::category_at`).
pub const REVIEW_BAR: f32 = 34.0;
pub const LEDGER_BALANCES: f32 = 64.0;
pub const FILTER_BAR: f32 = 40.0;
pub const LEDGER_HEAD: f32 = 28.0;
pub const TXN_ROW: f32 = 24.0;
/// The register's fixed columns: Date, Amount, Cleared.
pub const DATE_COL: f32 = 112.0;
pub const AMOUNT_COL: f32 = arkui::layout_tokens::AMOUNT_COLUMN_WIDTH;
pub const CLEARED_COL: f32 = 76.0;
pub const PAD: f32 = 18.0;

/// How far down Insights or Settings is scrolled once its title, the
/// page's first line, has gone off the top: the padding over it and its
/// line (Insights's is its arrows', 32 tall). Sooner, and the small title
/// came while the large one was still there to read under the band.
pub const TITLED: f32 = PAD + 32.0;

impl App {
    /// What the small header says on this page once its title has scrolled
    /// away (`arkui::compact`): Insights's and Settings' titles. The budget
    /// and the registers keep their headings over their lists, so they
    /// have none.
    pub(crate) fn compact_title(&self) -> Option<String> {
        // Every page's title is the shell's toolbar's, inline or large, and
        // stays where it is.
        let _ = self.page;
        None
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
                    left: self.page_origin().x + 1.0 + PAD - 6.0,
                    right: PAD + arkui::account::HEIGHT + PAD,
                    bottom: 0.0,
                })
                .height(Length::Fixed(arkui::account::band(PAD)))
                .align_y(Alignment::Center)
                .into(),
        )
    }
}

pub(crate) fn bold<'a>(s: impl Into<String>, size: u32) -> iced::widget::Text<'a> {
    text(s.into()).size(size).font(arkui::hero::BOLD)
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
        .style(arkui::style::chip(false))
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

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        // The shell (sections.rs): its sections, toolbars, search and
        // inspector, each section's page hosted in it; a swipe's arrow over it.
        let page = self.history.view(self.view_shell(), iced::time::Instant::now());
        // Nothing under the page: it runs to the window's foot. What the
        // last thing done came to, and a search or a count as it is typed,
        // are said for a moment in a pill laid over it (`arkui::toast`,
        // below) — a line under it, kept there by a note that was nearly
        // always there, cut off whatever the page had at its foot.
        // The sidebar is a surface, a step off the page's ground, with a
        // hairline where it meets the page, as harken's is.
        let base = container(page).width(Length::Fill).height(Length::Fill).style(style::page);
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
        // Who is signed in is at the sidebar's foot (`App::view_household`),
        // never over the toolbar.
        // The page's small header, once its title has scrolled away, in the
        // band beside the face.
        if let Some(small) = self.view_compact() {
            layers = layers.push(small);
        }
        // What the keys can do next, at the bottom right, over the page.
        if let Some(prefix) = self.keyboard.which.showing() {
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
        let pending = self.keyboard.keys.pending();
        if let Some(words) = arkui::toast::says(&self.toast, &pending) {
            layers = layers.push(arkui::toast::view(words, arkui::toast::MARGIN));
        }
        // The window's root: arkui's own needs of the platform.
        arkui::window::view(layers)
    }

    // * The sidebar

    // * The budget

    pub(crate) fn view_inspector(&self) -> Element<'_, Message> {
        let body = match self.selected.and_then(|c| self.peer.line(c)) {
            Some(l) => self.inspect_category(l),
            None => self.inspect_month(),
        };
        // The shell's pane holds it (`arkui::view::shell`): as wide as the
        // pane, under the toolbar.
        container(arkui::scroll::elastic(
            scrollable(body.spacing(12).padding(PAD)).height(Length::Fill).style(style::bars),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    pub(crate) fn figure<'a>(label: &str, cents: i64) -> Element<'a, Message> {
        row![text(label.to_string()).size(12).width(Length::Fill), text(money(cents)).size(12)]
            .align_y(Alignment::Center)
            .into()
    }

    fn inspect_month(&self) -> Column<'_, Message> {
        let s = self.peer.summary();
        let lines = self.peer.lines();
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
        .style(arkui::style::card);
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
        .style(arkui::style::card);
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
        .style(arkui::style::card);
        column![summary, auto, shared]
    }

    fn inspect_category<'a>(&'a self, l: &'a BudgetLine) -> Column<'a, Message> {
        let tone = l.tone();
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
                    .style(arkui::style::pill(tone.tint())),
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
        let head = container(figures).padding(14).style(arkui::style::card);

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
            target = target.push(
                progress_bar(0.0..=1.0, funded)
                    .girth(Length::Fixed(6.0))
                    .style(arkui::style::progress(tone.bar())),
            );
            if have < l.target && l.ruled {
                target = target.push(dim("Its formula assigns it: what its filters add up to decides.", 11));
            } else if have < l.target {
                let need = l.target - have;
                target = target.push(action(
                    &format!("Assign {} to meet target", money(need)),
                    Message::AssignTo(l.category_id, l.target),
                ));
            } else {
                target = target.push(text("Funded this month").size(12).style(Tone::Positive.text()));
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
                    .style(arkui::style::field),
                quiet("Save", Message::TargetDone),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
        let target = container(target).padding(14).style(arkui::style::card);

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
        .style(arkui::style::card);

        let mut kinds = column![
            bold("Kinds", 14),
            dim(
                "Kinds of it, from every store: Eggs under Groceries. Their spending is this category's.",
                11
            ),
        ]
        .spacing(6);
        for n in self.peer.tree().iter().filter(|n| n.depth >= 2 && self.is_under(n, l.category_id)) {
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
                    .style(arkui::style::field),
                quiet("Add", Message::NewKindDone),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
            _ => link("+ Add a kind", Message::NewKind(l.category_id)),
        });
        let kinds = container(kinds).padding(14).style(arkui::style::card);

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
                    .style(if l.underfunded > 0 { Tone::Short } else { Tone::Positive }.text()),
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
            card = card.push(text("Enough set aside to pay it").size(12).style(Tone::Positive.text()));
        } else {
            card = card.push(dim("Nothing owed on it.", 11));
        }
        card = card.push(dim(
            "Spending on the card moves what its category had here; what a category could not cover is debt to assign money to.",
            11,
        ));
        let hidden = l.hidden;
        column![
            container(card).padding(14).style(arkui::style::card),
            link(if hidden { "Unhide" } else { "Hide" }, Message::HideCategory(l.category_id, !hidden)),
        ]
        .spacing(12)
        .into()
    }

    /// Whether a category of the tree is under `id`, at any depth.
    fn is_under(&self, n: &CategoryNode, id: Id<Category>) -> bool {
        let mut at = n.parent_id;
        for _ in 0..money_core::budget::DEEPEST {
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
            .style(arkui::style::chip(false))
            .into()
    }

    // * The registers

    /// How tall the top above the register's table is: the review bar while
    /// the bank's imports wait, an account's balances, the bar of
    /// conditions while it shows.
    pub fn ledger_top(&self) -> f32 {
        let mut top = 0.0;
        if self.review_count() > 0 {
            top += REVIEW_BAR;
        }
        if self.register_account().is_some() {
            top += LEDGER_BALANCES;
        }
        if self.filter_shown() {
            top += FILTER_BAR;
        }
        top
    }

    /// The bank's imports in the register on screen that wait for a look,
    /// unless the register is that queue already.
    pub fn review_count(&self) -> usize {
        match self.peer.ledger {
            Ledger::Inbox => 0,
            _ => self.peer.register().iter().filter(|t| !t.approved).count(),
        }
    }

    /// The bar of conditions: when asked for (the toolbar's Filter), or
    /// while the register is one no section names (a saved filter, or
    /// conditions changed).
    pub fn filter_shown(&self) -> bool {
        self.show_filter || matches!(self.peer.ledger, Ledger::Saved(_) | Ledger::Adhoc)
    }

    /// The register for a filter, as a Mac's table: the bank's imports
    /// waiting for review in a bar over it (Review, Approve All), an
    /// account's cleared, uncleared and working balances and its bank's
    /// sync, the bar of conditions when asked for, then the table — Date,
    /// Payee, Category, Account (unless it is one account's), Memo,
    /// Amount, Cleared — striped, a dot for what waits for a look. When
    /// compact, each transaction is two lines.
    pub(crate) fn view_ledger(&self, _ledger: Ledger) -> Element<'_, Message> {
        let rows = self.peer.register();
        let account = self.register_account().and_then(|a| self.peer.account(a));
        // The account column, unless every row is one account's.
        let all = account.is_none();
        let compact = arkui::shell::SizeClass::of(self.window.width) == arkui::shell::SizeClass::Compact;
        let mut top = column![];

        let waiting = self.review_count();
        if waiting > 0 {
            let unapproved: Vec<Id<Txn>> = rows.iter().filter(|t| !t.approved).map(|t| t.id).collect();
            let words = format!("{waiting} new transaction{} from your banks", if waiting == 1 { "" } else { "s" });
            let actions = vec![
                arkui::figures::capsule_button(None, "Review", false, Message::Go(Page::Ledger(Ledger::Inbox))),
                arkui::figures::capsule_button(None, "Approve All", true, Message::Approve(unapproved)),
            ];
            top = top.push(container(arkui::figures::notice_bar(glyphs::INBOX, &words, actions)).height(Length::Fixed(REVIEW_BAR)));
        }
        if let Some(a) = account {
            let tint = |c: i64| match c < 0 {
                true => arkui::parts::Tint::Negative,
                false => arkui::parts::Tint::Neutral,
            };
            let strip = arkui::figures::figure_strip(vec![
                ("Cleared".into(), money(a.cleared), tint(a.cleared)),
                ("Uncleared".into(), money(a.uncleared), tint(a.uncleared)),
                ("Working Balance".into(), money(a.balance), tint(a.balance)),
            ]);
            let now = crate::overview::now_ms(&self.today);
            let synced = match self.peer.banks().iter().find(|b| !a.bank.is_empty() && b.name == a.bank) {
                Some(b) => format!("{} \u{b7} Synced {}", a.subtitle(), format::ago(b.synced_ms, now)),
                None => format!("{} \u{b7} Not connected to a bank", a.subtitle()),
            };
            let mut sync = row![
                icon::tinted(glyphs::CLOUD, 12.0, |p| p.background.base.text.scale_alpha(0.5)),
                dim(synced, 11)
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            if a.unapproved > 0 {
                sync = sync.push(link(format!("{} to review", a.unapproved), Message::Go(Page::Ledger(Ledger::Inbox))));
            }
            top = top.push(
                container(column![strip, sync].spacing(4))
                    .padding([8.0, PAD])
                    .height(Length::Fixed(LEDGER_BALANCES)),
            );
        }
        if self.filter_shown() {
            let bar = row![self.view_filter_bar(), fill()]
                .push(row(self.filter_tools()).spacing(8))
                .align_y(Alignment::Center);
            top = top.push(container(bar).padding([4.0, PAD]).height(Length::Fixed(FILTER_BAR)).clip(true));
        }

        let mut head = row![].align_y(Alignment::Center);
        if !compact {
            head = head
                .push(table::heading("Date \u{2304}", Length::Fixed(DATE_COL)))
                .push(table::heading("Payee", Length::FillPortion(3)))
                .push(table::heading("Category", Length::FillPortion(3)));
            if all {
                head = head.push(table::heading("Account", Length::FillPortion(2)));
            }
            head = head
                .push(table::heading("Memo", Length::FillPortion(2)))
                .push(right(
                    text("Amount").size(table::HEADING_SIZE).style(style::faint(0.55)).into(),
                    AMOUNT_COL,
                ))
                .push(
                    container(text("Cleared").size(table::HEADING_SIZE).style(style::faint(0.55)))
                        .padding(Padding::ZERO.left(14))
                        .width(Length::Fixed(CLEARED_COL)),
                );
        }

        let mut body: Column<'_, Message> = Column::new();
        if let Some(f) = self.form.as_ref().filter(|f| f.editing.is_none()) {
            body = body.push(self.view_form(f, all));
        }
        let lit = self.txn_lit();
        let focused = self.keyboard.focus == Pane::Content && self.keys_free();
        for (i, t) in rows.iter().enumerate() {
            match &self.form {
                Some(f) if f.editing == Some(t.id) => body = body.push(self.view_form(f, all)),
                _ => {
                    let on_cursor = lit.is_some_and(|(a, b)| a <= i && i <= b);
                    body = body.push(match compact {
                        true => self.txn_lines(t, all, on_cursor, focused),
                        false => self.txn_row(t, all, i % 2 == 1, on_cursor, focused),
                    });
                }
            }
        }
        if rows.is_empty() {
            body = body.push(container(dim("No Transactions", 13)).center_x(Length::Fill).padding(40));
        }

        // The top is the sum of its bars, and every row one height, so where
        // a row's category is on screen is arithmetic (`App::category_at`):
        // the dropdown opens under it, keys or pointer.
        let mut table = column![];
        if !compact {
            table = table
                .push(
                    container(table::row(head, false, false, false))
                        .padding([0.0, PAD - 6.0])
                        .height(Length::Fixed(LEDGER_HEAD)),
                )
                .push(rule::horizontal(1).style(style::hairline));
        }
        table = table.push(arkui::scroll::elastic(
            scrollable(body.padding(Padding {
                top: 4.0,
                right: PAD - 6.0,
                bottom: PAD,
                left: PAD - 6.0,
            }))
            .id(look::REGISTER)
            .on_scroll(|v| Message::RegisterScrolled(v.absolute_offset().y))
            .height(Length::Fill)
            .style(style::bars),
        ));
        column![top, table].width(Length::Fill).into()
    }

    /// A transaction's row in the table. `on_cursor` is the register's
    /// cursor or its visual selection, lit while the register has the keys;
    /// a click puts the cursor on it, and on the cursor's row opens it to
    /// edit. Odd rows are striped, as a Mac's table is.
    fn txn_row<'a>(&'a self, t: &'a TxnLine, all: bool, odd: bool, on_cursor: bool, focused: bool) -> Element<'a, Message> {
        let lit = on_cursor && focused;
        let w = |n: f32| Length::Fixed(n);
        let needs = t.category == "Needs a category";
        // The category opens the dropdown, for this row — or for the whole
        // selection, when the row is in it; one the bank's import left
        // without is orange, with the menu's chevron.
        let words = match needs {
            true => "Needs Category \u{2304}".to_string(),
            false => t.category.clone(),
        };
        let label = text(words)
            .size(table::CELL_SIZE)
            .wrapping(text::Wrapping::None)
            .style(move |theme| text::Style {
                color: Some(match needs {
                    true => arkui::theme::of(theme).warning.base.color,
                    false => style::on_row(theme, lit, false, false),
                }),
                ..Default::default()
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
            (true, false) => table::cell(category_words(t), Length::FillPortion(3), lit, false, false),
            (false, false) => container(mouse_area(label).on_press(Message::CategorizeRow(t.id)))
                .width(Length::FillPortion(3))
                .padding(Padding {
                    right: table::SPACING,
                    ..Padding::ZERO
                })
                .clip(true)
                .into(),
        };
        // What waits for a look has a dot before its payee, as unread mail
        // does; a receipt found or a photo taken, a page after it.
        let approved = t.approved;
        let dot = container(Space::new().width(7).height(7)).style(move |theme| container::Style {
            background: (!approved).then(|| iced::Background::Color(arkui::theme::of(theme).primary.base.color)),
            border: iced::Border {
                radius: 4.0.into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        });
        let mut payee = row![dot, text(t.payee.clone()).size(table::CELL_SIZE).wrapping(text::Wrapping::None)]
            .spacing(7)
            .align_y(Alignment::Center);
        if !t.receipt.is_empty() || t.photos > 0 {
            let read = !t.receipt.is_empty();
            payee = payee.push(
                button(icon::tinted(glyphs::RECEIPT, 13.0, move |p| match read {
                    true => p.background.base.text.scale_alpha(0.6),
                    false => p.background.base.text.scale_alpha(0.35),
                }))
                .on_press(Message::EditTxn(t.id))
                .padding(1)
                .style(style::quiet),
            );
        }
        let mut line = row![
            table::cell(day(&t.date), w(DATE_COL), lit, false, false),
            container(payee).width(Length::FillPortion(3)).clip(true),
            category,
        ]
        .align_y(Alignment::Center);
        if all {
            line = line.push(table::cell(t.account.clone(), Length::FillPortion(2), lit, false, true));
        }
        line = line
            .push(table::cell(t.memo.clone(), Length::FillPortion(2), lit, false, true))
            .push(right(signed(t.amount, 13), AMOUNT_COL));
        let (mark, next) = match t.cleared {
            Cleared::Uncleared => (glyphs::UNTICK, Some(true)),
            Cleared::Cleared => (glyphs::TICKED, Some(false)),
            Cleared::Reconciled => (glyphs::RECONCILE, None),
        };
        let mut cleared = button(icon::tinted(mark, 14.0, move |p| match t.cleared {
            Cleared::Uncleared => p.background.base.text.scale_alpha(0.3),
            _ => p.success.base.color,
        }))
        .padding(2)
        .style(style::quiet);
        if let Some(c) = next {
            cleared = cleared.on_press(Message::ToggleCleared(t.id, c));
        }
        line = line.push(container(cleared).padding(Padding::ZERO.left(28)).width(w(CLEARED_COL)));
        mouse_area(
            table::row(line, on_cursor, focused, odd)
                .center_y(Length::Fixed(TXN_ROW))
                .style(move |theme| style::striped(theme, on_cursor, focused, odd)),
        )
        .on_press(Message::TxnClick(t.id))
        .into()
    }

    /// A transaction in two lines, as a compact list draws a table's first
    /// column: the payee and the amount, then its category (orange while it
    /// needs one) and account, and its day.
    fn txn_lines<'a>(&'a self, t: &'a TxnLine, all: bool, on_cursor: bool, focused: bool) -> Element<'a, Message> {
        let needs = t.category == "Needs a category";
        let approved = t.approved;
        let dot = container(Space::new().width(7).height(7)).style(move |theme| container::Style {
            background: (!approved).then(|| iced::Background::Color(arkui::theme::of(theme).primary.base.color)),
            border: iced::Border {
                radius: 4.0.into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        });
        let mut under = category_words(t);
        if all {
            under = format!("{under} \u{b7} {}", t.account);
        }
        let first = row![dot, text(t.payee.clone()).size(14).width(Length::Fill), signed(t.amount, 14)]
            .spacing(6)
            .align_y(Alignment::Center);
        let second = row![
            text(under).size(12).width(Length::Fill).style(move |theme| text::Style {
                color: Some(match needs {
                    true => arkui::theme::of(theme).warning.base.color,
                    false => arkui::theme::of(theme).background.base.text.scale_alpha(0.55),
                }),
                ..Default::default()
            }),
            dim(short_day(&t.date), 12),
        ]
        .padding(Padding::ZERO.left(13));
        column![
            mouse_area(
                container(column![first, second].spacing(2))
                    .padding([8, 4])
                    .style(move |theme| style::row(theme, on_cursor, focused, false)),
            )
            .on_press(Message::TxnClick(t.id)),
            rule::horizontal(1).style(style::hairline)
        ]
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
                .style(arkui::style::field)
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
                        .style(arkui::style::chip(false)),
                ),
                _ => Element::from(
                    button(text(category_label).size(13))
                        .on_press(Message::ChooseCategory)
                        .padding([4, 6])
                        .width(Length::FillPortion(3))
                        .style(arkui::style::chip(false)),
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
            for a in self.peer.accounts().iter().filter(|a| !a.closed) {
                under = under.push(
                    button(text(a.name.clone()).size(12))
                        .on_press(Message::FormAccount(a.id))
                        .padding([3, 8])
                        .style(arkui::style::chip(f.account == Some(a.id))),
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
                    .style(arkui::style::chip(false)),
                text_input("0.00", &r.amount)
                    .on_input(move |v| Message::SplitAmount(i, v))
                    .on_submit(Message::SaveTxn)
                    .size(12)
                    .padding([3, 6])
                    .width(Length::Fixed(94.0))
                    .style(arkui::style::field),
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
            l => text(format!("{} too much", money(-l))).size(11).style(Tone::Overspent.text()),
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
                ..Default::default()
            }))
            .on_press(Message::ChoosePayer(PaidFor {
                receipt_id: c.receipt_id.clone(),
                charge_id: Some(c.id.clone()),
            }))
            .padding([3, 6])
            .width(Length::Fill)
            .style(arkui::style::chip(false));
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
                        button(text(accounts::kind_title(k)).size(12))
                            .on_press(Message::AccountKind(k))
                            .padding([4, 8])
                            .style(arkui::style::chip(*kind == k)),
                    );
                }
                column![
                    bold("Add an account", 16),
                    text_input("Its name", name)
                        .on_input(Message::AccountName)
                        .size(13)
                        .padding([6, 8])
                        .style(arkui::style::field),
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
                        .style(arkui::style::field),
                    row![
                        button(text("Budget").size(12))
                            .on_press(Message::AccountOnBudget(true))
                            .padding([4, 10])
                            .style(arkui::style::chip(*on_budget)),
                        button(text("Tracking only").size(12))
                            .on_press(Message::AccountOnBudget(false))
                            .padding([4, 10])
                            .style(arkui::style::chip(!*on_budget)),
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
                                .style(arkui::style::field),
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
                let tone = Tone::of_amount(cents);
                r = r.push(
                    container(text(money(cents)).size(11))
                        .padding([1, 7])
                        .style(arkui::style::pill(tone.tint())),
                );
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
                        .style(arkui::style::field),
                );
                (*from, if short { "Cover it from".to_string() } else { "Move it to".to_string() })
            }
        };
        c = c.push(bold(title, 16));
        if let (Choosing::Form, Some(f)) = (choosing, &self.form) {
            if f.editing.is_none() {
                list = list.push(container(text("Transfer to").size(10).style(style::faint(0.5))).padding([6, 10]));
                for a in self.peer.accounts().iter().filter(|a| !a.closed && Some(a.id) != f.account) {
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
        for g in self.peer.groups().iter().filter(|g| !g.hidden) {
            list = list.push(container(text(g.name.clone()).size(10).style(style::faint(0.5))).padding([6, 10]));
            for l in self
                .peer
                .lines()
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
/// What a transaction's category cell says: a transfer's is Transfer
/// (its other account is in its payee), an import's without one says it
/// needs one.
fn category_words(t: &TxnLine) -> String {
    match (t.transfer, t.category.as_str()) {
        (true, "") => "Transfer".into(),
        (_, "Needs a category") => "Needs Category".into(),
        (_, c) => c.into(),
    }
}

/// A day as a table writes it: `Nov 18, 2026`.
fn day(date: &str) -> String {
    format::day_long(date)
}

/// A day as a compact row writes it: `Nov 18`.
fn short_day(date: &str) -> String {
    let long = format::day_long(date);
    long.split_once(',').map_or(long.clone(), |(d, _)| d.to_string())
}

/// An amount as a register writes it: money in with its `+`, in the
/// positive's green; money out as it is. Colour is never the only sign.
fn signed<'a>(cents: i64, size: u32) -> Element<'a, Message> {
    let words = match cents > 0 {
        true => format!("+{}", money(cents)),
        false => money(cents),
    };
    text(words)
        .size(size)
        .style(move |theme| text::Style {
            color: Some(match cents > 0 {
                true => arkui::theme::of(theme).success.base.color,
                false => arkui::theme::of(theme).background.base.text,
            }),
            ..Default::default()
        })
        .into()
}

fn right<'a>(content: Element<'a, Message>, width: f32) -> Element<'a, Message> {
    container(content)
        .width(Length::Fixed(width))
        .align_x(iced::alignment::Horizontal::Right)
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
            .style(arkui::style::field)
            .into()
    }
}
