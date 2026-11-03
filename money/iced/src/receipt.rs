//! A transaction's receipt, in the form it is corrected in: there is no
//! receipts page. A receipt found for a transaction (a store's, matched; a
//! photo's, read by the server) is drawn as the paper it is — monospace,
//! black on white — its items each filed from where they stand; one
//! photographed is "Reading…" until the server has read it, and "Check
//! this" when what it read does not add up.
//!
//! A receipt is added to a transaction three ways: a photo dropped on the
//! window or the page, or chosen (`arkui::files`; kept on this device and
//! sent to the server, which reads it, `docs/plan-files.md`), one typed by
//! hand, or one of the receipts that
//! paid for nothing yet, attached. One already there is edited by hand, or
//! detached.

use arkui::style;
use iced::widget::{button, column, container, image, mouse_area, row, text, text_input, Column, Space};
use iced::{Alignment, Color, Element, Font, Length, Task};
use money_domain::*;

use crate::amount::{self, money};
use crate::view::{action, bold, dim, fill, link, quiet};
use crate::{look, App, Message, Modal};

/// What the receipt panel does.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A photo dropped on the window or chosen: the open transaction's
    /// receipt.
    Given(arkui::files::Picked),
    /// The chooser opened for one.
    Choose,
    /// A receipt that paid for nothing, said to have paid for this one.
    Attach(String),
    /// This one's receipt said to have paid for nothing.
    Detach(String),
    /// Typing one: a new one (none), or one already there put right.
    Write(Option<String>),
    Place(String),
    Date(String),
    Name(usize, String),
    Amount(usize, String),
    AddItem,
    RemoveItem(usize),
    Save,
    Cancel,
    /// A receipt's photo, shown over the window.
    Photo(ark::file::FileRef),
}

/// A receipt being typed: its place, day and items, each a name and what
/// it cost, as typed (spent, unsigned).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Paper {
    pub receipt_id: String,
    pub place: String,
    pub date: String,
    pub items: Vec<(String, String)>,
}

/// What a receipt's photo may be, for the chooser.
const PHOTOS: &[&str] = &["image/jpeg", "image/png", "image/heic", "image/webp"];

/// The receipts that paid for nothing yet, nearest this transaction in
/// amount and then in days first.
pub fn unmatched<'a>(receipts: &'a [ReceiptLine], t: &TxnLine) -> Vec<&'a ReceiptLine> {
    let mut free: Vec<&ReceiptLine> = receipts.iter().filter(|r| r.txn_id.is_none() && r.charges == 0).collect();
    free.sort_by_key(|r| ((r.total - t.amount).abs(), money_domain::dates::days_between(&r.date, &t.date).abs()));
    free.truncate(5);
    free
}

impl App {
    /// The transaction open in the form, when it is one already entered.
    fn open_txn(&self) -> Option<&TxnLine> {
        let id = self.form.as_ref()?.editing?;
        self.peer.register.rows.iter().find(|t| t.id == id)
    }

