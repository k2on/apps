//! The Budget tab, as one Ark app: the month's plan — Ready to Assign
//! across the top with Auto-Assign, then each group's categories with what
//! each was assigned, spent and has — and everything done to it: assigning,
//! moving money, a target, a new category or group, renaming, hiding,
//! deleting and moving categories and groups.
//!
//! The rows are the store's (`MoneyView::Groups`, `Lines` and `Summary`
//! of the month); [`State`] keeps what the screen is in the middle of — the
//! month the platform shows, the groups folded, a sheet being filled.
//! [`update`](Budget) decides what every button does; [`screen`](Budget)
//! works out every figure and word ([`BudgetPage`]); [`view::budget`] lays
//! it out once for every toolkit.
//!
//! The cards' group, Credit Card Payments, comes first, as YNAB draws it:
//! each card's payment category wears the card's mark and says how much of
//! what the card owes is set aside. Its categories are named, filled and
//! emptied as the cards are, so they offer no rename, target or delete. A
//! category whose month follows a rule offers no assigning or moving: its
//! menu says the rule.

use crate::routes::Route;
use ark::row::Id;
use ark_app::{App, Command, Data};
use ark_format::{amount, NumberStyle};
use ark_haptics::{perform, Haptic};
use money_core::words::Tone;
use money_core::*;

use crate::accounts::Style;
use crate::shell::inspect::tint_of;

pub mod view;

/// The screen, as `ark_app` runs it.
pub struct Budget;

/// An amount as a form has it: typed (a text field) or entered in cents
/// (the iPhone's amount entry).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Figure {
    pub typed: String,
    pub entered: Option<i64>,
}

impl Figure {
    /// In cents, never below nothing; none when what is typed is no amount.
    pub fn cents(&self, style: &NumberStyle) -> Option<i64> {
        match (self.entered, self.typed.trim()) {
            (Some(c), _) => Some(c.abs()),
            (None, "") => Some(0),
            (None, t) => ark_format::parse_amount(t, style).map(i64::abs),
        }
    }

    fn of(cents: i64, style: &NumberStyle) -> Figure {
        Figure {
            typed: match cents {
                0 => String::new(),
                c => ark_format::amount_editable(c, style),
            },
            entered: None,
        }
    }
}

/// A sheet over the budget, as it is being filled.
#[derive(Clone, Debug, PartialEq)]
pub enum Sheet {
    /// A category's assigned amount this month.
    Assign { id: String, figure: Figure },
    /// Money moved from one to another (none is Ready to Assign).
    Move {
        from: Option<String>,
        to: Option<String>,
        figure: Figure,
    },
    /// A category's monthly target.
    Target { id: String, figure: Figure },
    /// A new category in a group.
    NewCategory { group: String, name: String },
}

/// A name being typed in an alert.
#[derive(Clone, Debug, PartialEq)]
pub enum Naming {
    NewGroup(String),
    /// A group renamed: its id and the name typed.
    Group(String, String),
}

/// What the screen is in the middle of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The month shown (`2026-11`), as the platform pages it.
    pub month: String,
    /// The groups folded, by their hex ids.
    pub collapsed: Vec<String>,
    pub show_hidden: bool,
    /// Auto-Assign's question is up.
    pub auto: bool,
    pub sheet: Option<Sheet>,
    pub naming: Option<Naming>,
    pub style: Style,
}

