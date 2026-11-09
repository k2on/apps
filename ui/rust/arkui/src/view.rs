//! A view laid out once in Rust (`ark_app::view`, ui/docs/app.md), drawn
//! by iced in ArkUI's parts: what `#[view]`'s `iced(&record)` entry point
//! returns. The desktop's measures are the desktop's (ui/docs/parts.md):
//! a card rounded by 8, rows lit by the theme's pressed colour, no give.
//!
//! What iced has no twin for is said here, not left out silently: a row's
//! menu is ArkUI's own menu at the pointer ([`crate::context_menu`]:
//! right click, a long press, or the menu key), a sheet or a prompt is a
//! dialog over the page, a picker is a row of chips, and an amount is typed.

use crate::text::{self, text};
use crate::text_input::text_input;
use ark_app::view::{Confirm, Input, ListRow, Part, Picture, Placement, Section, Val, View};
use iced::widget::{button, checkbox, column, container, row, rule, scrollable, stack, Column, Space};
use iced::{Alignment, Length};

use crate::layout_tokens::{CONTENT_WIDTH, INSPECTOR_WIDTH, SIDEBAR_WIDTH, TOOLBAR_HEIGHT};
use crate::shell::{Frame, Pane, SizeClass};
use crate::{context_menu, glyphs, icon, layer, parts, sidebar, style};

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
        Part::FormSection { header, footer, content } => {
            let mut c = column![].spacing(6);
            if let Some(h) = header {
                c = c.push(text(h.get()).size(12).style(style::dim));
            }
            c = c.extend(nodes(*content));
            if let Some(f) = footer {
                c = c.push(text(f.get()).size(12).style(style::dim));
            }
            c.into()
        }
        Part::Picker { selected, pick, choices, .. } => {
            let chosen = selected.get();
            let chips = flatten(*choices).into_iter().filter_map(|v| match v {
                View::Part(Part::Choice { title, tag, .. }) => {
                    let tag = tag.get();
                    Some(parts::chip(&title.get(), tag == chosen, pick.call(tag)))
                }
                _ => None,
            });
            row(chips).spacing(4).wrap().into()
        }
        Part::Choice { .. } => return None,
        Part::AmountEntry {
            label,
            typed,
            problem,
            on_typed,
            ..
        } => field(&label.get(), typed.get(), problem.get(), on_typed),
        Part::Chip { label, selected, tap, .. } => parts::chip(&label.get(), selected.get(), tap),
        Part::Toggle { title, note, on, change } => {
            let mut c = column![checkbox(on.get()).label(title).on_toggle(move |b| change.call(b))].spacing(2);
            if let Some(n) = note {
                c = c.push(text(n.get()).size(12).style(style::dim));
            }
            c.into()
        }
        Part::Button { title, enabled, tap } => button(text(title)).on_press_maybe(enabled.get().then_some(tap)).into(),
        Part::BarItem {
            title,
            glyph,
            enabled,
            tap,
            placement,
        } => bar_button(title, glyph, placement, enabled.get(), tap),
        // The shell's are drawn by `shell`, with the window's frame; a
        // hosted screen and the dialogs need it too.
        Part::Page { .. }
        | Part::Sheet { .. }
        | Part::Prompt { .. }
        | Part::Tabs { .. }
        | Part::SectionTab { .. }
        | Part::Split { .. }
        | Part::Toolbar { .. }
        | Part::Searchable { .. }
        | Part::Inspector { .. }
        | Part::Hosted { .. }
        | Part::Confirm { .. }
        | Part::Alert { .. }
        | Part::SettingsPanes { .. }
        | Part::SettingsPane { .. } => return None,
    })
}

// * The shell (ark_app::shell)