    pub fn receipt(&mut self, m: Msg) -> Task<Message> {
        let txn = self.open_txn().map(|t| t.id);
        match m {
            Msg::Choose => {
                return arkui::files::choose("A photo of the receipt", PHOTOS).map(|p| match p {
                    Some(p) => Message::Receipt(Msg::Given(p)),
                    None => Message::Swallow,
                });
            }
            Msg::Given(p) => {
                let Some(txn_id) = txn else {
                    self.note = "open a transaction, then drop its receipt's photo on it".into();
                    return Task::none();
                };
                let Some(media) = p.media().filter(|m| m.starts_with("image/")) else {
                    self.note = format!("{} is not a photo", p.name);
                    return Task::none();
                };
                match self.peer.client.put_file(&p.bytes, &media) {
                    Ok(file) => {
                        self.author("add the photo", "add_photo", &AddPhoto { file, txn_id: Some(txn_id) });
                    }
                    Err(e) => self.note = format!("could not keep the photo: {e}"),
                }
            }
            Msg::Attach(receipt_id) => {
                self.author("attach the receipt", "pay", &MatchReceipt { receipt_id, txn_id: txn });
            }
            Msg::Detach(receipt_id) => {
                self.author("detach the receipt", "pay", &MatchReceipt { receipt_id, txn_id: None });
            }
            Msg::Write(id) => {
                let t = self.open_txn().cloned().unwrap_or_default();
                let paper = match id.as_deref().and_then(|id| self.peer.txn_receipts().iter().find(|r| r.id == id).cloned()) {
                    Some(r) => Paper {
                        receipt_id: r.id.clone(),
                        place: r.place.clone(),
                        date: r.date.clone(),
                        items: self
                            .peer
                            .receipt_items()
                            .iter()
                            .map(|i| (i.name.clone(), amount::editable(-i.amount)))
                            .collect(),
                    },
                    None => Paper {
                        place: t.payee.clone(),
                        date: t.date.clone(),
                        items: vec![(String::new(), amount::editable(-t.amount))],
                        ..Paper::default()
                    },
                };
                self.paper = Some(paper);
            }
            Msg::Place(v) => self.paper.iter_mut().for_each(|p| p.place = v.clone()),
            Msg::Date(v) => self.paper.iter_mut().for_each(|p| p.date = v.clone()),
            Msg::Name(i, v) => {
                if let Some(it) = self.paper.as_mut().and_then(|p| p.items.get_mut(i)) {
                    it.0 = v;
                }
            }
            Msg::Amount(i, v) => {
                if let Some(it) = self.paper.as_mut().and_then(|p| p.items.get_mut(i)) {
                    it.1 = v;
                }
            }
            Msg::AddItem => self.paper.iter_mut().for_each(|p| p.items.push(Default::default())),
            Msg::RemoveItem(i) => {
                if let Some(p) = self.paper.as_mut().filter(|p| p.items.len() > 1) {
                    p.items.remove(i);
                }
            }
            Msg::Cancel => self.paper = None,
            Msg::Save => {
                let Some(p) = self.paper.clone() else { return Task::none() };
                let mut items = vec![];
                for (name, cost) in p.items.iter().filter(|(n, c)| !n.trim().is_empty() || !c.trim().is_empty()) {
                    let Some(cents) = amount::parse(cost) else {
                        self.note = format!("{cost:?} is not an amount");
                        return Task::none();
                    };
                    items.push(StoreItem {
                        name: name.trim().into(),
                        sku: String::new(),
                        quantity: 1000,
                        unit: "ea".into(),
                        amount: -cents,
                    });
                }
                let write = WriteReceipt {
                    receipt_id: p.receipt_id,
                    txn_id: txn,
                    place: p.place,
                    date: p.date,
                    total: None,
                    items,
                };
                if self.author("save the receipt", "write_receipt", &write) {
                    self.paper = None;
                }
            }
            Msg::Photo(file) => {
                self.photo = None;
                self.modal = Some(Modal::Photo(file));
                self.load_photo();
            }
        }
        Task::none()
    }

    /// The photo the modal shows, once its bytes are here: fetched from the
    /// server the first time, and decoded once.
    pub fn load_photo(&mut self) {
        let Some(Modal::Photo(f)) = &self.modal else { return };
        if self.photo.as_ref().is_some_and(|(h, _)| *h == f.hash) {
            return;
        }
        let f = f.clone();
        if let Some(bytes) = self.peer.client.open_file(&f) {
            self.photo = Some((f.hash, image::Handle::from_bytes(bytes)));
        }
    }