/// Everything the Budget screen can be told. Ids are rows' `Id::hex`.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub enum BudgetMsg {
    /// The month the platform shows now (`2026-11`).
    Month {
        month: String,
    },
    /// Ask the bank for anything new (the list pulled down).
    Refresh,
    /// A category's month: Category Details, inspected or pushed.
    Open {
        id: String,
    },
    /// A group folded or unfolded.
    Toggle {
        id: String,
    },
    /// Hidden categories and groups shown, or not.
    ToggleHidden,
    /// Auto-Assign's question raised, and answered: each category given
    /// what it lacks of its target, what it was given last month, or nothing.
    AutoAssign,
    AutoUnderfunded,
    AutoLastMonth,
    AutoReset,
    CancelAuto,
    /// The Assign sheet raised for a category.
    Assign {
        id: String,
    },
    /// A category given the rest of its target this month, at once; a
    /// card's payment what covers the card.
    MeetTarget {
        id: String,
    },
    CoverCard {
        id: String,
    },
    /// Move Money's sheet raised from a category (into it, when it is overspent).
    Move {
        id: String,
    },
    /// Move Money's ends, by a category's hex (empty is Ready to Assign).
    MoveFrom {
        key: String,
    },
    MoveTo {
        key: String,
    },
    /// The target sheet raised for a category.
    Target {
        id: String,
    },
    /// New Category's sheet, in a group (empty: the first that takes one).
    NewCategory {
        group: String,
    },
    CategoryName {
        name: String,
    },
    CategoryGroup {
        key: String,
    },
    /// What a sheet's amount says: typed, or entered in cents.
    Typed {
        typed: String,
    },
    Cents {
        cents: i64,
    },
    /// The sheet's change sent, if it has no problem; or the sheet put away.
    Save,
    Dismiss,
    /// New Group's alert, a group's rename, what is typed, kept or not.
    NewGroup,
    RenameGroup {
        id: String,
    },
    Named {
        name: String,
    },
    SaveName,
    CancelName,
    /// A category or group hidden, or shown again.
    HideCategory {
        id: String,
    },
    HideGroup {
        id: String,
    },
    DeleteCategory {
        id: String,
    },
    DeleteGroup {
        id: String,
    },
    /// A category a place up or down its group; a group a place up or down.
    CategoryUp {
        id: String,
    },
    CategoryDown {
        id: String,
    },
    GroupUp {
        id: String,
    },
    GroupDown {
        id: String,
    },
    /// A menu's words that do nothing (a rule said).
    RuleSaid,
    /// What became of a change sent.
    Sent {
        refused: Option<String>,
    },
}

/// One category's row in the month.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct CategoryItem {
    pub id: String,
    /// Its emoji before its name, or its name alone (a card's payment).
    pub title: String,
    /// `credit` for a card's payment: the app's mark before its name.
    pub mark: Option<String>,
    pub faded: bool,
    pub fraction: Option<f64>,
    pub meter_tint: String,
    pub note: Option<String>,
    pub note_tint: String,
    pub note_end: Option<String>,
    pub assigned: i64,
    pub activity: i64,
    /// What it has, as its badge says it, and the badge's tint.
    pub status: String,
    pub status_tint: String,
    pub said: String,
    // Its menu.
    /// Its rule said, while one assigns it: the menu's first words.
    pub rule: String,
    pub ruled: bool,
    pub assignable: bool,
    /// "Assign $X to Cover the Card", while a card's payment is short.
    pub cover: String,
    pub can_cover: bool,
    /// "Assign $X to Meet Target", while it lacks some of its target.
    pub meet: String,
    pub can_meet: bool,
    /// "Move Money…", or "Cover Overspending…" while overspent.
    pub move_title: String,
    /// "Set Target…" or "Edit Target…".
    pub target_title: String,
    /// Not a card's payment: it has a target, a name and can go.
    pub own: bool,
    pub hide_title: String,
    pub hide_glyph: String,
    pub can_up: bool,
    pub can_down: bool,
}

/// One group of the month: its categories, its totals.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct GroupItem {
    pub id: String,
    pub title: String,
    pub assigned: i64,
    pub activity: i64,
    pub available: i64,
    /// The cards' group: what of their debt is not covered, while some is.
    pub uncovered: Option<String>,
    pub open: bool,
    /// Not the cards' group: renamed, deleted, given a category.
    pub own: bool,
    /// Offered a first category: it has none, and is no card's.
    pub empty: bool,
    pub hide_title: String,
    pub hide_glyph: String,
    pub can_up: bool,
    pub can_down: bool,
    pub said: String,
    pub lines: Vec<CategoryItem>,
}

/// A choice of a sheet's picker: a category, a group, Ready to Assign.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct BudgetChoice {
    pub key: String,
    pub title: String,
    pub glyph: String,
}

/// A sheet's amount, as it stands.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct AmountForm {
    /// What it is for: the category's name, or the move's way.
    pub subject: String,
    pub label: String,
    pub prompt: String,
    pub typed: String,
    pub cents: i64,
    pub problem: Option<String>,
    pub note: String,
    pub can_save: bool,
}