/// A section's title, glyph, tag, badge and content.
type Tab<M> = (&'static str, &'static str, &'static str, Option<i64>, View<M>);

/// A shell's view as iced draws it, a standard Mac window's way: the
/// sections a sidebar (tabs at the foot when compact), a list beside what
/// it opens (one at a time when narrower than expanded), a toolbar row
/// with the search field in it, an inspector at the trailing edge (a sheet
/// when compact), a question or a notice over everything. `frame` is the
/// window's width and which pane has the keys (`crate::shell`); each
/// hosted screen is what `hosts` draws for its name.
pub fn shell<'a, M: Clone + 'static>(view: View<M>, frame: Frame, hosts: &mut dyn FnMut(&'static str) -> Element<'a, M>) -> Element<'a, M> {
    draw(view, frame, hosts)
}

fn draw<'a, M: Clone + 'static>(v: View<M>, frame: Frame, hosts: &mut dyn FnMut(&'static str) -> Element<'a, M>) -> Element<'a, M> {
    let mut body: Vec<Element<'a, M>> = vec![];
    let mut over: Vec<Element<'a, M>> = vec![];
    for v in flatten(v) {
        let View::Part(p) = v else { continue };
        match p {
            Part::Confirm { .. } | Part::Alert { .. } | Part::Sheet { .. } | Part::Prompt { .. } => {
                if let Some((dismiss, panel)) = notice(p) {
                    over.push(layer::dimmed(dismiss.clone(), dismiss));
                    over.push(layer::centred(panel));
                }
            }
            p => body.push(shell_part(p, frame, hosts)),
        }
    }
    let base: Element<'a, M> = match body.len() {
        1 => body.pop().unwrap_or_else(|| Space::new().into()),
        _ => column(body).spacing(8).into(),
    };
    match over.is_empty() {
        true => base,
        false => stack(std::iter::once(base).chain(over)).into(),
    }
}

