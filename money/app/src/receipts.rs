//! A transaction's receipt, in its form (the sheets' [`crate::sheets`]):
//! where it stands — read and drawn as the paper it was printed on (a word
//! when its reading did not add up), its photo a tap away, put right or
//! taken off; still being read; or none — and the ways to give it one: a
//! paper receipt scanned (the scanner module, `ark_app::modules::scanner`:
//! the page found, held still and made flat), a photo chosen from the
//! library (the scanner's `PickPhoto`), one typed, or one nobody has
//! matched attached. A photo is kept — put on the device and sent to the
//! server on this connection or the next (`MoneyRequest::KeepPhoto`),
//! which reads it (money/server's `read_receipt`) and files what it read
//! under the transaction. What the server read comes back as the domain's
//! rows (`MoneyView::Paid`, `Items`, `Photos`, `Receipts`).
//!
//! [`State`] is what giving one is in the middle of; [`Draft`] the
//! receipt form's rules (the desktop's: an amount typed as the paper
//! prints it, a purchase positive, a discount below zero); [`section`] the
//! record the transaction's form draws ([`ReceiptSection`]).

use ark::file::FileRef;
use ark_app::modules::haptics::{perform, Haptic};
use ark_app::modules::scanner::{pick_photo, scan_page};
use ark_app::{Answer, Command, Data};
use ark_format::{amount_editable, parse_amount, NumberStyle};
use money_core::*;

use crate::sheets::{Sheets, SheetsMsg};
use crate::MoneyRequest;

pub mod view;

#[cfg(test)]
mod tests;

/// The media a scanned or chosen photo is kept as.
pub const JPEG: &str = "image/jpeg";

/// What giving a transaction its receipt is in the middle of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The scanner's page or the photo picker is up.
    pub asking: bool,
    /// What went wrong with the last, said until the next.
    pub problem: Option<String>,
    /// The receipt form, over the transaction's.
    pub typing: Option<Draft>,
    /// The receipts nobody matched, offered.
    pub matching: bool,
    /// A photo shown whole.
    pub photo: Option<FileRef>,
}

/// One line of the receipt form: a name and what was paid for it, as typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub name: String,
    /// Dollars and cents as typed, `4.29`; a discount below zero, `-0.50`.
    pub amount: String,
    pub sku: String,
    pub quantity: i64,
    pub unit: String,
}

impl Item {
    fn blank() -> Item {
        Item {
            quantity: 1000,
            unit: "ea".into(),
            ..Item::default()
        }
    }

    fn is_blank(&self) -> bool {
        self.name.trim().is_empty() && self.amount.trim().is_empty()
    }
}

/// A receipt typed by hand, or one read put right.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    /// The receipt put right; blank for a new one.
    pub receipt_id: String,
    pub txn: Option<Id<Txn>>,
    pub place: String,
    pub date: String,
    pub items: Vec<Item>,
    /// The item being typed.
    pub editing: Option<usize>,
    /// The domain's sentence, when it refused what was saved.
    pub refused: Option<String>,
}

impl Draft {
    /// A new one for `txn`, from its payee and day: one item, being typed.
    pub fn new(txn: Option<Id<Txn>>, place: &str, date: &str) -> Draft {
        Draft {
            txn,
            place: place.into(),
            date: date.into(),
            items: vec![Item::blank()],
            editing: Some(0),
            ..Draft::default()
        }
    }

    /// The form over a receipt already there: its items, a purchase
    /// positive as people read a receipt.
    pub fn of(r: &ReceiptLine, items: &[ItemLine], txn: Option<Id<Txn>>, style: &NumberStyle) -> Draft {
        let mut items: Vec<Item> = items
            .iter()
            .map(|i| Item {
                name: i.name.clone(),
                amount: amount_editable(-i.amount, style),
                sku: i.sku.clone(),
                quantity: i.quantity,
                unit: i.unit.clone(),
            })
            .collect();
        if items.is_empty() {
            items.push(Item::blank());
        }
        Draft {
            receipt_id: r.id.clone(),
            txn: txn.or(r.txn_id),
            place: r.place.clone(),
            date: r.date.clone(),
            items,
            ..Draft::default()
        }
    }

