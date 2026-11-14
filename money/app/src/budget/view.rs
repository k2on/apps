//! The Budget tab laid out, once: drawn by iced and the browser from this
//! function, written out as SwiftUI and Compose from its recording
//! (`money_app::screens`, written while nix builds the native projects).

use ark_app::shell::*;
use ark_app::view;
use ark_app::view::*;

use super::{BudgetMsg, BudgetPage, CategoryItem, GroupItem, COLUMNS};

/// The tab: its month the large title; Ready to Assign in its card with
/// Auto-Assign; each group a section of its categories, its figures in
/// columns on a regular width, folded by its heading, held for its menu;
/// over it while they are up, Auto-Assign's question, the sheets (Assign,
/// Move Money, Target, New Category) and the alerts naming a group.
#[view]
pub fn budget(p: &BudgetPage) -> View<BudgetMsg> {
    tab_page(
        p.title,
        (
            headline(p.ready_label, p.ready, p.ready_tint, p.ready_caption).action("Auto-Assign", "wand-sparkles", BudgetMsg::AutoAssign),
            for g in p.groups {
                group(g)
            },
            if p.auto {
                confirmation_dialog("Auto-Assign", "How should this month's money be given out?", BudgetMsg::CancelAuto)
                    .action("Underfunded", false, BudgetMsg::AutoUnderfunded)
                    .action("Assigned Last Month", false, BudgetMsg::AutoLastMonth)
                    .action("Reset Assigned Amounts", true, BudgetMsg::AutoReset)
            },
            (
                if let Some(f) = p.assigning {
                    form_sheet(
                        "Assign",
                        BudgetMsg::Dismiss,
                        form_section(amount_entry(
                            f.label,
                            f.prompt,
                            f.typed,
                            f.cents,
                            f.problem,
                            |typed| BudgetMsg::Typed { typed },
                            |cents| BudgetMsg::Cents { cents },
                        ))
                        .header(f.subject)
                        .footer(f.note),
                    )
                    .confirm("Assign", f.can_save, BudgetMsg::Save)
                },
                if let Some(m) = p.moving {
                    form_sheet(
                        "Move Money",
                        BudgetMsg::Dismiss,
                        (
                            form_section((
                                search_picker(
                                    "From",
                                    m.from,
                                    |key| BudgetMsg::MoveFrom { key },
                                    for c in m.choices {
                                        search_choice(c.title, c.glyph, c.key).group(c.group).detail(c.detail, c.tint)
                                    },
                                )
                                .prompt("Search Categories"),
                                search_picker(
                                    "To",
                                    m.to,
                                    |key| BudgetMsg::MoveTo { key },
                                    for c in m.choices {
                                        search_choice(c.title, c.glyph, c.key).group(c.group).detail(c.detail, c.tint)
                                    },
                                )
                                .prompt("Search Categories"),
                            )),
                            form_section(amount_entry(
                                m.amount.label,
                                m.amount.prompt,
                                m.amount.typed,
                                m.amount.cents,
                                m.amount.problem,
                                |typed| BudgetMsg::Typed { typed },
                                |cents| BudgetMsg::Cents { cents },
                            ))
                            .footer(m.amount.note),
                        ),
                    )
                    .confirm("Move", m.amount.can_save, BudgetMsg::Save)
                },
                if let Some(f) = p.targeting {
                    form_sheet(
                        "Target",
                        BudgetMsg::Dismiss,
                        form_section(amount_entry(
                            f.label,
                            f.prompt,
                            f.typed,
                            f.cents,
                            f.problem,
                            |typed| BudgetMsg::Typed { typed },
                            |cents| BudgetMsg::Cents { cents },
                        ))
                        .header(f.subject)
                        .footer(f.note),
                    )
                    .confirm("Save", f.can_save, BudgetMsg::Save)
                },
                if let Some(f) = p.adding {
                    form_sheet(
                        "New Category",
                        BudgetMsg::Dismiss,
                        form_section((
                            name_field("Name", f.name, f.problem, |name| BudgetMsg::CategoryName { name }),
                            option_picker(
                                "Group",
                                f.group,
                                |key| BudgetMsg::CategoryGroup { key },
                                for g in f.groups {
                                    picker_choice(g.title, g.glyph, g.key)
                                },
                            ),
                        )),
                    )
                    .confirm("Add", f.can_save, BudgetMsg::Save)
                },
                if let Some(n) = p.new_group {
                    prompt(
                        "New Group",
                        n.name,
                        |name| BudgetMsg::Named { name },
                        "Add",
                        BudgetMsg::SaveName,
                        BudgetMsg::CancelName,
                    )
                },
                if let Some(n) = p.renaming {
                    prompt(
                        "Rename Group",
                        n.name,
                        |name| BudgetMsg::Named { name },
                        "Rename",
                        BudgetMsg::SaveName,
                        BudgetMsg::CancelName,
                    )
                },
            ),
        ),
    )
    .refresh(BudgetMsg::Refresh)
    .into()
}