    /// The receipt under a transaction's form: drawn as paper, or how one
    /// is added.
    pub fn view_receipt_panel<'a>(&'a self, t: &'a TxnLine) -> Element<'a, Message> {
        if let Some(p) = &self.paper {
            return self.view_paper_form(p);
        }
        let photos: Vec<&PhotoLine> = self.peer.photos.rows.iter().filter(|p| p.txn_id == Some(t.id)).collect();
        let receipts = self.peer.txn_receipts();
        let mut side = column![].spacing(8);
        let mut panel = row![].spacing(20);
        match receipts.first() {
            Some(r) => {
                let photo = self.peer.photos.rows.iter().find(|p| p.receipt_id == r.id);
                panel = panel.push(self.view_paper(r, photo));
                if photo.is_some_and(|p| p.state == "check") {
                    side = side.push(
                        text("Check this: what was read does not add up to the total.")
                            .size(12)
                            .style(look::Tone::Short.text()),
                    );
                }
                if let Some(p) = photo {
                    side = side.push(link("Show the photo", Message::Receipt(Msg::Photo(p.file.clone()))));
                }
                side = side.push(link("Edit the receipt", Message::Receipt(Msg::Write(Some(r.id.clone())))));
                match &self.peer.charges {
                    // Paid shipment by shipment: each charge and what paid it.
                    Some(c) if r.charges > 0 => side = side.push(self.view_charges(&c.rows)),
                    _ => side = side.push(link("Detach it", Message::Receipt(Msg::Detach(r.id.clone())))),
                }
            }
            None => {
                for p in photos.iter().filter(|p| p.receipt_id.is_empty()) {
                    let said = match p.state.as_str() {
                        "failed" => format!("The photo could not be read: {}", p.why),
                        _ => "Reading the photo\u{2026}".into(),
                    };
                    side = side.push(row![dim(said, 12), link("Show", Message::Receipt(Msg::Photo(p.file.clone())))].spacing(10));
                }
                side = side.push(dim("Drop a photo of its receipt here, or", 12));
                side = side.push(
                    row![
                        link("Choose a photo\u{2026}", Message::Receipt(Msg::Choose)),
                        link("Type the receipt", Message::Receipt(Msg::Write(None))),
                    ]
                    .spacing(16),
                );
                let free = unmatched(&self.peer.receipts.rows, t);
                if !free.is_empty() {
                    side = side.push(dim("Or attach one that paid for nothing yet:", 12));
                    for r in free {
                        let what = format!("{} \u{b7} {} \u{b7} {}", r.date, place_of(r), money(-r.total));
                        side = side.push(row![text(what).size(12), link("Attach", Message::Receipt(Msg::Attach(r.id.clone())))].spacing(10));
                    }
                }
            }
        }
        container(panel.push(side.width(Length::Fill))).padding([10, 24]).into()
    }