    /// What was typed as cents (`4.29`, `$4.29`, `-0.5`); none when it is
    /// not an amount, or blank.
    pub fn cents(typed: &str, style: &NumberStyle) -> Option<i64> {
        match typed.trim().is_empty() {
            true => None,
            false => parse_amount(typed, style),
        }
    }

    /// The lines with something in them.
    pub fn filled(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|i| !i.is_blank())
    }

    /// What the items come to, purchases positive.
    pub fn total(&self, style: &NumberStyle) -> i64 {
        self.filled().filter_map(|i| Draft::cents(&i.amount, style)).sum()
    }

    /// What keeps it from being saved, said; none when it can be.
    pub fn problem(&self, style: &NumberStyle) -> Option<String> {
        let filled: Vec<&Item> = self.filled().collect();
        if filled.is_empty() {
            return Some("Add an item.".into());
        }
        if filled.iter().any(|i| i.name.trim().is_empty()) {
            return Some("Every item needs a name.".into());
        }
        if filled.iter().any(|i| Draft::cents(&i.amount, style).is_none()) {
            return Some("An amount isn't one.".into());
        }
        if !money_core::dates::is_day(&self.date) {
            return Some("The day isn't one.".into());
        }
        None
    }

    /// What the domain is given: amounts our sign, spent negative; the
    /// total the items'.
    pub fn write(&self, style: &NumberStyle) -> WriteReceipt {
        WriteReceipt {
            receipt_id: self.receipt_id.clone(),
            txn_id: self.txn,
            place: self.place.trim().into(),
            date: self.date.clone(),
            total: None,
            items: self
                .filled()
                .map(|i| StoreItem {
                    name: i.name.trim().into(),
                    sku: i.sku.clone(),
                    quantity: i.quantity,
                    unit: i.unit.clone(),
                    amount: -Draft::cents(&i.amount, style).unwrap_or(0),
                })
                .collect(),
        }
    }

    fn item(&mut self) -> Option<&mut Item> {
        self.editing.and_then(|i| self.items.get_mut(i))
    }
}

/// The receipt that paid for `txn`, the first when several did.
fn paid<'a>(data: &Data<'a, MoneyView>, txn: Id<Txn>) -> Option<&'a ReceiptLine> {
    data.rows::<ReceiptLine>(&MoneyView::Paid { txn }).first()
}

fn photos<'a>(data: &Data<'a, MoneyView>) -> &'a [PhotoLine] {
    data.rows(&MoneyView::Photos)
}

/// The views the receipt section reads while the form is up for `txn`,
/// `receipt` the one that paid for it.
pub fn holds(s: &State, txn: Option<Id<Txn>>, receipt: Option<&str>) -> Vec<MoneyView> {
    let Some(txn) = txn else { return vec![] };
    let mut v = vec![MoneyView::Paid { txn }, MoneyView::Photos, MoneyView::Receipts];
    if let Some(r) = receipt.filter(|r| !r.is_empty()) {
        v.push(MoneyView::Items { receipt: r.into() });
    }
    if let Some(d) = s
        .typing
        .as_ref()
        .filter(|d| !d.receipt_id.is_empty() && Some(d.receipt_id.as_str()) != receipt)
    {
        v.push(MoneyView::Items {
            receipt: d.receipt_id.clone(),
        });
    }
    v
}

/// The receipt that paid for `txn`, by its id: what [`holds`] reads its items of.
pub fn paid_id(data: &Data<'_, MoneyView>, txn: Option<Id<Txn>>) -> Option<String> {
    txn.and_then(|t| paid(data, t)).map(|r| r.id.clone())
}

fn photographed(a: Answer<Option<Vec<u8>>>) -> SheetsMsg {
    match a {
        Ok(page) => SheetsMsg::Photographed { page, failed: None },
        Err(e) => SheetsMsg::Photographed {
            page: None,
            failed: Some(e.to_string()),
        },
    }
}

fn written(r: Result<(), String>) -> SheetsMsg {
    SheetsMsg::ReceiptDone { refused: r.err() }
}

/// The transaction a receipt is for: what one typed starts from (its payee
/// and day) and what one matched is nearest to (its amount, our sign).
pub struct About<'a> {
    pub txn: Option<Id<Txn>>,
    pub amount: i64,
    pub payee: &'a str,
    pub day: &'a str,
}