/// A group: its heading (what its categories have, or what of the cards'
/// debt is not covered; on a regular width what they were assigned and
/// spent too), folded by a tap, held for New Category, Rename, Hide, Move
/// Up or Down and Delete Group; its categories, or Add a Category while it
/// has none.
#[view]
pub fn group(g: &GroupItem) -> View<BudgetMsg> {
    card_section((
        for l in g.lines {
            category(l)
        },
        if g.empty {
            list_row("Add a Category")
                .glyph("plus")
                .accent()
                .on_tap(BudgetMsg::NewCategory { group: g.id })
        },
    ))
    .header(g.title)
    .amount(g.available)
    .total_note(g.uncovered)
    .columns(COLUMNS, g.assigned, g.activity)
    .collapsible(g.open, BudgetMsg::Toggle { id: g.id })
    .menu_if("New Category", "circle-plus", BudgetMsg::NewCategory { group: g.id }, false, g.own)
    .menu_if("Rename", "pencil", BudgetMsg::RenameGroup { id: g.id }, false, g.own)
    .menu_if(g.hide_title, g.hide_glyph, BudgetMsg::HideGroup { id: g.id }, false, true)
    .menu_if("Move Up", "arrow-up", BudgetMsg::GroupUp { id: g.id }, false, g.can_up)
    .menu_if("Move Down", "arrow-down", BudgetMsg::GroupDown { id: g.id }, false, g.can_down)
    .menu_if("Delete Group", "trash", BudgetMsg::DeleteGroup { id: g.id }, true, g.own)
    .into()
}

/// A category in the month: its emoji and name (a card's payment its
/// card's mark), the bar toward its target and what needs attention, on a
/// regular width what it was assigned and spent, what it has in a badge of
/// its state. Tapped, its month; held, what can be done to it.
#[view]
pub fn category(l: &CategoryItem) -> View<BudgetMsg> {
    meter_row(l.title, l.status, l.status_tint)
        .mark(l.mark, l.mark_slot)
        .faded(l.faded)
        .meter(l.fraction, l.meter_tint)
        .note(l.note, l.note_tint)
        .note_end(l.note_end)
        .columns(l.assigned, l.activity)
        .said(l.said)
        .on_tap(BudgetMsg::Open { id: l.id })
        .menu_if(l.rule, "wand-sparkles", BudgetMsg::RuleSaid, false, l.ruled)
        .menu_if(l.cover, "credit-card", BudgetMsg::CoverCard { id: l.id }, false, l.can_cover)
        .menu_if("Assign…", "circle-plus", BudgetMsg::Assign { id: l.id }, false, l.assignable)
        .menu_if(l.meet, "target", BudgetMsg::MeetTarget { id: l.id }, false, l.can_meet)
        .menu_if(l.move_title, "arrow-left-right", BudgetMsg::Move { id: l.id }, false, l.assignable)
        .menu_if(l.target_title, "crosshair", BudgetMsg::Target { id: l.id }, false, l.own)
        .menu_if(l.hide_title, l.hide_glyph, BudgetMsg::HideCategory { id: l.id }, false, true)
        .menu_if("Move Up", "arrow-up", BudgetMsg::CategoryUp { id: l.id }, false, l.can_up)
        .menu_if("Move Down", "arrow-down", BudgetMsg::CategoryDown { id: l.id }, false, l.can_down)
        .menu_if("Delete Category", "trash", BudgetMsg::DeleteCategory { id: l.id }, true, l.own)
        .into()
}