/// Move Money's sheet.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct MoveForm {
    pub from: String,
    pub to: String,
    pub choices: Vec<BudgetChoice>,
    pub amount: AmountForm,
}

/// New Category's sheet.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct CategoryForm {
    pub name: String,
    pub problem: Option<String>,
    pub group: String,
    pub groups: Vec<BudgetChoice>,
    pub can_save: bool,
}

/// A name's alert.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct NameForm {
    pub name: String,
}

/// Everything the Budget screen shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct BudgetPage {
    /// The month, "November 2026": the page's large title.
    pub title: String,
    /// "Ready to Assign", or "Assigned Too Much" below nothing.
    pub ready_label: String,
    pub ready: String,
    pub ready_tint: String,
    /// What needs attention, in words: "2 overspent · 3 underfunded".
    pub ready_caption: String,
    pub groups: Vec<GroupItem>,
    pub show_hidden: bool,
    pub auto: bool,
    pub assigning: Option<AmountForm>,
    pub moving: Option<MoveForm>,
    pub targeting: Option<AmountForm>,
    pub adding: Option<CategoryForm>,
    pub new_group: Option<NameForm>,
    pub renaming: Option<NameForm>,
}

/// The column titles over a group's figures.
pub const COLUMNS: [&str; 3] = ["Assigned", "Activity", "Available"];

fn us(s: &State) -> &NumberStyle {
    &s.style.0
}

fn id_of<T>(hex: &str) -> Option<Id<T>> {
    let b: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect::<Option<_>>()?;
    Some(Id::new(b.try_into().ok()?))
}

/// The month's lines, by group: the cards' first, then the rest in their
/// order, each category in its order; hidden ones only when asked.
pub fn grouped<'a>(groups: &'a [GroupLine], lines: &'a [BudgetLine], hidden: bool) -> Vec<(&'a GroupLine, Vec<&'a BudgetLine>)> {
    let cards = money_core::cards::card_group();
    let mut gs: Vec<&GroupLine> = groups.iter().filter(|g| hidden || !g.hidden).collect();
    gs.sort_by_key(|g| (g.id != cards, g.pos));
    gs.into_iter()
        .map(|g| {
            let mut ls: Vec<&BudgetLine> = lines.iter().filter(|l| l.group_id == g.id && (hidden || !l.hidden)).collect();
            ls.sort_by_key(|l| l.pos);
            (g, ls)
        })
        // The cards' group only while there is a card.
        .filter(|(g, ls)| g.id != cards || !ls.is_empty())
        .collect()
}

impl Budget {
    fn lines<'a>(s: &State, data: &Data<'a, MoneyView>) -> &'a [BudgetLine] {
        data.rows(&MoneyView::Lines { month: s.month.clone() })
    }
}

fn line<'a>(lines: &'a [BudgetLine], id: &str) -> Option<&'a BudgetLine> {
    lines.iter().find(|l| l.category_id.hex() == id)
}

fn send(change: MoneyChange) -> Vec<Command<Budget>> {
    if change.problem().is_some() {
        return vec![];
    }
    vec![Command::Send {
        change,
        then: Some(|r| BudgetMsg::Sent { refused: r.err() }),
    }]
}

/// The figure of the sheet up, changed by `f`.
fn figure(s: &mut State, f: impl FnOnce(&mut Figure)) {
    if let Some(Sheet::Assign { figure, .. } | Sheet::Move { figure, .. } | Sheet::Target { figure, .. }) = &mut s.sheet {
        f(figure)
    }
}

impl App for Budget {
    type Key = MoneyView;
    type State = State;
    type Msg = BudgetMsg;
    type Change = MoneyChange;
    type Route = Route;
    type Request = crate::MoneyRequest;
    type Screen = BudgetPage;

    fn holds(s: &State) -> Vec<MoneyView> {
        let mut v = vec![MoneyView::Groups];
        if !s.month.is_empty() {
            v.push(MoneyView::Lines { month: s.month.clone() });
            v.push(MoneyView::Summary { month: s.month.clone() });
        }
        v
    }