/// A receipt message taken into `s`, for the transaction `about`.
pub fn update(s: &mut State, msg: SheetsMsg, about: About<'_>, data: &Data<'_, MoneyView>, style: &NumberStyle) -> Vec<Command<Sheets>> {
    use SheetsMsg as M;
    let txn = about.txn;
    match msg {
        M::ScanReceipt | M::ChoosePhoto if !s.asking && txn.is_some() => {
            s.asking = true;
            s.problem = None;
            match msg {
                M::ScanReceipt => vec![Command::ask_then(scan_page(), photographed)],
                _ => vec![Command::ask_then(pick_photo(), photographed)],
            }
        }
        M::Photographed { page, failed } => {
            s.asking = false;
            match (page, failed) {
                (Some(jpeg), _) => vec![
                    Command::Request(MoneyRequest::KeepPhoto {
                        jpeg,
                        txn: txn.map(|t| t.hex()),
                    }),
                    Command::ask(perform(Haptic::Success)),
                ],
                (None, Some(why)) => {
                    s.problem = Some(crate::words::scan_failed(&why));
                    vec![]
                }
                (None, None) => vec![],
            }
        }
        M::TypeReceipt => {
            s.problem = None;
            s.typing = Some(Draft::new(txn, about.payee, about.day));
            vec![]
        }
        M::EditReceipt => {
            s.problem = None;
            if let Some(r) = txn.and_then(|t| paid(data, t)) {
                let items: &[ItemLine] = data.rows(&MoneyView::Items { receipt: r.id.clone() });
                s.typing = Some(Draft::of(r, items, txn, style));
            }
            vec![]
        }
        M::ReceiptPlace { place } => typed(s, |d| d.place = place),
        M::ReceiptDay { day } => typed(s, |d| d.date = day),
        M::PickItem { key } => typed(s, |d| d.editing = key.parse().ok().filter(|i| *i < d.items.len())),
        M::ItemName { name } => typed(s, |d| {
            if let Some(i) = d.item() {
                i.name = name
            }
        }),
        M::ItemAmount { typed: amount } => typed(s, |d| {
            if let Some(i) = d.item() {
                i.amount = amount
            }
        }),
        M::AddItem => typed(s, |d| {
            d.items.push(Item::blank());
            d.editing = Some(d.items.len() - 1);
        }),
        M::RemoveItem => typed(s, |d| {
            if let Some(i) = d.editing.take().filter(|i| *i < d.items.len()) {
                d.items.remove(i);
            }
            if d.items.is_empty() {
                d.items.push(Item::blank());
                d.editing = Some(0);
            }
        }),
        M::SaveReceipt => match &s.typing {
            Some(d) if d.problem(style).is_none() => vec![Command::Send {
                change: MoneyChange::WriteReceipt(d.write(style)),
                then: Some(written),
            }],
            _ => vec![],
        },
        M::CloseReceipt => {
            s.typing = None;
            vec![]
        }
        M::MatchReceipt => {
            s.problem = None;
            s.matching = true;
            vec![]
        }
        M::CloseMatch => {
            s.matching = false;
            vec![]
        }
        M::Matched { id } => {
            s.matching = false;
            match txn {
                Some(t) => vec![Command::Send {
                    change: MoneyChange::Pay(MatchReceipt {
                        receipt_id: id,
                        txn_id: Some(t),
                    }),
                    then: Some(written),
                }],
                None => vec![],
            }
        }
        M::TakeReceiptOff => match txn.and_then(|t| paid(data, t)) {
            Some(r) => vec![Command::Send {
                change: MoneyChange::Pay(MatchReceipt {
                    receipt_id: r.id.clone(),
                    txn_id: None,
                }),
                then: Some(written),
            }],
            None => vec![],
        },
        M::ShowPhoto { hash } => match photos(data).iter().find(|p| p.file.hash == hash) {
            Some(p) => {
                s.photo = Some(p.file.clone());
                vec![Command::Want(p.file.clone())]
            }
            None => vec![],
        },
        M::ClosePhoto => {
            s.photo = None;
            vec![]
        }
        M::ReceiptDone { refused } => match refused {
            None => {
                s.typing = None;
                vec![Command::ask(perform(Haptic::Success))]
            }
            Some(why) => {
                match &mut s.typing {
                    Some(d) => d.refused = Some(why),
                    None => s.problem = Some(why),
                }
                vec![]
            }
        },
        _ => vec![],
    }
}

