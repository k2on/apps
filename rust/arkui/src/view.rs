//! A view laid out once in Rust (`ark_app::view`, docs/ark-app.md), drawn
//! by iced in ArkUI's parts: what `#[view]`'s `iced(&record)` entry point
//! returns. The desktop's measures are the desktop's (docs/arkui-parts.md):
//! a card rounded by 8, rows lit by the theme's pressed colour, no give.
//!
//! What iced has no twin for is said here, not left out silently: a row's
//! menu is its right click, its first item (iced has no context menu on a
//! row yet), and a sheet or a prompt is a dialog over the page.

use ark_app::view::{Input, ListRow, Part, Picture, Section, Val, View};
use iced::widget::{button, checkbox, column, container, mouse_area, row, scrollable, stack, text, text_input, Column, Space};
use iced::{Alignment, Length};

use crate::{glyphs, icon, layer, parts, style};

pub use iced::Element;

/// What a page is padded by.
const PAD: f32 = 18.0;
/// A dialog's width.
const DIALOG: f32 = 420.0;

/// `view` as iced draws it.
pub fn element<M: Clone + 'static>(view: View<M>) -> Element<'static, M> {
    match view {
        View::Part(Part::Page { title, top, content, .. }) => page(title, View::List(vec![*top, *content])),
        v => column(nodes(v)).spacing(8).into(),
    }
}

fn page<M: Clone + 'static>(title: &'static str, content: View<M>) -> Element<'static, M> {
    let mut body = vec![];
    let mut over = vec![];
    for v in flatten(content) {
        match v {
            View::Part(Part::Sheet { .. } | Part::Prompt { .. }) => over.push(v),
            v => body.extend(nodes(v)),
        }
    }
    let list = column![text(title).size(26).font(crate::hero::BOLD)]
        .extend(body)
        .spacing(10)
        .padding(PAD)
        .max_width(640);
    let mut layers = stack![scrollable(container(list).center_x(Length::Fill)).height(Length::Fill)];
    for v in over {
        if let Some((dismiss, dialog)) = dialog(v) {
            layers = layers.push(layer::dimmed(dismiss.clone(), dismiss)).push(layer::centred(dialog));
        }
    }
    layers.into()
}

/// `v` with its lists opened out: what lies one after another.
fn flatten<M>(v: View<M>) -> Vec<View<M>> {
    match v {
        View::List(vs) => vs.into_iter().flat_map(flatten).collect(),
        View::Empty => vec![],
        v => vec![v],
    }
}

fn nodes<M: Clone + 'static>(v: View<M>) -> Vec<Element<'static, M>> {
    flatten(v).into_iter().filter_map(part).collect()
}

fn part<M: Clone + 'static>(v: View<M>) -> Option<Element<'static, M>> {
    let View::Part(p) = v else {
        // A recording's `for` and `if` are the emitters'; a real view has none.
        return None;
    };
    Some(match p {
        Part::Hero { label, cents, note } => column![
            text(label).size(13).style(style::faint(0.55)),
            amount(cents.get(), 34, false),
            text(note).size(12).style(style::dim),
        ]
        .spacing(2)
        .into(),
        Part::Section(s) => section(s),
        Part::Row(r) => list_row(*r),
        Part::NameField {
            label,
            value,
            problem,
            typed,
        } => field(label, value.get(), problem.get(), typed),
        Part::AmountField { label, typed, problem, on } => field(&label.get(), typed.get(), problem.get(), on),
        Part::Chips(c) => row(nodes(*c)).spacing(4).wrap().into(),
        Part::Chip { label, selected, tap, .. } => parts::chip(&label.get(), selected.get(), tap),
        Part::Toggle { title, note, on, change } => column![
            checkbox(on.get()).label(title).on_toggle(change.0),
            text(note.get()).size(12).style(style::dim),
        ]
        .spacing(2)
        .into(),
        Part::Button { title, enabled, tap } => button(text(title)).on_press_maybe(enabled.get().then_some(tap)).into(),
        Part::Page { .. } | Part::Sheet { .. } | Part::Prompt { .. } => return None,
    })
}