    fn update(s: &mut State, msg: BudgetMsg, data: &Data<'_, MoneyView>) -> Vec<Command<Self>> {
        use BudgetMsg as M;
        let lines = Self::lines(s, data);
        let month = s.month.clone();
        match msg {
            M::Month { month } => {
                let stepped = !s.month.is_empty() && s.month != month;
                s.month = month;
                match stepped {
                    true => vec![Command::ask(perform(Haptic::Selection))],
                    false => vec![],
                }
            }
            M::Refresh => vec![Command::Request(crate::MoneyRequest::Refresh)],
            M::Open { id } => match line(lines, &id) {
                Some(l) => vec![Command::Open(Route::Category { id, name: l.name.clone() })],
                None => vec![],
            },
            M::Toggle { id } => {
                match s.collapsed.iter().position(|g| *g == id) {
                    Some(i) => {
                        s.collapsed.remove(i);
                    }
                    None => s.collapsed.push(id),
                }
                vec![]
            }
            M::ToggleHidden => {
                s.show_hidden = !s.show_hidden;
                vec![]
            }
            M::AutoAssign => {
                s.auto = true;
                vec![]
            }
            M::CancelAuto => {
                s.auto = false;
                vec![]
            }
            M::AutoUnderfunded | M::AutoLastMonth | M::AutoReset => {
                s.auto = false;
                let how = match msg {
                    M::AutoUnderfunded => Auto::Underfunded,
                    M::AutoLastMonth => Auto::LastMonth,
                    _ => Auto::Reset,
                };
                send(MoneyChange::AutoAssign(AutoAssign { month, how, only: vec![] }))
            }
            M::Assign { id } => {
                if let Some(l) = line(lines, &id).filter(|l| l.assignable()) {
                    s.sheet = Some(Sheet::Assign {
                        figure: Figure::of(l.assigned, us(s)),
                        id,
                    });
                }
                vec![]
            }
            M::MeetTarget { id } => rest(lines, &id, &month, false),
            M::CoverCard { id } => rest(lines, &id, &month, true),
            M::Move { id } => {
                if let Some(l) = line(lines, &id).filter(|l| l.assignable()) {
                    let (from, to) = match l.available < 0 {
                        true => (None, Some(id)),
                        false => (Some(id), None),
                    };
                    s.sheet = Some(Sheet::Move {
                        figure: Figure::of(l.available.abs(), us(s)),
                        from,
                        to,
                    });
                }
                vec![]
            }
            M::MoveFrom { key } => {
                if let Some(Sheet::Move { from, .. }) = &mut s.sheet {
                    *from = (!key.is_empty()).then_some(key);
                }
                vec![]
            }
            M::MoveTo { key } => {
                if let Some(Sheet::Move { to, .. }) = &mut s.sheet {
                    *to = (!key.is_empty()).then_some(key);
                }
                vec![]
            }
            M::Target { id } => {
                if let Some(l) = line(lines, &id).filter(|l| !l.is_payment()) {
                    s.sheet = Some(Sheet::Target {
                        figure: Figure::of(l.target, us(s)),
                        id,
                    });
                }
                vec![]
            }
            M::NewCategory { group } => {
                let groups: &[GroupLine] = data.rows(&MoneyView::Groups);
                let cards = money_core::cards::card_group();
                let group = match groups.iter().any(|g| g.id.hex() == group && g.id != cards) {
                    true => group,
                    false => grouped(groups, lines, false)
                        .iter()
                        .find(|(g, _)| g.id != cards)
                        .map(|(g, _)| g.id.hex())
                        .unwrap_or_default(),
                };
                s.sheet = Some(Sheet::NewCategory { group, name: String::new() });
                vec![]
            }
            M::CategoryName { name } => {
                if let Some(Sheet::NewCategory { name: n, .. }) = &mut s.sheet {
                    *n = name;
                }
                vec![]
            }
            M::CategoryGroup { key } => {
                if let Some(Sheet::NewCategory { group, .. }) = &mut s.sheet {
                    *group = key;
                }
                vec![]
            }
            M::Typed { typed } => {
                figure(s, |f| {
                    f.typed = typed;
                    f.entered = None;
                });
                vec![]
            }
            M::Cents { cents } => {
                figure(s, |f| f.entered = Some(cents.abs()));
                vec![]
            }
            M::Save => {
                let Some(sheet) = &s.sheet else { return vec![] };
                match change(sheet, &month, us(s)) {
                    Some(c) => send(c),
                    None => vec![],
                }
            }
            M::Dismiss => {
                s.sheet = None;
                vec![]
            }
            M::NewGroup => {
                s.naming = Some(Naming::NewGroup(String::new()));
                vec![]
            }
            M::RenameGroup { id } => {
                let groups: &[GroupLine] = data.rows(&MoneyView::Groups);
                s.naming = groups
                    .iter()
                    .find(|g| g.id.hex() == id && g.id != money_core::cards::card_group())
                    .map(|g| Naming::Group(id, g.name.clone()));
                vec![]
            }
            M::Named { name } => {
                match &mut s.naming {
                    Some(Naming::NewGroup(n) | Naming::Group(_, n)) => *n = name,
                    None => {}
                }
                vec![]
            }
            M::SaveName => match s.naming.take() {
                Some(Naming::NewGroup(name)) => send(MoneyChange::AddGroup(AddGroup { name: name.trim().into() })),
                Some(Naming::Group(id, name)) => match id_of(&id) {
                    Some(id) => send(MoneyChange::RenameGroup(RenameGroup {
                        id,
                        name: name.trim().into(),
                    })),
                    None => vec![],
                },
                None => vec![],
            },
            M::CancelName => {
                s.naming = None;
                vec![]
            }
            M::HideCategory { id } => match line(lines, &id) {
                Some(l) => send(MoneyChange::HideCategory(HideCategory {
                    id: l.category_id,
                    hidden: !l.hidden,
                })),
                None => vec![],
            },
            M::HideGroup { id } => {
                let groups: &[GroupLine] = data.rows(&MoneyView::Groups);
                match groups.iter().find(|g| g.id.hex() == id) {
                    Some(g) => send(MoneyChange::HideGroup(HideGroup { id: g.id, hidden: !g.hidden })),
                    None => vec![],
                }
            }
            M::DeleteCategory { id } => match line(lines, &id).filter(|l| !l.is_payment()) {
                Some(l) => send(MoneyChange::DeleteCategory(DeleteCategory {
                    id: l.category_id,
                    move_to: None,
                })),
                None => vec![],
            },
            M::DeleteGroup { id } => match id_of::<Group>(&id).filter(|g| *g != money_core::cards::card_group()) {
                Some(id) => send(MoneyChange::DeleteGroup(DeleteGroup { id, move_to: None })),
                None => vec![],
            },
            M::CategoryUp { id } => step_category(data, lines, &id, -1),
            M::CategoryDown { id } => step_category(data, lines, &id, 1),
            M::GroupUp { id } => step_group(data, lines, &id, -1),
            M::GroupDown { id } => step_group(data, lines, &id, 1),
            M::RuleSaid => vec![],
            M::Sent { refused } => match refused {
                Some(_) => vec![],
                None => {
                    let assigned = matches!(s.sheet, Some(Sheet::Assign { .. } | Sheet::Move { .. }));
                    s.sheet = None;
                    match assigned {
                        true => vec![Command::ask(perform(Haptic::Success))],
                        false => vec![],
                    }
                }
            },
        }
    }