fn typed(s: &mut State, f: impl FnOnce(&mut Draft)) -> Vec<Command<Sheets>> {
    if let Some(d) = &mut s.typing {
        f(d);
        d.refused = None;
    }
    vec![]
}

/// A photo taken for the transaction and not read into a receipt yet.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ReadingPhoto {
    pub hash: String,
    /// "Your server reads it once the photo reaches it.", or why it has not yet.
    pub note: String,
}

/// One line of the receipt form.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct FormItem {
    /// Its place in the form, as [`SheetsMsg::PickItem`] names it.
    pub key: String,
    /// "Item" while it has no name.
    pub title: String,
    pub figure: String,
    /// It is being typed: lit, its fields under the list.
    pub editing: bool,
}

/// The receipt form as it shows.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ReceiptForm {
    /// A new one: "New Receipt"; else "Receipt".
    pub adding: bool,
    pub place: String,
    pub date: String,
    pub items: Vec<FormItem>,
    /// An item is being typed: its name and amount as typed.
    pub typing: bool,
    pub name: String,
    pub amount: String,
    pub amount_problem: Option<String>,
    /// What the items come to.
    pub total: String,
    /// Why it can't be saved yet, or the domain's refusal; empty for none.
    pub note: String,
    pub can_save: bool,
}

/// One receipt nobody has matched, offered.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct MatchRow {
    pub id: String,
    /// Who it is from.
    pub title: String,
    /// "Oct 24, 2026 · 6 items".
    pub subtitle: String,
    /// Our sign.
    pub cents: i64,
}

/// The receipts nobody has matched, the nearest first.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct MatchSheet {
    pub receipts: Vec<MatchRow>,
}

/// A photo shown whole.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct PhotoSheet {
    /// Where it is read (`Views::file_url`); empty while it is fetched.
    pub url: String,
    /// "Pinch to zoom.", or while it is fetched what it waits on.
    pub note: String,
}

/// A transaction's receipt section, as its form draws it.
#[derive(Clone, Debug, PartialEq, ark::twin::Twin)]
pub struct ReceiptSection {
    /// No receipt, no photo of one.
    pub none: bool,
    pub reading: Vec<ReadingPhoto>,
    /// A receipt: drawn as its paper.
    pub has_paper: bool,
    /// Its reading did not add up.
    pub check: bool,
    pub heading: String,
    pub date: String,
    pub names: Vec<String>,
    pub figures: Vec<String>,
    pub notes: Vec<String>,
    pub total: String,
    /// The photo it was read from, by its file's hash.
    pub has_photo: bool,
    pub photo: String,
    /// The ways to give it one: there is no receipt yet.
    pub can_add: bool,
    /// "Match a Receipt (3)", while there are any.
    pub can_match: bool,
    pub match_title: String,
    /// What a scan or a pick said went wrong.
    pub has_problem: bool,
    pub problem: String,
    pub form: Option<ReceiptForm>,
    pub matching: Option<MatchSheet>,
    pub viewing: Option<PhotoSheet>,
}

/// Who a receipt is from, as its paper heads it.
pub fn heading(r: &ReceiptLine) -> String {
    match r.place.is_empty() {
        true => r.store_name().to_uppercase(),
        false => r.place.to_uppercase(),
    }
}

/// Money as a receipt prints it: no sign for what was paid, a minus for
/// what was taken off.
fn printed(our_sign: i64, style: &NumberStyle) -> String {
    amount_editable(-our_sign, style)
}