fn shell_part<'a, M: Clone + 'static>(p: Part<M>, frame: Frame, hosts: &mut dyn FnMut(&'static str) -> Element<'a, M>) -> Element<'a, M> {
    match p {
        Part::Tabs { selected, select, tabs } => {
            let chosen = selected.get();
            let tabs: Vec<Tab<M>> = flatten(*tabs)
                .into_iter()
                .filter_map(|v| match v {
                    View::Part(Part::SectionTab {
                        title,
                        glyph,
                        tag,
                        badge,
                        content,
                    }) => Some((title, glyph, tag, badge.and_then(|b| b.get()), *content)),
                    _ => None,
                })
                .collect();
            let at = tabs.iter().position(|t| t.2 == chosen).unwrap_or(0);
            let heads: Vec<_> = tabs.iter().map(|t| (t.0, t.1, t.2, t.3)).collect();
            let content = tabs.into_iter().nth(at).map(|t| t.4).unwrap_or(View::Empty);
            let shown = draw(content, frame, hosts);
            match frame.class() {
                SizeClass::Compact => {
                    let bar = row(heads.into_iter().enumerate().map(|(i, (title, glyph, tag, _))| {
                        let on = i == at;
                        let label = column![
                            icon::line(shape(glyph), on),
                            text(title).size(10).style(move |t| text::Style {
                                color: Some(match on {
                                    true => crate::theme::of(t).primary.base.color,
                                    false => style::entry_text(t, false),
                                }),
                                ..Default::default()
                            })
                        ]
                        .spacing(2)
                        .align_x(Alignment::Center);
                        button(container(label).center_x(Length::Fill))
                            .width(Length::Fill)
                            .padding([6, 0])
                            .style(style::bare)
                            .on_press(select.call(tag.to_string()))
                            .into()
                    }));
                    column![
                        container(shown).height(Length::Fill),
                        rule::horizontal(1).style(style::edge),
                        container(bar).style(style::surface)
                    ]
                    .into()
                }
                _ => {
                    let items = heads
                        .into_iter()
                        .enumerate()
                        .map(|(i, (title, glyph, tag, badge))| sidebar::Item {
                            glyph: shape(glyph),
                            label: title.to_string(),
                            heading: None,
                            trailing: badge.map(|n| sidebar::count(n, sidebar::lit(i, at, frame.focus == Pane::Sidebar))),
                            press: select.call(tag.to_string()),
                        })
                        .collect();
                    let side = sidebar::view(items, at, frame.focus == Pane::Sidebar, SIDEBAR_WIDTH, PAD, "ark-shell-sidebar");
                    row![side, rule::vertical(1).style(style::edge), container(shown).width(Length::Fill)]
                        .height(Length::Fill)
                        .into()
                }
            }
        }
        Part::SectionTab { content, .. } | Part::SettingsPane { content, .. } => draw(*content, frame, hosts),
        Part::Split {
            content,
            detail,
            showing,
            back,
        } => match (frame.class(), showing.get()) {
            (SizeClass::Expanded, _) => row![
                container(draw(*content, frame, hosts)).width(CONTENT_WIDTH).height(Length::Fill),
                rule::vertical(1).style(style::edge),
                container(draw(*detail, frame, hosts)).width(Length::Fill).height(Length::Fill),
            ]
            .into(),
            (_, true) => column![
                container(bar_button("Back", "chevron-left", Placement::Navigation, true, back)).padding([4, 8]),
                container(draw(*detail, frame, hosts)).height(Length::Fill),
            ]
            .into(),
            (_, false) => draw(*content, frame, hosts),
        },
        Part::Toolbar { title, items, content } => {
            // The search field goes in the toolbar, as the Mac's does.
            let (search, content) = match *content {
                View::Part(Part::Searchable {
                    prompt,
                    text: t,
                    typed,
                    content,
                }) => (Some(search(prompt, t.get(), typed)), *content),
                c => (None, c),
            };
            let items: Vec<(Placement, Element<'a, M>)> = flatten(*items)
                .into_iter()
                .filter_map(|v| match v {
                    View::Part(Part::BarItem {
                        title,
                        glyph,
                        placement,
                        enabled,
                        tap,
                    }) => Some((placement, bar_button(title, glyph, placement, enabled.get(), tap))),
                    _ => None,
                })
                .collect();
            let mut line = row![].spacing(6).align_y(Alignment::Center);
            let mut principal = vec![];
            let mut trailing = vec![];
            for (placement, item) in items {
                match placement {
                    Placement::Navigation => line = line.push(item),
                    Placement::Principal => principal.push(item),
                    Placement::Primary | Placement::Secondary => trailing.push(item),
                }
            }
            line = line.push(text(title.get()).size(15).font(crate::hero::BOLD));
            line = line.push(Space::new().width(Length::Fill));
            line = line.extend(principal);
            line = line.push(Space::new().width(Length::Fill));
            let (in_bar, under) = match frame.class() == SizeClass::Compact {
                true => (None, search),
                false => (search, None),
            };
            if let Some(s) = in_bar {
                line = line.push(container(s).width(220));
            }
            line = line.extend(trailing);
            let mut out = column![
                container(line).height(TOOLBAR_HEIGHT).padding([0, 12]).center_y(TOOLBAR_HEIGHT),
                rule::horizontal(1).style(style::edge)
            ];
            if let Some(s) = under {
                out = out.push(container(s).padding([6, 12]));
            }
            out.push(container(draw(content, frame, hosts)).height(Length::Fill)).into()
        }
        Part::Searchable {
            prompt,
            text: t,
            typed,
            content,
        } => column![container(search(prompt, t.get(), typed)).padding([6, 12]), draw(*content, frame, hosts)].into(),
        Part::Inspector {
            title,
            shown,
            toggle,
            content,
            inspector,
        } => {
            let content = draw(*content, frame, hosts);
            if !shown.get() {
                return content;
            }
            let pane = column![
                container(text(title).size(13).font(crate::hero::BOLD)).padding([10, 12]),
                container(draw(*inspector, frame, hosts)).height(Length::Fill)
            ];
            match frame.class() {
                SizeClass::Compact => {
                    let close = toggle.call(false);
                    stack![
                        content,
                        layer::dimmed(close.clone(), close),
                        container(container(pane).height(Length::FillPortion(1)).style(style::card))
                            .height(Length::Fill)
                            .align_y(iced::alignment::Vertical::Bottom)
                    ]
                    .into()
                }
                _ => row![
                    container(content).width(Length::Fill),
                    rule::vertical(1).style(style::edge),
                    container(pane).width(INSPECTOR_WIDTH).height(Length::Fill).style(style::surface)
                ]
                .into(),
            }
        }
        Part::Hosted { name } => hosts(name),
        Part::SettingsPanes { panes } => {
            let panes = flatten(*panes).into_iter().filter_map(|v| match v {
                View::Part(Part::SettingsPane { title, content, .. }) => Some(
                    column![text(title).size(15).font(crate::hero::BOLD), draw(*content, frame, hosts)]
                        .spacing(8)
                        .into(),
                ),
                _ => None,
            });
            scrollable(column(panes.collect::<Vec<Element<'a, M>>>()).spacing(24).padding(PAD)).into()
        }
        Part::Page { title, top, content, .. } => page(title, View::List(vec![*top, *content])),
        p => part(View::Part(p)).unwrap_or_else(|| Space::new().into()),
    }
}

/// A toolbar's button: its glyph, and its title beside it on the desktop
/// (a Mac toolbar's "icon and text"), quiet when it is secondary.
fn bar_button<'a, M: Clone + 'a>(title: &'static str, glyph: &'static str, placement: Placement, enabled: bool, tap: M) -> Element<'a, M> {
    let quiet = placement == Placement::Secondary;
    let label = row![
        icon::line(shape(glyph), false),
        text(title).size(13).style(move |t| match quiet {
            true => style::dim(t),
            false => text::Style::default(),
        })
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    button(label)
        .padding([4, 8])
        .style(style::bare)
        .on_press_maybe(enabled.then_some(tap))
        .into()
}

fn search<'a, M: Clone + 'static>(prompt: &'static str, value: String, typed: Input<String, M>) -> Element<'a, M> {
    parts::search_field(prompt, &value).on_input(move |s| typed.call(s)).into()
}

/// A question or a notice over the window, and what puts it away.
fn notice<M: Clone + 'static>(p: Part<M>) -> Option<(M, Element<'static, M>)> {
    let (dismiss, content): (M, Column<'static, M>) = match p {
        Part::Confirm {
            title,
            message,
            actions,
            cancel,
        } => {
            let mut buttons = row![Space::new().width(Length::Fill), button(text("Cancel")).on_press(cancel.clone())].spacing(8);
            for a in actions {
                let destructive = a.destructive;
                buttons = buttons.push(
                    button(text(a.title.get()).style(move |t| match destructive {
                        true => text::Style {
                            color: Some(crate::theme::of(t).danger.base.color),
                            ..Default::default()
                        },
                        false => text::Style::default(),
                    }))
                    .on_press(a.tap),
                );
            }
            (
                cancel,
                column![
                    text(title.get()).size(18).font(crate::hero::BOLD),
                    text(message.get()).size(13).style(style::dim),
                    buttons
                ],
            )
        }
        Part::Alert { title, message, ok } => (
            ok.clone(),
            column![
                text(title.get()).size(18).font(crate::hero::BOLD),
                text(message.get()).size(13).style(style::dim),
                row![Space::new().width(Length::Fill), button(text("OK")).on_press(ok)]
            ],
        ),
        p => return dialog(View::Part(p)),
    };
    let panel = container(content.spacing(12)).padding(20).width(DIALOG).style(style::card);
    Some((dismiss, panel.into()))
}

/// A dialog for a sheet or a prompt, and what puts it away.
fn dialog<M: Clone + 'static>(v: View<M>) -> Option<(M, Element<'static, M>)> {
    let View::Part(p) = v else { return None };
    let (dismiss, content): (M, Column<'static, M>) = match p {
        Part::Sheet {
            title,
            dismiss,
            confirm,
            content,
        } => {
            let mut c = column![text(title).size(18).font(crate::hero::BOLD)].extend(nodes(*content));
            if let Some(Confirm { title, enabled, tap }) = confirm {
                c = c.push(
                    row![
                        Space::new().width(Length::Fill),
                        button(text("Cancel")).on_press(dismiss.clone()),
                        button(text(title)).on_press_maybe(enabled.get().then_some(tap)),
                    ]
                    .spacing(8),
                );
            }
            (dismiss, c)
        }
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
                text_input("Name", &t.get())
                    .on_input(move |s| typed.call(s))
                    .on_submit(ok.clone())
                    .style(style::field),
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
    if r.pushes {
        line = line.push(icon::plain(glyphs::CHEVRON, true));
    }
    let mut b = button(line).width(Length::Fill).padding([2, 0]).style(style::bare);
    if let Some(t) = r.tap {
        b = b.on_press(t);
    }
    let items: Vec<context_menu::Item<M>> = r
        .menu
        .into_iter()
        .filter(|m| m.offered())
        .map(|m| context_menu::Item {
            glyph: glyphs::named(&m.glyph.get()),
            label: m.title.get(),
            destructive: m.destructive.get(),
            message: m.tap,
        })
        .collect();
    match items.is_empty() {
        true => b.into(),
        false => context_menu::context_menu(b, r.title.get(), items),
    }
}

fn field<M: Clone + 'static>(label: &str, value: String, problem: Option<String>, typed: Input<String, M>) -> Element<'static, M> {
    let mut c = column![
        text(label.to_string()).size(12).style(style::dim),
        text_input(label, &value).on_input(move |s| typed.call(s)).style(style::field)
    ]
    .spacing(4);
    if let Some(p) = problem {
        c = c.push(text(p).size(12).style(|t| text::Style {
            color: Some(crate::theme::of(t).danger.base.color),
            ..Default::default()
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
            ..Default::default()
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