    fn screen(s: &State, data: &Data<'_, MoneyView>) -> BudgetPage {
        let style = us(s);
        let a = |c: i64| amount(c, style);
        let lines = Self::lines(s, data);
        let groups: &[GroupLine] = data.rows(&MoneyView::Groups);
        let summary: Option<&Summary> = data.rows::<Summary>(&MoneyView::Summary { month: s.month.clone() }).first();
        let cards = money_core::cards::card_group();
        let shown = grouped(groups, lines, s.show_hidden);
        let n = shown.len();
        let groups_out = shown
            .iter()
            .enumerate()
            .map(|(gi, (g, ls))| {
                let is_cards = g.id == cards;
                let available: i64 = ls.iter().map(|l| l.available).sum();
                let uncovered: i64 = match is_cards {
                    true => money_core::words::uncovered(&ls.iter().map(|l| (*l).clone()).collect::<Vec<_>>()),
                    false => 0,
                };
                let m = ls.len();
                GroupItem {
                    id: g.id.hex(),
                    title: g.name.clone(),
                    assigned: ls.iter().map(|l| l.assigned).sum(),
                    activity: ls.iter().map(|l| l.activity).sum(),
                    available,
                    uncovered: (uncovered > 0).then(|| format!("{} not covered", a(uncovered))),
                    open: !s.collapsed.contains(&g.id.hex()),
                    own: !is_cards,
                    empty: ls.is_empty() && !is_cards,
                    hide_title: if g.hidden { "Show" } else { "Hide" }.into(),
                    hide_glyph: if g.hidden { "eye" } else { "eye-off" }.into(),
                    // The cards' group stays first.
                    can_up: !is_cards && gi > 0 && shown[gi - 1].0.id != cards,
                    can_down: !is_cards && gi + 1 < n,
                    said: match uncovered > 0 {
                        true => format!("{}, {} available, {} of the cards' debt not covered", g.name, a(available), a(uncovered)),
                        false => format!("{}, {} available", g.name, a(available)),
                    },
                    lines: ls.iter().enumerate().map(|(i, l)| category(l, style, i > 0, i + 1 < m)).collect(),
                }
            })
            .collect();
        let summary = summary.cloned().unwrap_or_default();
        let mut said = vec![];
        if summary.overspent > 0 {
            said.push(format!("{} overspent", summary.overspent));
        }
        if summary.underfunded > 0 {
            said.push(format!("{} underfunded", summary.underfunded));
        }
        let caption = match (said.is_empty(), summary.ready_to_assign) {
            (false, _) => said.join(" \u{b7} "),
            (true, 0) => "Every dollar has a job.".into(),
            (true, _) => "Give these dollars a job.".into(),
        };
        BudgetPage {
            title: ark_format::month(&s.month),
            ready_label: if summary.ready_to_assign < 0 {
                "Assigned Too Much"
            } else {
                "Ready to Assign"
            }
            .into(),
            ready: a(summary.ready_to_assign),
            ready_tint: tint_of(summary.tone()).into(),
            ready_caption: caption,
            groups: groups_out,
            show_hidden: s.show_hidden,
            auto: s.auto,
            assigning: match &s.sheet {
                Some(Sheet::Assign { id, figure }) => line(lines, id).map(|l| {
                    amount_form(
                        l.name.clone(),
                        "Assigned This Month",
                        "Assigned this month",
                        figure,
                        format!("{} available now.", a(l.available)),
                        style,
                    )
                }),
                _ => None,
            },
            moving: match &s.sheet {
                Some(Sheet::Move { from, to, figure }) => Some(move_form(lines, from, to, figure, style)),
                _ => None,
            },
            targeting: match &s.sheet {
                Some(Sheet::Target { id, figure }) => line(lines, id).map(|l| {
                    amount_form(
                        l.name.clone(),
                        "Monthly Target",
                        "Needed each month",
                        figure,
                        "What the month should give it; none is no target.".into(),
                        style,
                    )
                }),
                _ => None,
            },
            adding: match &s.sheet {
                Some(Sheet::NewCategory { group, name }) => Some(category_form(groups, group, name)),
                _ => None,
            },
            new_group: match &s.naming {
                Some(Naming::NewGroup(name)) => Some(NameForm { name: name.clone() }),
                _ => None,
            },
            renaming: match &s.naming {
                Some(Naming::Group(_, name)) => Some(NameForm { name: name.clone() }),
                _ => None,
            },
        }
    }
}