    /// A receipt as its paper: the place and day, each item and what it
    /// cost (an item pressed is filed), and the total, monospace on white.
    fn view_paper<'a>(&'a self, r: &'a ReceiptLine, photo: Option<&'a PhotoLine>) -> Element<'a, Message> {
        let ink = Color::from_rgb8(0x1a, 0x1a, 0x1a);
        let faded = Color::from_rgb8(0x77, 0x77, 0x77);
        let mono = |s: String, size: u32, c: Color| text(s).font(Font::MONOSPACE).size(size).color(c);
        let rule = || mono("-".repeat(34), 12, faded);
        let mut paper: Column<'a, Message> = column![
            container(mono(place_of(r).to_uppercase(), 15, ink)).center_x(Length::Fill),
            container(mono(r.date.clone(), 12, faded)).center_x(Length::Fill),
            rule(),
        ]
        .spacing(3);
        for i in self.peer.receipt_items() {
            let filed = match i.category_id {
                Some(c) => self.peer.category_name(c).unwrap_or_else(|| i.category.clone()),
                None => "file it".into(),
            };
            let tint = if i.category_id.is_some() {
                faded
            } else {
                Color::from_rgb8(0xb2, 0x5b, 0x00)
            };
            let line = column![
                row![mono(i.name.clone(), 12, ink).width(Length::Fill), mono(money(-i.amount), 12, ink)].spacing(8),
                mono(
                    match i.quantity {
                        1000 => format!("  {filed}"),
                        q => format!("  {} \u{b7} {filed}", crate::view::quantity(q, &i.unit)),
                    },
                    10,
                    tint
                ),
            ];
            let item = vec![ItemRef {
                receipt_id: i.receipt_id.clone(),
                line: i.line,
            }];
            paper = paper.push(mouse_area(line).on_press(Message::FileItems(item)));
        }
        paper = paper
            .push(rule())
            .push(row![mono("TOTAL".into(), 13, ink).width(Length::Fill), mono(money(-r.total), 13, ink)]);
        if let Some(p) = photo.filter(|p| p.state != "sure") {
            paper = paper.push(container(mono(format!("read: {}", p.state), 10, faded)).center_x(Length::Fill));
        }
        container(paper)
            .padding(16)
            .width(Length::Fixed(320.0))
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(Color::WHITE)),
                border: iced::Border {
                    color: Color::from_rgb8(0xdd, 0xdd, 0xdd),
                    width: 1.0,
                    radius: 2.0.into(),
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.18),
                    offset: iced::Vector::new(0.0, 2.0),
                    blur_radius: 6.0,
                },
                ..container::Style::default()
            })
            .into()
    }

    /// A receipt being typed: where, when, and each item.
    fn view_paper_form<'a>(&'a self, p: &'a Paper) -> Element<'a, Message> {
        let input = |placeholder: &str, value: &'a str, on: fn(String) -> Msg| {
            text_input(placeholder, value)
                .on_input(move |v| Message::Receipt(on(v)))
                .on_submit(Message::Receipt(Msg::Save))
                .size(12)
                .padding([3, 6])
                .style(look::input)
        };
        let mut form = column![
            bold(
                if p.receipt_id.is_empty() {
                    "A receipt, typed"
                } else {
                    "The receipt, put right"
                },
                14
            ),
            row![
                input("Store", &p.place, Msg::Place).width(Length::Fixed(220.0)),
                input("2026-10-26", &p.date, Msg::Date).width(Length::Fixed(110.0)),
            ]
            .spacing(8),
        ]
        .spacing(6);
        for (i, (name, cost)) in p.items.iter().enumerate() {
            let mut line = row![
                text_input("Item", name)
                    .on_input(move |v| Message::Receipt(Msg::Name(i, v)))
                    .on_submit(Message::Receipt(Msg::Save))
                    .size(12)
                    .padding([3, 6])
                    .width(Length::Fixed(260.0))
                    .style(look::input),
                text_input("0.00", cost)
                    .on_input(move |v| Message::Receipt(Msg::Amount(i, v)))
                    .on_submit(Message::Receipt(Msg::Save))
                    .size(12)
                    .padding([3, 6])
                    .width(Length::Fixed(90.0))
                    .style(look::input),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if p.items.len() > 1 {
                line = line.push(link("Remove", Message::Receipt(Msg::RemoveItem(i))));
            }
            form = form.push(line);
        }
        let sum: i64 = p.items.iter().filter_map(|(_, c)| amount::parse(c)).sum();
        form = form.push(
            row![
                link("+ Add an item", Message::Receipt(Msg::AddItem)),
                dim(format!("Total {}", money(sum)), 12),
                fill(),
                quiet("Cancel", Message::Receipt(Msg::Cancel)),
                action("Save receipt", Message::Receipt(Msg::Save)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
        container(form.width(Length::Fixed(560.0))).padding([10, 24]).into()
    }

    /// The photo modal's body.
    pub fn view_photo(&self) -> Element<'_, Message> {
        let body: Element<'_, Message> = match &self.photo {
            Some((_, h)) => image(h.clone()).height(Length::Fixed(640.0)).into(),
            None => dim("Fetching the photo\u{2026}", 13).into(),
        };
        column![
            body,
            row![
                Space::new().width(Length::Fill),
                button(text("Close").size(13)).on_press(Message::CloseModal).style(style::quiet)
            ]
        ]
        .spacing(10)
        .into()
    }
}

/// A receipt's store, as its paper names it.
fn place_of(r: &ReceiptLine) -> String {
    match (r.place.trim(), r.store.as_str()) {
        ("", s) => crate::view::store_name(s),
        (p, _) => p.into(),
    }
}