/// A dialog for a sheet or a prompt, and what puts it away.
fn dialog<M: Clone + 'static>(v: View<M>) -> Option<(M, Element<'static, M>)> {
    let View::Part(p) = v else { return None };
    let (dismiss, content): (M, Column<'static, M>) = match p {
        Part::Sheet { title, dismiss, content } => (dismiss, column![text(title).size(18).font(crate::hero::BOLD)].extend(nodes(*content))),
        Part::Prompt {
            title,
            text: t,
            typed,
            confirm,
            ok,
            cancel,
        } => (
            cancel.clone(),
            column![
                text(title).size(18).font(crate::hero::BOLD),
                text_input("Name", &t.get()).on_input(typed.0).on_submit(ok.clone()).style(style::field),
                row![
                    Space::new().width(Length::Fill),
                    button(text("Cancel")).on_press(cancel),
                    button(text(confirm)).on_press(ok)
                ]
                .spacing(8),
            ],
        ),
        _ => return None,
    };
    let panel = container(content.spacing(12)).padding(20).width(DIALOG).style(style::card);
    Some((dismiss, panel.into()))
}

fn section<M: Clone + 'static>(s: Section<M>) -> Element<'static, M> {
    let shows = s.shows.as_ref().is_none_or(|b| b.get());
    let trailing = s.amount.filter(|_| shows).map(|a| {
        let cents = a.get();
        amount(cents, 13, cents >= 0)
    });
    let mut out = column![].spacing(0);
    if let Some(t) = s.title {
        out = out.push(parts::section_header(&t.get(), trailing));
    }
    out = out.push(parts::group_card(nodes(*s.rows)));
    if let Some(note) = s.footer.and_then(|f| f.get()) {
        out = out.push(container(text(note).size(12).style(style::dim)).padding([6, 4]));
    }
    out.into()
}

fn list_row<M: Clone + 'static>(r: ListRow<M>) -> Element<'static, M> {
    let mut line = row![].spacing(10).align_y(Alignment::Center);
    match &r.picture {
        Picture::None => {}
        Picture::Tile(g) => line = line.push(parts::tile(shape(g), |t| style::dim(t).color.unwrap_or_default(), 28.0)),
        Picture::Mark { glyph, .. } => line = line.push(parts::tile(shape(&glyph.get()), |t| crate::theme::of(t).primary.base.color, 28.0)),
        Picture::Glyph(g) => line = line.push(icon::plain(shape(g), false)),
    }
    let title = text(r.title.get()).size(14);
    let words: Element<'static, M> = match &r.subtitle {
        Some(s) => column![title, text(s.get()).size(12).style(style::dim)].spacing(1).into(),
        None => title.into(),
    };
    line = line.push(words).push(Space::new().width(Length::Fill));
    if let Some(n) = r.badge.and_then(|b| b.get()) {
        line = line.push(parts::badge(n as usize));
    }
    if let Some(a) = r.amount {
        line = line.push(amount(a.get(), 14, r.quiet));
    }
    let mut b = button(line).width(Length::Fill).padding([2, 0]).style(style::bare);
    if let Some(t) = r.tap {
        b = b.on_press(t);
    }
    let mut area = mouse_area(b);
    if let Some(first) = r.menu.into_iter().next() {
        area = area.on_right_press(first.tap);
    }
    area.into()
}

fn field<M: Clone + 'static>(label: &str, value: String, problem: Option<String>, typed: Input<String, M>) -> Element<'static, M> {
    let mut c = column![
        text(label.to_string()).size(12).style(style::dim),
        text_input(label, &value).on_input(typed.0).style(style::field)
    ]
    .spacing(4);
    if let Some(p) = problem {
        c = c.push(text(p).size(12).style(|t| text::Style {
            color: Some(crate::theme::of(t).danger.base.color),
        }));
    }
    c.into()
}

/// An amount: red below nothing, or quiet.
fn amount<M: 'static>(cents: i64, size: u32, quiet: bool) -> Element<'static, M> {
    text(ark_format::amount(cents, &ark_format::NumberStyle::us()))
        .size(size)
        .style(move |t| text::Style {
            color: Some(match (cents < 0, quiet) {
                (true, _) => crate::theme::of(t).danger.base.color,
                (false, true) => style::dim(t).color.unwrap_or_default(),
                (false, false) => crate::theme::of(t).background.base.text,
            }),
        })
        .into()
}

fn shape(name: &str) -> &'static [u8] {
    glyphs::named(name).unwrap_or(glyphs::NOTE)
}

/// A value read: what a real view holds.
#[allow(dead_code)]
fn is<T: Clone + Default>(v: &Val<T>) -> T {
    v.get()
}