/// A category given the rest of its target this month, or (`cover`) a
/// card's payment what covers its card.
fn rest(lines: &[BudgetLine], id: &str, month: &str, cover: bool) -> Vec<Command<Budget>> {
    match line(lines, id).filter(|l| l.assignable()) {
        Some(l) => send(MoneyChange::Assign(Assign {
            month: month.into(),
            category_id: l.category_id,
            amount: match cover {
                true => l.to_cover().unwrap_or(l.assigned),
                false => l.assigned.max(l.target),
            },
        })),
        None => vec![],
    }
}

/// A category moved a place up (`-1`) or down its group: in front of the
/// one before it, or of the one after the next (its group's end).
fn step_category(data: &Data<'_, MoneyView>, lines: &[BudgetLine], id: &str, by: i64) -> Vec<Command<Budget>> {
    let groups: &[GroupLine] = data.rows(&MoneyView::Groups);
    let all = grouped(groups, lines, true);
    let Some((_, ls)) = all.iter().find(|(_, ls)| ls.iter().any(|l| l.category_id.hex() == id)) else {
        return vec![];
    };
    let i = ls.iter().position(|l| l.category_id.hex() == id).unwrap_or(0) as i64;
    let before = match by {
        -1 if i > 0 => Some(ls[i as usize - 1].category_id),
        1 if i + 1 < ls.len() as i64 => ls.get(i as usize + 2).map(|l| l.category_id),
        _ => return vec![],
    };
    let l = ls[i as usize];
    send(MoneyChange::MoveCategory(MoveCategory {
        id: l.category_id,
        group_id: l.group_id,
        before,
    }))
}

