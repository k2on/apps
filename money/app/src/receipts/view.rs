//! A transaction's receipt laid out, once: its section in the transaction's
//! form, and the sheets it raises over it (the receipt form, the receipts
//! nobody matched, the photo whole). Drawn by every toolkit from these
//! functions or their recordings.

use ark_app::view;
use ark_app::view::*;

use super::{ReceiptForm, ReceiptSection};
use crate::sheets::SheetsMsg;

/// The receipt as the paper is, with a word when its reading did not add
/// up, its photo a tap away, and putting it right or taking it off; the
/// photos still being read; or none, and the ways to give it one.
#[view]
pub fn receipt_section(r: &ReceiptSection) -> View<SheetsMsg> {
    (
        form_section((
            (
                if r.none {
                    list_row("No Receipt Yet").glyph("receipt").quiet()
                },
                for p in r.reading {
                    list_row("Reading the Receipt\u{2026}")
                        .subtitle(p.note)
                        .glyph("refresh-cw")
                        .on_tap(SheetsMsg::ShowPhoto { hash: p.hash })
                },
                if r.check {
                    list_row("Check This").subtitle("It doesn't add up as read.").glyph("triangle-alert")
                },
                if r.has_paper {
                    paper(r.heading, r.date, r.names, r.figures, r.notes, r.total)
                },
                if r.has_photo {
                    list_row("Photo").glyph("image").on_tap(SheetsMsg::ShowPhoto { hash: r.photo })
                },
            ),
            if r.has_paper {
                (
                    list_row("Edit Receipt").glyph("pencil").on_tap(SheetsMsg::EditReceipt),
                    list_row("Take Receipt Off").glyph("circle-minus").on_tap(SheetsMsg::TakeReceiptOff),
                )
            },
            if r.can_add {
                (
                    list_row("Scan Receipt").glyph("scan-line").on_tap(SheetsMsg::ScanReceipt),
                    list_row("Choose Photo").glyph("image").on_tap(SheetsMsg::ChoosePhoto),
                    list_row("Type Receipt").glyph("keyboard").on_tap(SheetsMsg::TypeReceipt),
                )
            },
            if r.can_match {
                list_row(r.match_title).glyph("link").on_tap(SheetsMsg::MatchReceipt)
            },
            // What a scan or a pick said went wrong, until the next.
            if r.has_problem {
                list_row(r.problem).glyph("triangle-alert").quiet()
            },
        ))
        .header("Receipt"),
        if let Some(f) = r.form {
            if f.adding {
                form_sheet("New Receipt", SheetsMsg::CloseReceipt, receipt_fields(f)).confirm("Save", f.can_save, SheetsMsg::SaveReceipt)
            } else {
                form_sheet("Receipt", SheetsMsg::CloseReceipt, receipt_fields(f)).confirm("Save", f.can_save, SheetsMsg::SaveReceipt)
            }
        },
        if let Some(m) = r.matching {
            form_sheet(
                "Match a Receipt",
                SheetsMsg::CloseMatch,
                form_section(for x in m.receipts {
                    list_row(x.title)
                        .subtitle(x.subtitle)
                        .amount(x.cents)
                        .glyph("receipt")
                        .on_tap(SheetsMsg::Matched { id: x.id })
                })
                .footer("The receipts nothing pays for yet, the nearest to this transaction first."),
            )
        },
        if let Some(v) = r.viewing {
            form_sheet(
                "Receipt Photo",
                SheetsMsg::ClosePhoto,
                form_section(zoom_photo(v.url, "The receipt's photo")).footer(v.note),
            )
        },
    )
        .into()
}

/// The receipt form: where and when, each item with what it cost (a row to
/// tap to type it), the one being typed's fields, and the total.
#[view]
pub fn receipt_fields(f: &ReceiptForm) -> View<SheetsMsg> {
    (
        form_section((
            name_field("Store", f.place, None, |place| SheetsMsg::ReceiptPlace { place }),
            date_field("Date", f.date, |day| SheetsMsg::ReceiptDay { day }),
        )),
        form_section((
            for i in f.items {
                if i.editing {
                    list_row(i.title).value(i.figure).accent().on_tap(SheetsMsg::PickItem { key: i.key })
                } else {
                    list_row(i.title).value(i.figure).on_tap(SheetsMsg::PickItem { key: i.key })
                }
            },
            list_row("Add Item").glyph("plus").on_tap(SheetsMsg::AddItem),
        ))
        .header("Items")
        .footer("A discount is below zero, -0.50."),
        // The item being typed, apart from the list, so its fields keep the
        // keyboard as its row changes with them.
        if f.typing {
            form_section((
                name_field("Item", f.name, None, |name| SheetsMsg::ItemName { name }),
                amount_field("Price", f.amount, f.amount_problem, |typed| SheetsMsg::ItemAmount { typed }),
                list_row("Remove Item").glyph("trash").on_tap(SheetsMsg::RemoveItem),
            ))
            .header("Item")
        },
        form_section(list_row("Total").value(f.total)).footer(f.note),
    )
        .into()
}