/// The section for the transaction `about`, as `s` and the views have it.
pub fn section(s: &State, about: About<'_>, data: &Data<'_, MoneyView>, style: &NumberStyle) -> ReceiptSection {
    let receipt = about.txn.and_then(|t| paid(data, t));
    let photos = photos(data);
    let reading: Vec<ReadingPhoto> = match receipt {
        Some(_) => vec![],
        None => photos
            .iter()
            .filter(|p| p.txn_id.is_some() && p.txn_id == about.txn && p.receipt_id.is_empty())
            .map(|p| ReadingPhoto {
                hash: p.file.hash.clone(),
                note: match p.state.as_str() {
                    "failed" if !p.why.is_empty() => format!("Not read yet: {}", p.why),
                    _ => "Your server reads it once the photo reaches it.".into(),
                },
            })
            .collect(),
    };
    let photo = receipt.and_then(|r| photos.iter().find(|p| p.receipt_id == r.id));
    let items: &[ItemLine] = receipt.map_or(&[], |r| data.rows(&MoneyView::Items { receipt: r.id.clone() }));
    let unmatched = words::unmatched_for(data.rows(&MoneyView::Receipts), about.amount, about.day);
    let mut names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
    let mut figures: Vec<String> = items.iter().map(|i| printed(i.amount, style)).collect();
    let mut notes: Vec<String> = items
        .iter()
        .map(|i| match i.quantity {
            q if q == 1000 || q <= 0 => String::new(),
            q => crate::words::quantity(q, &i.unit),
        })
        .collect();
    // Its items not here yet: a line that says so.
    if items.is_empty() && receipt.is_some_and(|r| r.items > 0) {
        names.push("\u{2026}".into());
        figures.push(String::new());
        notes.push(String::new());
    }
    ReceiptSection {
        none: receipt.is_none() && reading.is_empty(),
        has_paper: receipt.is_some(),
        check: photo.is_some_and(|p| p.state == "check"),
        heading: receipt.map(heading).unwrap_or_default(),
        date: receipt.map(|r| ark_format::day_long(&r.date)).unwrap_or_default(),
        names,
        figures,
        notes,
        total: receipt.map(|r| printed(r.total, style)).unwrap_or_default(),
        has_photo: photo.is_some(),
        photo: photo.map(|p| p.file.hash.clone()).unwrap_or_default(),
        can_add: receipt.is_none(),
        can_match: receipt.is_none() && !unmatched.is_empty(),
        match_title: format!("Match a Receipt ({})", unmatched.len()),
        reading,
        has_problem: s.problem.is_some(),
        problem: s.problem.clone().unwrap_or_default(),
        form: s.typing.as_ref().map(|d| form(d, style)),
        matching: s.matching.then(|| MatchSheet {
            receipts: unmatched
                .iter()
                .map(|r| MatchRow {
                    id: r.id.clone(),
                    title: heading(r),
                    subtitle: match r.items {
                        1 => format!("{} \u{00b7} 1 item", ark_format::day_long(&r.date)),
                        n => format!("{} \u{00b7} {n} items", ark_format::day_long(&r.date)),
                    },
                    cents: r.total,
                })
                .collect(),
        }),
        viewing: s.photo.as_ref().map(|f| {
            let url = data.store.file_url(f).unwrap_or_default();
            PhotoSheet {
                note: match url.is_empty() {
                    true => "Fetching the photo from your server\u{2026}".into(),
                    false => "Pinch to zoom; double-tap to zoom in or out.".into(),
                },
                url,
            }
        }),
    }
}

fn form(d: &Draft, style: &NumberStyle) -> ReceiptForm {
    let problem = d.problem(style);
    let editing = d.editing.and_then(|i| d.items.get(i));
    ReceiptForm {
        adding: d.receipt_id.is_empty(),
        place: d.place.clone(),
        date: d.date.clone(),
        items: d
            .items
            .iter()
            .enumerate()
            .map(|(n, i)| FormItem {
                key: n.to_string(),
                title: match i.name.trim().is_empty() {
                    true => "Item".into(),
                    false => i.name.clone(),
                },
                figure: match Draft::cents(&i.amount, style) {
                    Some(c) => amount_editable(c, style),
                    None => i.amount.clone(),
                },
                editing: d.editing == Some(n),
            })
            .collect(),
        typing: editing.is_some(),
        name: editing.map(|i| i.name.clone()).unwrap_or_default(),
        amount: editing.map(|i| i.amount.clone()).unwrap_or_default(),
        amount_problem: editing
            .filter(|i| !i.amount.trim().is_empty() && Draft::cents(&i.amount, style).is_none())
            .map(|_| "That isn't an amount.".into()),
        total: amount_editable(d.total(style), style),
        note: d.refused.clone().or(problem.clone()).unwrap_or_default(),
        can_save: problem.is_none(),
    }
}