/// A group moved a place up or down, never before the cards' own.
fn step_group(data: &Data<'_, MoneyView>, lines: &[BudgetLine], id: &str, by: i64) -> Vec<Command<Budget>> {
    let groups: &[GroupLine] = data.rows(&MoneyView::Groups);
    let cards = money_core::cards::card_group();
    let mut gs: Vec<&GroupLine> = groups.iter().filter(|g| g.id != cards).collect();
    gs.sort_by_key(|g| g.pos);
    let _ = lines;
    let Some(i) = gs.iter().position(|g| g.id.hex() == id) else {
        return vec![];
    };
    let before = match by {
        -1 if i > 0 => Some(gs[i - 1].id),
        1 if i + 1 < gs.len() => gs.get(i + 2).map(|g| g.id),
        _ => return vec![],
    };
    send(MoneyChange::MoveGroup(MoveGroup { id: gs[i].id, before }))
}

/// The change a sheet makes, once it has no problem.
fn change(sheet: &Sheet, month: &str, style: &NumberStyle) -> Option<MoneyChange> {
    let c = match sheet {
        Sheet::Assign { id, figure } => MoneyChange::Assign(Assign {
            month: month.into(),
            category_id: id_of(id)?,
            amount: figure.cents(style)?,
        }),
        Sheet::Move { from, to, figure } => {
            if from == to {
                return None;
            }
            MoneyChange::MoveMoney(MoveMoney {
                month: month.into(),
                from: from.as_deref().and_then(id_of),
                to: to.as_deref().and_then(id_of),
                amount: figure.cents(style)?,
            })
        }
        Sheet::Target { id, figure } => MoneyChange::SetTarget(SetTarget {
            id: id_of(id)?,
            target: figure.cents(style)?,
        }),
        Sheet::NewCategory { group, name } => MoneyChange::AddCategory(AddCategory {
            group_id: id_of(group)?,
            name: name.trim().into(),
        }),
    };
    c.problem().is_none().then_some(c)
}

