//! The inspector's "Assign by formula": a category's assignment as a sum
//! of terms, each a factor of what a saved filter adds up to in the month
//! — "10% × SUM(ACME paychecks)" — typed as rows (a filter picked from a
//! menu, a factor typed as "10%", "0.1" or "1.5x"), with what it would
//! give the month on screen as it is typed, and each term's filter opened
//! as a register limited to the month. Saved, it holds from the month on
//! screen on (`money_domain::formulas`).

use arkui::{glyphs, icon, menu};
use iced::widget::{button, column, container, row, text, text_input};
use iced::{Alignment, Element, Length, Task};
use money_domain::dates::month_name;
use money_domain::formulas::{parse_share, percent};
use money_domain::*;

use crate::amount::money;
use crate::look;
use crate::peer::Ledger;
use crate::view::{action, bold, dim, fill, link, quiet};
use crate::{App, Message};

/// One term as typed: the filter it sums, and its factor.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TermForm {
    pub filter: Option<Id<SavedFilter>>,
    pub factor: String,
}

/// The formula as typed.
#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub terms: Vec<TermForm>,
}

impl Default for Form {
    /// One term to fill in.
    fn default() -> Form {
        Form {
            terms: vec![TermForm::default()],
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// A term's filter: its menu, and one chosen from it.
    Pick(usize),
    Chose(usize, Id<SavedFilter>),
    Factor(usize, String),
    Add,
    Remove(usize),
    /// A term's filter opened as a register, limited to the month.
    Open(usize),
    Save,
    Clear,
}

impl Form {
    /// The terms, when every one is a filter and a factor.
    pub fn typed(&self) -> Option<Vec<Term>> {
        self.terms
            .iter()
            .map(|t| {
                let (times, per) = parse_share(&t.factor)?;
                Some(Term {
                    filter_id: t.filter?,
                    times,
                    per,
                })
            })
            .collect::<Option<Vec<Term>>>()
            .filter(|t| !t.is_empty())
    }
}

impl App {
    pub fn formula_msg(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::Pick(i) => self.pick_filter(i),
            Msg::Chose(i, id) => {
                if let Some(t) = self.formula.terms.get_mut(i) {
                    t.filter = Some(id);
                    if t.factor.trim().is_empty() {
                        t.factor = "100%".into();
                    }
                }
                self.preview_formula();
            }
            Msg::Factor(i, s) => {
                if let Some(t) = self.formula.terms.get_mut(i) {
                    t.factor = s;
                }
                self.preview_formula();
            }
            Msg::Add => {
                if self.formula.terms.len() < money_domain::formulas::TERMS {
                    self.formula.terms.push(TermForm::default());
                }
                self.preview_formula();
            }
            Msg::Remove(i) => {
                if i < self.formula.terms.len() {
                    self.formula.terms.remove(i);
                }
                if self.formula.terms.is_empty() {
                    self.formula.terms.push(TermForm::default());
                }
                self.preview_formula();
            }
            Msg::Open(i) => {
                let Some(id) = self.formula.terms.get(i).and_then(|t| t.filter) else {
                    return Task::none();
                };
                let Some(saved) = self.peer.saved(id) else { return Task::none() };
                let f = saved.filter.within(&self.peer.month);
                return self.open_filter(f, Ledger::Saved(id));
            }
            Msg::Save => self.save_formula(),
            Msg::Clear => {
                if let Some(c) = self.selected {
                    let r = ClearFormula {
                        category_id: c,
                        from: Some(self.peer.month.clone()),
                    };
                    if self.author("remove the formula", "categories.clear_formula", &r) {
                        self.note = format!("assigned by hand from {}", month_name(&self.peer.month));
                        self.load_formula();
                    }
                }
            }
        }
        Task::none()
    }

    /// The saved filters, for a term to sum, in a menu at the pointer.
    fn pick_filter(&mut self, i: usize) {
        if self.peer.filters.rows.is_empty() {
            self.note = "no filters yet: open a register, set its conditions, and Save as filter\u{2026}".into();
            return;
        }
        let chosen = self.formula.terms.get(i).and_then(|t| t.filter);
        let entries = self
            .peer
            .filters
            .rows
            .iter()
            .map(|f| {
                let glyph = if chosen == Some(f.id) { glyphs::TICKED } else { glyphs::SEARCH };
                menu::Entry::run(glyph, f.name.clone(), Message::Formula(Msg::Chose(i, f.id)))
            })
            .collect();
        self.ctx.open_menu(menu::Menu::open(
            "Sum a filter",
            entries,
            self.pointer,
            self.window,
            menu::Anchor::Pointer,
        ));
    }

    /// The inspector's formula, as the selected category's month follows
    /// it, or one term to fill in; and its preview.
    pub fn load_formula(&mut self) {
        self.formula = match self.selected.and_then(|c| self.peer.formula(c)) {
            Some(f) => Form {
                terms: f
                    .terms
                    .iter()
                    .map(|t| TermForm {
                        filter: Some(t.filter_id),
                        factor: percent(t.times, t.per),
                    })
                    .collect(),
            },
            None => Form::default(),
        };
        self.preview_formula();
    }

    /// What the formula as typed would assign this month, watched.
    pub fn preview_formula(&mut self) {
        let p = match (self.selected, self.formula.typed()) {
            (Some(_), Some(terms)) => Some(PreviewFormula {
                month: self.peer.month.clone(),
                terms,
            }),
            _ => None,
        };
        self.peer.set_preview(p);
    }

    /// The formula as typed, saved from the month on screen.
    fn save_formula(&mut self) {
        let Some(c) = self.selected else { return };
        if self.formula.terms.iter().any(|t| t.filter.is_none()) {
            self.note = "every term sums a filter: choose one".into();
            return;
        }
        if let Some(t) = self.formula.terms.iter().find(|t| parse_share(&t.factor).is_none()) {
            self.note = format!("{:?} is not a factor, as 10%, 0.1 or 1.5x", t.factor.trim());
            return;
        }
        let Some(terms) = self.formula.typed() else { return };
        let f = SetFormula {
            category_id: c,
            from: Some(self.peer.month.clone()),
            terms,
        };
        if self.author("save the formula", "categories.set_formula", &f) {
            self.note = format!("assigned by formula from {}", month_name(&self.peer.month));
            self.load_formula();
        }
    }

    /// "Assign by formula": a row per term — the filter it sums, its
    /// factor, what it summed this month, Open — then what the formula
    /// would give the month on screen, Save and Remove.
    pub fn inspect_formula<'a>(&'a self, l: &'a BudgetLine) -> Element<'a, Message> {
        let month = month_name(&self.peer.month);
        let mut c = column![row![
            bold("Assign by formula", 14),
            fill(),
            icon::faint(glyphs::REPEAT, false).width(13.0).height(13.0)
        ]
        .align_y(Alignment::Center)]
        .spacing(8);
        c = c.push(match self.peer.formula(l.category_id) {
            Some(f) => dim(format!("{} \u{2014} since {}.", f.text, month_name(&f.from)), 11),
            None => dim("Each month, what saved filters add up to, times a factor: like a spreadsheet's cell.", 11),
        });
        let preview = self.peer.preview.as_ref().and_then(|p| p.rows.first());
        for (i, t) in self.formula.terms.iter().enumerate() {
            let name = t
                .filter
                .and_then(|id| self.peer.saved(id))
                .map_or("Choose a filter".to_string(), |f| f.name.clone());
            let pick = button(
                row![
                    text(name).size(12).wrapping(text::Wrapping::None).width(Length::Fill),
                    icon::faint(glyphs::DOWN, false).width(10.0).height(10.0)
                ]
                .align_y(Alignment::Center),
            )
            .on_press(Message::Formula(Msg::Pick(i)))
            .padding([4, 6])
            .width(Length::Fill)
            .style(look::choice(t.filter.is_some()));
            let factor = text_input("10%", &t.factor)
                .on_input(move |s| Message::Formula(Msg::Factor(i, s)))
                .on_submit(Message::Formula(Msg::Save))
                .size(12)
                .padding([4, 6])
                .width(Length::Fixed(58.0))
                .style(look::input);
            let remove = button(icon::faint(glyphs::CLOSE, false).width(12.0).height(12.0))
                .on_press(Message::Formula(Msg::Remove(i)))
                .padding(3)
                .style(arkui::style::quiet);
            let line = row![factor, text("\u{d7}").size(12), pick, remove].spacing(4).align_y(Alignment::Center);
            // What its filter summed in the month, and the register for it.
            let summed = preview.and_then(|p| p.terms.get(i)).filter(|s| Some(s.filter_id) == t.filter);
            let mut under = row![].spacing(8).align_y(Alignment::Center);
            if let Some(s) = summed {
                under = under.push(dim(format!("SUM = {} ({} in {month})", money(s.sum), s.count), 11));
            }
            if t.filter.is_some() {
                under = under.push(fill()).push(link("Open", Message::Formula(Msg::Open(i))));
            }
            c = c.push(column![line, under].spacing(2));
        }
        if self.formula.terms.len() < money_domain::formulas::TERMS {
            c = c.push(link("+ Add a term", Message::Formula(Msg::Add)));
        }
        c = c.push(match preview {
            Some(p) => column![App::figure(&format!("{month} by this formula"), p.amount), dim(p.text.clone(), 11)].spacing(4),
            None => column![dim("A filter and a factor for every term, to see what it gives this month.", 11)],
        });
        let mut buttons = row![action("Save", Message::Formula(Msg::Save))].spacing(8).align_y(Alignment::Center);
        if l.ruled {
            buttons = buttons.push(quiet("Remove formula", Message::Formula(Msg::Clear)));
        }
        c = c.push(dim(format!("Saved, it assigns from {month} on; months before keep theirs."), 11));
        c = c.push(buttons);
        container(c).padding(14).style(look::card).into()
    }
}