fn category(l: &BudgetLine, style: &NumberStyle, can_up: bool, can_down: bool) -> CategoryItem {
    let a = |c: i64| amount(c, style);
    let tone = l.tone();
    let card = l.is_payment();
    let meter = l.meter();
    let (fraction, meter_tint, note, note_tint, note_end) = match (card, meter) {
        (true, Some(m)) => {
            let words = l.captions(style);
            let covered = m.fraction() >= 1.0;
            (
                Some(m.fraction()),
                if covered { "positive" } else { "caution" },
                Some(words.left),
                if l.underfunded > 0 { "caution" } else { "positive" },
                Some(words.right),
            )
        }
        (true, None) => (None, "positive", Some("Nothing owed".into()), "neutral", None),
        (false, m) => {
            let worded = matches!(tone, Tone::Overspent | Tone::Short | Tone::OnCard);
            (
                m.map(|m| m.fraction()),
                m.map(|m| tint_of(m.tone)).unwrap_or("positive"),
                worded.then(|| l.story(style)),
                tint_of(tone),
                None,
            )
        }
    };
    let emoji = crate::look::category_emoji(&l.name);
    let rest = l.to_target();
    CategoryItem {
        id: l.category_id.hex(),
        title: match (card, emoji) {
            (false, e) if !e.is_empty() => format!("{e} {}", l.name),
            _ => l.name.clone(),
        },
        mark: card.then(|| "credit".into()),
        faded: l.hidden,
        fraction,
        meter_tint: meter_tint.into(),
        note,
        note_tint: note_tint.into(),
        note_end,
        assigned: l.assigned,
        activity: l.activity,
        status: a(l.available),
        status_tint: tint_of(tone).into(),
        said: format!("{}, {}", l.name, l.spoken(style)),
        rule: l.rule_words().unwrap_or_default(),
        ruled: l.ruled,
        assignable: l.assignable(),
        cover: format!("Assign {} to Cover the Card", a(l.underfunded)),
        can_cover: l.assignable() && l.to_cover().is_some(),
        meet: format!("Assign {} to Meet Target", a(rest)),
        can_meet: l.assignable() && !card && rest > 0,
        move_title: if l.available < 0 { "Cover Overspending…" } else { "Move Money…" }.into(),
        target_title: if l.target > 0 { "Edit Target…" } else { "Set Target…" }.into(),
        own: !card,
        hide_title: if l.hidden { "Show" } else { "Hide" }.into(),
        hide_glyph: if l.hidden { "eye" } else { "eye-off" }.into(),
        can_up,
        can_down,
    }
}

fn amount_form(subject: String, label: &str, prompt: &str, f: &Figure, note: String, style: &NumberStyle) -> AmountForm {
    let cents = f.cents(style);
    AmountForm {
        subject,
        label: label.into(),
        prompt: prompt.into(),
        typed: f.typed.clone(),
        cents: cents.unwrap_or(0),
        problem: cents.is_none().then(|| "That isn't an amount".into()),
        note,
        can_save: cents.is_some(),
    }
}

/// The key Ready to Assign has in Move Money's pickers.
const READY: &str = "";

fn move_form(lines: &[BudgetLine], from: &Option<String>, to: &Option<String>, f: &Figure, style: &NumberStyle) -> MoveForm {
    let mut choices = vec![BudgetChoice {
        key: READY.into(),
        title: "Ready to Assign".into(),
        glyph: "inbox".into(),
    }];
    choices.extend(lines.iter().filter(|l| l.assignable() && !l.hidden).map(|l| BudgetChoice {
        key: l.category_id.hex(),
        title: l.name.clone(),
        glyph: crate::look::category_glyph(&l.name).into(),
    }));
    let name = |k: &Option<String>| {
        k.as_deref()
            .and_then(|k| line(lines, k))
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "Ready to Assign".into())
    };
    let mut amount = amount_form(
        format!("{} to {}", name(from), name(to)),
        "Amount",
        "To move",
        f,
        format!("From {} to {}.", name(from), name(to)),
        style,
    );
    amount.can_save = amount.can_save && amount.cents > 0 && from != to;
    MoveForm {
        from: from.clone().unwrap_or_default(),
        to: to.clone().unwrap_or_default(),
        choices,
        amount,
    }
}

fn category_form(groups: &[GroupLine], group: &str, name: &str) -> CategoryForm {
    let cards = money_core::cards::card_group();
    let mut gs: Vec<&GroupLine> = groups.iter().filter(|g| g.id != cards).collect();
    gs.sort_by_key(|g| g.pos);
    let problem = match (name.is_empty(), id_of::<Group>(group)) {
        (true, _) | (_, None) => None,
        (false, Some(group_id)) => MoneyChange::AddCategory(AddCategory {
            group_id,
            name: name.trim().into(),
        })
        .problem(),
    };
    CategoryForm {
        name: name.into(),
        can_save: !name.trim().is_empty() && problem.is_none() && id_of::<Group>(group).is_some(),
        problem,
        group: group.into(),
        groups: gs
            .iter()
            .map(|g| BudgetChoice {
                key: g.id.hex(),
                title: g.name.clone(),
                glyph: "folder".into(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests;
