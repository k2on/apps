//! A view laid out once in Rust (`ark_app::view`, ui/docs/app.md), drawn
//! by iced in ArkUI's parts: what `#[view]`'s `iced(&record)` entry point
//! returns. The desktop's measures are the desktop's (ui/docs/parts.md):
//! a card rounded by 8, rows lit by the theme's pressed colour, no give.
//!
//! What iced has no twin for is said here, not left out silently: a row's
//! menu is ArkUI's own menu at the pointer ([`crate::context_menu`]:
//! right click, a long press, or the menu key), a sheet or a prompt is a
//! dialog over the page, a picker is a row of chips, a search picker a
//! dropdown under its field ([`crate::search_picker`]), and an amount is typed.

use crate::text::{self, text};
use crate::text_input::text_input;
use ark_app::shell::compact_tag;
use ark_app::view::{
    ColumnAlign, ColumnWidth, Confirm, Input, ListRow, MeterRow, Part, Picture, Placement, Section, TableCell, TableColumn, TableRow, Val, View,
};
use iced::widget::{button, checkbox, column, container, row, rule, scrollable, stack, tooltip, Column, Space};
use iced::{Alignment, Length, Padding};

use crate::layout_tokens::{CONTENT_WIDTH, INSPECTOR_WIDTH, SEARCH_WIDTH, SIDEBAR_MIN_WIDTH, SIDEBAR_WIDTH, TOOLBAR_HEIGHT};
use crate::shell::{Frame, Pane, SizeClass};
use crate::{context_menu, glyphs, icon, layer, parts, sidebar, style};

pub use iced::Element;

mod layout;

/// What a page is padded by.
const PAD: f32 = 18.0;
/// A dialog's width.
const DIALOG: f32 = 420.0;
/// Above the sidebar's first place and below its last.
const SIDEBAR_PAD: f32 = 10.0;
/// A toolbar button's glyph, and the square it is pressed in.
const BAR_GLYPH: f32 = 17.0;
const BAR_BUTTON: f32 = 30.0;
/// A large title's size, under the toolbar short of a sidebar.
const LARGE_TITLE: f32 = 28.0;

/// `view` as iced draws it.
pub fn element<M: Clone + 'static>(view: View<M>) -> Element<'static, M> {
    match view {
        View::Part(Part::Page { title, top, content, .. }) => page(title, View::List(vec![*top, *content])),
        v => column(nodes(v)).spacing(8).into(),
    }
}

fn page<M: Clone + 'static>(title: Val<String>, content: View<M>) -> Element<'static, M> {
    let mut body = vec![];
    let mut over = vec![];
    // A table takes the window's width; cards and forms a readable column.
    let wide = flatten_ref(&content).iter().any(|v| matches!(v, View::Part(Part::Table { .. })));
    for v in flatten(content) {
        match v {
            View::Part(Part::Sheet { .. } | Part::Prompt { .. } | Part::Confirm { .. } | Part::Alert { .. }) => over.push(v),
            v => body.extend(nodes(v)),
        }
    }
    // Its title is the toolbar's (`shell`), inline or large: not said twice.
    let _ = title;
    let list = column(body).spacing(10).padding(PAD).max_width(if wide { f32::INFINITY } else { 720.0 });
    let mut layers = stack![scrollable(container(list).center_x(Length::Fill)).height(Length::Fill)];
    for v in over {
        let asked = match v {
            View::Part(p) => notice(p),
            v => dialog(v),
        };
        if let Some((dismiss, dialog)) = asked {
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
    let p = match v {
        View::Part(p) => p,
        View::Layout(l) => return layout::layout(*l, None),
        // A recording's `for` and `if` are the emitters'; a real view has none.
        _ => return None,
    };
    Some(match p {
        Part::Hero { label, cents, note } => column![
            text(label).size(13).style(style::faint(0.55)),
            amount(cents.get(), 34, false),
            text(note).size(12).style(style::dim),
        ]
        .spacing(2)
        .into(),
        Part::Summary {
            label,
            cents,
            first,
            first_cents,
            second,
            second_cents,
            trend,
        } => {
            let (a, b) = (first_cents.get(), second_cents.get());
            let facts = row![
                column![text(first).size(11).style(style::dim), amount(a, 13, false)].spacing(1),
                column![text(second).size(11).style(style::dim), amount(b, 13, false)].spacing(1),
            ]
            .spacing(18);
            container(
                column![
                    text(label).size(12).style(style::dim),
                    amount_bold(cents.get(), crate::figures::HEADLINE),
                    row![
                        facts,
                        Space::new().width(Length::Fill),
                        container(crate::figures::sparkline(&trend.get(), 34.0)).width(160)
                    ]
                    .align_y(Alignment::End),
                ]
                .spacing(4),
            )
            .padding([12, 14])
            .width(Length::Fill)
            .style(style::card)
            .into()
        }
        Part::Tile { glyph, tint } => {
            let tint = crate::parts::Tint::named(&tint.get());
            parts::tile(shape(&glyph.get()), move |t| tint.color(t), 36.0)
        }
        Part::StatusBadge { figure, tint } => crate::figures::status_badge(figure.get(), crate::parts::Tint::named(&tint.get())),
        Part::Meter { fraction, tint } => {
            let tint = match crate::parts::Tint::named(&tint.get()) {
                crate::parts::Tint::Neutral => crate::parts::Tint::Positive,
                t => t,
            };
            crate::parts::progress_bar(fraction.get() as f32, tint, crate::figures::GOAL_GIRTH)
        }
        Part::Detail { content } => scrollable(column(nodes(*content)).spacing(14).padding([12, 14]))
            .height(Length::Fill)
            .into(),
        Part::Section(s) => section(s),
        Part::Row(r) => list_row(*r),
        Part::MeterRow(r) => meter_row(*r),
        Part::Headline(h) => {
            let ark_app::view::Headline {
                label,
                figure,
                tint,
                caption,
                action,
            } = *h;
            let act = action.map(|(title, glyph, tap)| crate::figures::capsule_button(glyphs::named(glyph), title, true, tap));
            crate::figures::headline(&label.get(), figure.get(), parts::Tint::named(&tint.get()), &caption.get(), act)
        }
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
        Part::Choice { .. } | Part::SearchChoice { .. } => return None,
        Part::SearchPicker {
            label,
            selected,
            unset,
            prompt,
            pick,
            choices,
        } => crate::search_picker::search_picker(label, selected.get(), unset.get(), prompt, ark_app::view::search::choices(&choices), pick),
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
        // A first run's page: centred, no wider than a dialog; its mark on
        // a tile in the accent, the title large, the sentence dim.
        Part::FirstRunPage {
            glyph,
            title,
            sentence,
            content,
        } => {
            let head = column![
                parts::tile(shape(&glyph.get()), |t| crate::theme::of(t).primary.base.color, 72.0),
                text(title.get()).size(LARGE_TITLE).font(crate::hero::BOLD),
                text(sentence.get()).size(14).style(style::dim).align_x(Alignment::Center),
            ]
            .spacing(8)
            .align_x(Alignment::Center);
            let body = column![head, Space::new().height(18)]
                .extend(nodes(*content))
                .spacing(10)
                .align_x(Alignment::Center)
                .max_width(DIALOG);
            scrollable(container(body).width(Length::Fill).center_x(Length::Fill).padding([48.0, PAD]))
                .height(Length::Fill)
                .into()
        }
        Part::AddressField {
            label,
            prompt,
            text: typed,
            note,
            problem,
            enabled,
            typed: on,
            submit,
        } => {
            let mut input = text_input(&prompt.get(), &typed.get()).style(style::field);
            if enabled.get() {
                input = input.on_input(move |s| on.call(s)).on_submit(submit);
            }
            let under: Element<'static, M> = match problem.get() {
                Some(p) => text(p)
                    .size(12)
                    .style(|t| iced::widget::text::Style {
                        color: Some(crate::theme::of(t).danger.base.color),
                        ..Default::default()
                    })
                    .into(),
                None => text(note.get()).size(12).style(style::dim).into(),
            };
            column![text(label).size(12).style(style::dim), input, under]
                .spacing(4)
                .width(Length::Fill)
                .into()
        }
        Part::WideButton {
            title,
            prominent,
            enabled,
            busy,
            tap,
        } => {
            let busy = busy.get();
            let label = row![]
                .push_maybe(busy.then(|| icon::plain(shape("refresh-cw"), true)))
                .push(text(title.get()))
                .spacing(6)
                .align_y(Alignment::Center);
            button(container(label).center_x(Length::Fill))
                .width(Length::Fill)
                .padding([8, 14])
                .style(if prominent { style::action } else { style::quiet })
                .on_press_maybe((enabled.get() && !busy).then_some(tap))
                .into()
        }
        Part::StatusLine { text: said, busy, problem } => {
            let mut c = column![].spacing(4).align_x(Alignment::Center);
            if let Some(t) = said.get() {
                let busy = busy.get();
                c = c.push(
                    row![]
                        .push_maybe(busy.then(|| icon::plain(shape("refresh-cw"), true)))
                        .push(text(t).size(13).style(style::dim))
                        .spacing(6),
                );
            }
            if let Some(p) = problem.get() {
                c = c.push(text(p).size(13).style(|t| iced::widget::text::Style {
                    color: Some(crate::theme::of(t).danger.base.color),
                    ..Default::default()
                }));
            }
            c.into()
        }
        Part::CodeImage { size, dark } => crate::qr::qr_code(size.get().max(0) as usize, dark.get(), 220.0),
        Part::Photo { url, label, zoom } => {
            let url = url.get();
            match crate::images::local(&url) {
                // A window zooms by its own size: the photo fills what it is given.
                Some(h) => iced::widget::image(h)
                    .content_fit(iced::ContentFit::Contain)
                    .width(Length::Fill)
                    .height(if zoom { Length::Fill } else { Length::Shrink })
                    .into(),
                None => container(
                    row![]
                        .push_maybe(url.is_empty().then(|| icon::plain(shape("refresh-cw"), true)))
                        .push(text(label.get()).size(13).style(style::dim))
                        .spacing(6),
                )
                .center_x(Length::Fill)
                .padding(PAD)
                .into(),
            }
        }
        Part::Paper {
            heading,
            date,
            names,
            figures,
            notes,
            total,
        } => paper(heading.get(), date.get(), names.get(), figures.get(), notes.get(), total.get()),
        Part::Button { title, enabled, tap } => button(text(title)).on_press_maybe(enabled.get().then_some(tap)).into(),
        Part::DateField { label, day, typed } => column![
            text(label).size(12).style(style::dim),
            text_input("2026-11-11", &day.get())
                .on_input(move |s| typed.call(s))
                .style(style::field)
                .padding([6, 8]),
        ]
        .spacing(4)
        .into(),
        Part::Table { columns, rows } => data_table(columns, *rows),
        Part::BarChart { labels, values, said } => crate::figures::labelled_bars(&bars(labels.get(), values.get(), said.get())),
        // A table's rows are its own (`data_table`): alone, as a list's.
        Part::TableRow(r) => table_row(&[], *r, 0),
        Part::TableHeading { title } => crate::table::section(title.get(), 0.0),
        Part::BarItem {
            title,
            glyph,
            enabled,
            tap,
            placement,
        } => bar_button(title, glyph, placement, enabled.get(), tap),
        Part::Writing { markdown } => crate::writing::markdown(&markdown.get()),
        // The shell's are drawn by `shell`, with the window's frame; a
        // hosted screen and the dialogs need it too.
        Part::Page { .. }
        | Part::Sheet { .. }
        | Part::Prompt { .. }
        | Part::Tabs { .. }
        | Part::SectionTab { .. }
        | Part::TabGroup { .. }
        | Part::Push { .. }
        | Part::Split { .. }
        | Part::Toolbar { .. }
        | Part::Account { .. }
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

/// A section as the shell draws it: its title, glyph, tag, count, the
/// heading of the group it is in, what it shows.
struct Tab<M> {
    title: String,
    glyph: String,
    tag: String,
    badge: Option<i64>,
    group: Option<&'static str>,
    content: View<M>,
}

/// The sections of a `Tabs`, its groups' opened out under their headings.
fn tabs_of<M>(v: View<M>, group: Option<&'static str>, out: &mut Vec<Tab<M>>) {
    for v in flatten(v) {
        match v {
            View::Part(Part::SectionTab {
                title,
                glyph,
                tag,
                badge,
                content,
            }) => out.push(Tab {
                title: title.get(),
                glyph: glyph.get(),
                tag: tag.get(),
                badge: badge.and_then(|b| b.get()),
                group,
                content: *content,
            }),
            View::Part(Part::TabGroup { title, tabs, .. }) => tabs_of(*tabs, Some(title), out),
            _ => {}
        }
    }
}

/// What a part drawn inside the shell is handed from the parts around it:
/// the window, a pushed detail's way back, and the sections' bar while it
/// waits for the first toolbar to sit in (a regular width short of a
/// sidebar: the iPad upright's tab bar at the top).
struct Ctx<'a, 'h, M> {
    frame: Frame,
    back: Option<M>,
    top_tabs: Option<Element<'a, M>>,
    /// Who is signed in (`Part::Account`): every toolbar's last item.
    account: Option<(String, M)>,
    hosts: &'h mut dyn FnMut(&'static str) -> Element<'a, M>,
}

/// A shell's view as iced draws it, a standard Mac window's way: the
/// sections a sidebar on a wide window (groups under their headings, the
/// footer at its foot), a tab bar at the top of a narrower regular one and
/// at the foot of a compact one; a toolbar row — what goes back and the
/// title at its leading end, the actions, the inspector's toggle and the
/// search field at its trailing end — with a large title under it short of
/// a sidebar; the inspector a pane at the trailing edge (a sheet when
/// compact); a question or a notice over everything. `frame` is the
/// window's width and which pane has the keys (`crate::shell`); each
/// hosted screen is what `hosts` draws for its name.
pub fn shell<'a, M: Clone + 'static>(view: View<M>, frame: Frame, hosts: &mut dyn FnMut(&'static str) -> Element<'a, M>) -> Element<'a, M> {
    let mut ctx = Ctx {
        frame,
        back: None,
        top_tabs: None,
        account: None,
        hosts,
    };
    draw(view, &mut ctx)
}

fn draw<'a, M: Clone + 'static>(v: View<M>, ctx: &mut Ctx<'a, '_, M>) -> Element<'a, M> {
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
            p => body.push(shell_part(p, ctx)),
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

/// How the sections are shown at a width.
#[derive(Clone, Copy, PartialEq)]
enum Sections {
    Sidebar,
    TopBar,
    BottomBar,
}

fn sections(frame: Frame) -> Sections {
    match frame.class() {
        SizeClass::Compact => Sections::BottomBar,
        _ if frame.width >= SIDEBAR_MIN_WIDTH => Sections::Sidebar,
        _ => Sections::TopBar,
    }
}

fn shell_part<'a, M: Clone + 'static>(p: Part<M>, ctx: &mut Ctx<'a, '_, M>) -> Element<'a, M> {
    let frame = ctx.frame;
    match p {
        Part::Tabs {
            selected,
            select,
            tabs,
            footer,
            account,
        } => {
            if let View::Part(Part::Account { name, tap, .. }) = *account {
                ctx.account = Some((name.get(), tap));
            }
            let chosen = selected.get();
            let mut all = vec![];
            tabs_of(*tabs, None, &mut all);
            let how = sections(frame);
            // Short of a sidebar a child is shown in its parent's tab, which
            // pushes it (`ark_app::shell::compact_tag`).
            let shown_tag = match how {
                Sections::Sidebar => chosen.as_str(),
                _ => compact_tag(&chosen),
            };
            let at = all.iter().position(|t| t.tag == shown_tag).unwrap_or(0);
            match how {
                Sections::Sidebar => {
                    let focused = frame.focus == Pane::Sidebar;
                    let mut items = vec![];
                    let mut content = View::Empty;
                    for (i, t) in all.into_iter().enumerate() {
                        if i == at {
                            content = t.content;
                        }
                        items.push(sidebar::Item {
                            glyph: shape(&t.glyph),
                            label: t.title,
                            heading: t.group,
                            trailing: t.badge.map(|n| sidebar::count(n, sidebar::lit(i, at, focused))),
                            press: select.call(t.tag),
                        });
                    }
                    let foot = match *footer {
                        View::Empty => None,
                        f => Some(draw(f, ctx)),
                    };
                    let side = sidebar::view_with_footer(items, at, focused, SIDEBAR_WIDTH, SIDEBAR_PAD, "ark-shell-sidebar", foot);
                    let shown = draw(content, ctx);
                    row![side, rule::vertical(1).style(style::edge), container(shown).width(Length::Fill)]
                        .height(Length::Fill)
                        .into()
                }
                Sections::TopBar | Sections::BottomBar => {
                    // The tab bar has the sections alone: a group's are the
                    // sidebar's (`TabSection` hidden from the tab bar).
                    let mut heads = vec![];
                    let mut content = View::Empty;
                    for (i, t) in all.into_iter().enumerate() {
                        if i == at {
                            content = t.content;
                        }
                        if t.group.is_none() {
                            heads.push((t.title, t.glyph, t.tag.clone(), t.badge, t.tag == shown_tag));
                        }
                    }
                    if how == Sections::TopBar {
                        let bar = row(heads
                            .into_iter()
                            .map(|(title, _, tag, badge, on)| top_tab(title, badge, on, select.call(tag))))
                        .spacing(2)
                        .align_y(Alignment::Center);
                        ctx.top_tabs = Some(container(bar).padding(3).style(style::capsule_group).into());
                        return draw(content, ctx);
                    }
                    let bar = row(heads
                        .into_iter()
                        .map(|(title, glyph, tag, badge, on)| bottom_tab(title, glyph, badge, on, select.call(tag))));
                    let shown = draw(content, ctx);
                    column![
                        container(shown).height(Length::Fill),
                        rule::horizontal(1).style(style::edge),
                        container(bar).padding([2, 8]).style(style::surface)
                    ]
                    .into()
                }
            }
        }
        Part::SectionTab { content, .. } | Part::SettingsPane { content, .. } => draw(*content, ctx),
        Part::TabGroup { tabs, .. } => draw(*tabs, ctx),
        Part::Split {
            content,
            detail,
            showing,
            back,
        } => match (frame.class(), showing.get()) {
            (SizeClass::Expanded, _) => row![
                container(draw(*content, ctx)).width(CONTENT_WIDTH).height(Length::Fill),
                rule::vertical(1).style(style::edge),
                container(draw(*detail, ctx)).width(Length::Fill).height(Length::Fill),
            ]
            .into(),
            (_, true) => pushed(*detail, back, ctx),
            (_, false) => draw(*content, ctx),
        },
        Part::Push {
            content,
            detail,
            showing,
            back,
        } => match showing.get() {
            true => pushed(*detail, back, ctx),
            false => draw(*content, ctx),
        },
        Part::Toolbar { title, items, content } => toolbar(title.get(), *items, *content, ctx),
        Part::Searchable {
            prompt,
            text: t,
            typed,
            content,
        } => column![container(search(prompt, t.get(), typed)).padding([6, 12]), draw(*content, ctx)].into(),
        Part::Inspector {
            title,
            shown,
            toggle,
            content,
            inspector,
        } => {
            let content = draw(*content, ctx);
            if !shown.get() {
                return content;
            }
            match frame.class() {
                SizeClass::Compact => {
                    let close = toggle.call(false);
                    let pane = column![
                        row![
                            text(title).size(15).font(crate::hero::BOLD),
                            Space::new().width(Length::Fill),
                            bar_button("Done", "x", Placement::Primary, true, close.clone())
                        ]
                        .align_y(Alignment::Center)
                        .padding([8, 12]),
                        container(draw(*inspector, ctx)).height(Length::Fill)
                    ];
                    stack![
                        content,
                        layer::dimmed(close.clone(), close),
                        column![
                            Space::new().height(Length::FillPortion(1)),
                            container(pane).height(Length::FillPortion(3)).style(style::sheet)
                        ]
                    ]
                    .into()
                }
                _ => row![
                    container(content).width(Length::Fill),
                    rule::vertical(1).style(style::edge),
                    container(draw(*inspector, ctx))
                        .width(INSPECTOR_WIDTH)
                        .height(Length::Fill)
                        .style(style::surface)
                ]
                .into(),
            }
        }
        Part::Hosted { name } => (ctx.hosts)(name),
        Part::SettingsPanes { selected, select, panes } => {
            // A Mac's Settings window: its panes' glyphs and titles across
            // the top, the chosen one lit, and only its content under them.
            let chosen = selected.get();
            let mut heads = vec![];
            let mut shown = View::Empty;
            for (i, v) in flatten(*panes).into_iter().enumerate() {
                if let View::Part(Part::SettingsPane { title, glyph, content }) = v {
                    let on = title == chosen || (chosen.is_empty() && i == 0);
                    if on {
                        shown = *content;
                    }
                    heads.push(pane_tab(title, glyph, on, select.call(title.to_string())));
                }
            }
            // A title is never broken: a window too narrow for every tab
            // scrolls them sideways.
            let heads = scrollable(row(heads).spacing(4).align_y(Alignment::Center)).direction(scrollable::Direction::Horizontal(Default::default()));
            column![
                container(heads).center_x(Length::Fill).padding([6, 12]),
                rule::horizontal(1).style(style::edge),
                container(draw(shown, ctx)).height(Length::Fill)
            ]
            .into()
        }
        Part::Page { title, top, content, .. } => page(title, View::List(vec![*top, *content])),
        p => part(View::Part(p)).unwrap_or_else(|| Space::new().into()),
    }
}

/// A detail pushed onto its section: its own toolbar with the way back at
/// its leading end, or a Back row over it when it has none.
fn pushed<'a, M: Clone + 'static>(detail: View<M>, back: M, ctx: &mut Ctx<'a, '_, M>) -> Element<'a, M> {
    let has_bar = flatten_ref(&detail).iter().any(|v| matches!(v, View::Part(Part::Toolbar { .. })));
    if has_bar {
        ctx.back = Some(back);
        return draw(detail, ctx);
    }
    column![
        container(bar_button("Back", "chevron-left", Placement::Navigation, true, back)).padding([4, 8]),
        container(draw(detail, ctx)).height(Length::Fill),
    ]
    .into()
}

/// The parts lying one after another in `v`, without taking it apart.
fn flatten_ref<M>(v: &View<M>) -> Vec<&View<M>> {
    match v {
        View::List(vs) => vs.iter().flat_map(flatten_ref).collect(),
        View::Empty => vec![],
        v => vec![v],
    }
}

/// A section's toolbar over its content: the way back and the navigation
/// items, then the title (inline beside a sidebar, large under the row
/// short of one); the principal items in the middle (or the sections' tab
/// bar, short of a sidebar); the actions, the inspector's toggle and the
/// search field at the trailing end (the search under the title when
/// compact, as the iPhone's is).
fn toolbar<'a, M: Clone + 'static>(title: String, items: View<M>, content: View<M>, ctx: &mut Ctx<'a, '_, M>) -> Element<'a, M> {
    // What the section pushed takes the bar: its own title, its own items,
    // the way back at its leading end.
    let content = match content {
        View::Part(Part::Push {
            detail,
            showing,
            back,
            content,
        }) => match showing.get() {
            true => return pushed(*detail, back, ctx),
            false => *content,
        },
        c => c,
    };
    let frame = ctx.frame;
    let how = sections(frame);
    let (search, toggle, content) = peel(content);
    let mut lead = row![].spacing(2).align_y(Alignment::Center);
    if let Some(back) = ctx.back.take() {
        lead = lead.push(bar_button("Back", "chevron-left", Placement::Navigation, true, back));
    }
    let mut principal = vec![];
    let mut trailing = vec![];
    for v in flatten(items) {
        if let View::Part(Part::BarItem {
            title,
            glyph,
            placement,
            enabled,
            tap,
        }) = v
        {
            let b = bar_button(title, glyph, placement, enabled.get(), tap);
            match placement {
                Placement::Navigation => lead = lead.push(b),
                Placement::Principal => principal.push(b),
                Placement::Primary | Placement::Secondary => trailing.push(b),
            }
        }
    }
    if let Some((title, shown, tap)) = toggle {
        trailing.push(toggle_button(title, shown, tap));
    }
    if let Some((name, tap)) = ctx.account.clone() {
        trailing.push(account_button(name, tap));
    }
    let inline = how == Sections::Sidebar;
    let mut line = row![lead].spacing(8).align_y(Alignment::Center);
    if inline {
        line = line.push(text(title.clone()).size(15).font(crate::hero::BOLD).wrapping(text::Wrapping::None));
    }
    line = line.push(Space::new().width(Length::Fill));
    match ctx.top_tabs.take() {
        Some(tabs) => line = line.push(tabs),
        None => line = line.extend(principal),
    }
    line = line.push(Space::new().width(Length::Fill));
    line = line.push(row(trailing).spacing(2).align_y(Alignment::Center));
    let (in_bar, under) = match how {
        Sections::BottomBar => (None, search),
        _ => (search, None),
    };
    if let Some(s) = in_bar {
        let wide = match how {
            Sections::Sidebar => SEARCH_WIDTH,
            _ => SEARCH_WIDTH * 0.7,
        };
        line = line.push(container(s).width(wide));
    }
    let mut out = column![container(line).height(TOOLBAR_HEIGHT).padding([0, 12]).center_y(TOOLBAR_HEIGHT)];
    if inline {
        out = out.push(rule::horizontal(1).style(style::edge));
    } else {
        out = out.push(container(text(title).size(LARGE_TITLE).font(crate::hero::BOLD)).padding(Padding {
            top: 2.0,
            right: PAD,
            bottom: 6.0,
            left: PAD,
        }));
    }
    if let Some(s) = under {
        out = out.push(container(s).padding(Padding {
            top: 0.0,
            right: PAD,
            bottom: 8.0,
            left: PAD,
        }));
    }
    out.push(container(draw(content, ctx)).height(Length::Fill)).into()
}

/// The search field and the inspector's toggle a toolbar takes from what it
/// is over (`.searchable` and `.inspector` put theirs in the toolbar), and
/// what is left to draw: the inspector stays, its search taken out.
#[allow(clippy::type_complexity)]
fn peel<'a, M: Clone + 'static>(v: View<M>) -> (Option<Element<'a, M>>, Option<(&'static str, bool, M)>, View<M>) {
    match v {
        View::Part(Part::Searchable {
            prompt,
            text: t,
            typed,
            content,
        }) => {
            let (_, toggle, rest) = peel(*content);
            (Some(search(prompt, t.get(), typed)), toggle, rest)
        }
        View::Part(Part::Inspector {
            title,
            shown,
            toggle,
            content,
            inspector,
        }) => {
            let (search, _, rest) = peel(*content);
            let on = shown.get();
            let tap = toggle.call(!on);
            let kept = Part::Inspector {
                title,
                shown,
                toggle,
                content: Box::new(rest),
                inspector,
            };
            (search, Some((title, on, tap)), View::Part(kept))
        }
        v => (None, None, v),
    }
}

/// A toolbar's button: its glyph alone, as a Mac toolbar draws one, its
/// title said when the pointer rests on it.
fn bar_button<'a, M: Clone + 'a>(title: &'static str, glyph: &'static str, _placement: Placement, enabled: bool, tap: M) -> Element<'a, M> {
    let b = button(
        container(icon::tinted(shape(glyph), BAR_GLYPH, move |p| match enabled {
            true => p.background.base.text.scale_alpha(0.75),
            false => p.background.base.text.scale_alpha(0.3),
        }))
        .center(BAR_BUTTON),
    )
    .padding(0)
    .style(style::quiet)
    .on_press_maybe(enabled.then_some(tap));
    tip(b, title)
}

/// Who is signed in, the toolbar's last item: their initial on their
/// colour (the desktop's window draws their picture at the sidebar's foot,
/// from its own cache), opening their account; said as "Account" and their
/// name.
fn account_button<'a, M: Clone + 'a>(name: String, tap: M) -> Element<'a, M> {
    let who = crate::account::Who {
        name: name.clone(),
        id: name.clone(),
    };
    let b = button(container(crate::account::face(Some(&who), None, BAR_GLYPH + 4.0)).center(BAR_BUTTON))
        .padding(0)
        .style(style::quiet)
        .on_press(tap);
    tip(b, ark_app::shell::account_label(&name))
}

/// The inspector's toggle at the toolbar's trailing end: the trailing
/// pane's glyph, lit while it shows.
fn toggle_button<'a, M: Clone + 'a>(title: &'static str, shown: bool, tap: M) -> Element<'a, M> {
    let b = button(
        container(icon::tinted(glyphs::PANEL_RIGHT, BAR_GLYPH, move |p| match shown {
            true => p.primary.base.color,
            false => p.background.base.text.scale_alpha(0.75),
        }))
        .center(BAR_BUTTON),
    )
    .padding(0)
    .style(style::quiet)
    .on_press(tap);
    tip(b, title)
}

/// `content` with `words` said by it while the pointer rests on it.
fn tip<'a, M: 'a>(content: impl Into<Element<'a, M>>, words: impl Into<String>) -> Element<'a, M> {
    tooltip(
        content,
        container(text(words.into()).size(12)).padding([3, 8]).style(style::card),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

/// A pane of Settings at its window's top: its glyph over its title, lit
/// when shown, as a Mac's Settings toolbar draws its tabs.
fn pane_tab<'a, M: Clone + 'a>(title: &'static str, glyph: &'static str, on: bool, press: M) -> Element<'a, M> {
    let tint = move |p: &iced::theme::palette::Extended| match on {
        true => p.primary.base.color,
        false => p.background.base.text.scale_alpha(0.65),
    };
    let label = column![
        icon::tinted(shape(glyph), 22.0, tint),
        text(title).size(11).wrapping(text::Wrapping::None).style(move |t| text::Style {
            color: Some(tint(crate::theme::of(t))),
            ..Default::default()
        })
    ]
    .spacing(3)
    .align_x(Alignment::Center);
    button(label)
        .padding([4, 12])
        .style(move |t, s| style::capsule(t, s, on))
        .on_press(press)
        .into()
}

/// A section in the tab bar at the top: its title in a capsule, lit when
/// shown, its count a dot of the error's colour.
fn top_tab<'a, M: Clone + 'a>(title: String, badge: Option<i64>, on: bool, press: M) -> Element<'a, M> {
    let mut label = row![text(title).size(13).style(move |t| text::Style {
        color: Some(match on {
            true => crate::theme::of(t).primary.base.color,
            false => crate::theme::of(t).background.base.text,
        }),
        ..Default::default()
    })]
    .spacing(4)
    .align_y(Alignment::Center);
    if let Some(n) = badge {
        label = label.push(parts::count_badge(n));
    }
    button(label)
        .padding([5, 12])
        .style(move |t, s| style::capsule(t, s, on))
        .on_press(press)
        .into()
}

/// A section in the tab bar at the foot: its glyph over its title, lit
/// when shown, its count on the glyph's corner.
fn bottom_tab<'a, M: Clone + 'a>(title: String, glyph: String, badge: Option<i64>, on: bool, press: M) -> Element<'a, M> {
    let tint = move |p: &iced::theme::palette::Extended| match on {
        true => p.primary.base.color,
        false => p.background.base.text.scale_alpha(0.6),
    };
    let mut mark = row![icon::tinted(shape(&glyph), BAR_GLYPH, tint)].spacing(2);
    if let Some(n) = badge {
        mark = mark.push(parts::count_badge(n));
    }
    let label = column![
        mark,
        text(title).size(10).style(move |t| text::Style {
            color: Some(tint(crate::theme::of(t))),
            ..Default::default()
        })
    ]
    .spacing(2)
    .align_x(Alignment::Center);
    button(container(label).center_x(Length::Fill))
        .width(Length::Fill)
        .padding([6, 0])
        .style(style::bare)
        .on_press(press)
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
            // A sheet over this one (a receipt typed over its transaction), or
            // a question it asks: a window draws the top one alone, over the
            // page, and this one again once it is put away.
            let mut parts = flatten(*content);
            if let Some(i) = parts.iter().position(|v| {
                matches!(
                    v,
                    View::Part(Part::Sheet { .. } | Part::Prompt { .. } | Part::Confirm { .. } | Part::Alert { .. })
                )
            }) {
                let View::Part(over) = parts.swap_remove(i) else { return None };
                return notice(over);
            }
            let mut c = column![text(title).size(18).font(crate::hero::BOLD)].extend(parts.into_iter().flat_map(nodes));
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
    let note = s.note.as_ref().and_then(|n| n.get());
    let total: Option<Element<'static, M>> = match note {
        Some(words) => Some(
            text(words)
                .size(13)
                .font(crate::hero::BOLD)
                .style(|t| text::Style {
                    color: Some(parts::Tint::Caution.color(t)),
                    ..Default::default()
                })
                .into(),
        ),
        None => s.amount.filter(|_| shows).map(|a| {
            let cents = a.get();
            amount(cents, 13, cents >= 0)
        }),
    };
    // The figures in their columns, each under its title, the total last.
    let trailing: Option<Element<'static, M>> = match (s.columns, total) {
        (Some(k), total) => {
            let cell = |title: &'static str, figure: Element<'static, M>| -> Element<'static, M> {
                column![text(title).size(10).style(style::faint(0.45)), figure]
                    .align_x(Alignment::End)
                    .width(METER_COLUMN)
                    .into()
            };
            let mut line = row![
                cell(k.titles[0], amount(k.first.get(), 13, true)),
                cell(k.titles[1], amount(k.second.get(), 13, true)),
            ]
            .spacing(COLUMN_GAP)
            .align_y(Alignment::End);
            if let Some(t) = total {
                line = line.push(cell(k.titles[2], t));
            }
            Some(line.into())
        }
        (None, total) => total,
    };
    let open = s.open.as_ref().is_none_or(|(o, _)| o.get());
    let mut out = column![].spacing(0);
    if let Some(t) = s.title {
        let title = t.get();
        let mut head = parts::section_header(&title, trailing);
        if let Some((_, toggle)) = s.open.clone() {
            let chevron = icon::plain(if open { glyphs::DOWN } else { glyphs::CHEVRON }, true);
            head = button(row![chevron, head].spacing(4).align_y(Alignment::End))
                .padding(0)
                .width(Length::Fill)
                .style(style::bare)
                .on_press(toggle)
                .into();
        }
        let items = menu_of(s.menu);
        out = out.push(match items.is_empty() {
            true => head,
            false => context_menu::context_menu(head, title, items),
        });
    }
    if open {
        out = out.push(parts::group_card(nodes(*s.rows)));
    }
    if let Some(note) = s.footer.and_then(|f| f.get()) {
        out = out.push(container(text(note).size(12).style(style::dim)).padding([6, 4]));
    }
    out.into()
}

/// A column's width beside a row's name, and the gap between two.
const METER_COLUMN: f32 = 96.0;
const COLUMN_GAP: f32 = 12.0;

/// A menu's items as ArkUI's context menu offers them: those offered.
fn menu_of<M: Clone + 'static>(menu: Vec<ark_app::view::MenuItem<M>>) -> Vec<context_menu::Item<M>> {
    menu.into_iter()
        .filter(|m| m.offered())
        .map(|m| context_menu::Item {
            glyph: glyphs::named(&m.glyph.get()),
            label: m.title.get(),
            destructive: m.destructive.get(),
            message: m.tap,
        })
        .collect()
}

/// ArkUI's `MeterRow`: the mark and the title over the slim bar and its
/// words; the columns; the status badge at its end.
fn meter_row<M: Clone + 'static>(r: MeterRow<M>) -> Element<'static, M> {
    let faded = r.faded.as_ref().is_some_and(|f| f.get());
    let mut name = row![].spacing(8).align_y(Alignment::Center);
    if let Some(kind) = r.mark.as_ref().and_then(|m| m.get()) {
        let glyph = shape(if kind == "credit" { "credit-card" } else { &kind });
        name = name.push(parts::tile(glyph, |t| crate::theme::of(t).primary.base.color, 20.0));
    }
    name = name.push(text(r.title.get()).size(14).style(move |t| text::Style {
        color: faded.then(|| style::dim(t).color.unwrap_or_default()),
        ..Default::default()
    }));
    let mut words = column![name].spacing(4).width(Length::Fill);
    if let (Some(f), Some(t)) = (r.fraction.as_ref().and_then(|f| f.get()), r.meter_tint.as_ref()) {
        words = words.push(crate::figures::goal_bar(f as f32, parts::Tint::named(&t.get())));
    }
    let note = r.note.as_ref().and_then(|n| n.get());
    let end = r.note_end.as_ref().and_then(|n| n.get());
    if note.is_some() || end.is_some() {
        let tint = parts::Tint::named(&r.note_tint.as_ref().map(|t| t.get()).unwrap_or_default());
        let mut line = row![].spacing(8);
        if let Some(n) = note {
            line = line.push(text(n).size(11).style(move |t| text::Style {
                color: Some(tint.color(t)),
                ..Default::default()
            }));
        }
        line = line.push(Space::new().width(Length::Fill));
        if let Some(e) = end {
            line = line.push(text(e).size(11).style(style::dim));
        }
        words = words.push(line);
    }
    let mut line = row![words].spacing(COLUMN_GAP).align_y(Alignment::Center);
    if let Some((a, b)) = &r.columns {
        line = line
            .push(container(amount(a.get(), 14, false)).width(METER_COLUMN).align_right(METER_COLUMN))
            .push(container(amount(b.get(), 14, true)).width(METER_COLUMN).align_right(METER_COLUMN));
    }
    let badge = crate::figures::status_badge(r.status.get(), parts::Tint::named(&r.status_tint.get()));
    line = line.push(container(badge).width(METER_COLUMN).align_right(METER_COLUMN));
    let b = button(line).width(Length::Fill).padding([2, 0]).style(style::bare).on_press_maybe(r.tap);
    let items = menu_of(r.menu);
    match items.is_empty() {
        true => b.into(),
        false => context_menu::context_menu(b, r.title.get(), items),
    }
}

fn list_row<M: Clone + 'static>(r: ListRow<M>) -> Element<'static, M> {
    let mut line = row![].spacing(10).align_y(Alignment::Center);
    match &r.picture {
        Picture::None => {}
        Picture::Tile(g) => line = line.push(parts::tile(shape(g), |t| style::dim(t).color.unwrap_or_default(), 28.0)),
        Picture::Mark { glyph, .. } => line = line.push(parts::tile(shape(&glyph.get()), |t| crate::theme::of(t).primary.base.color, 28.0)),
        Picture::Glyph(g) => line = line.push(icon::tinted(shape(g), 16.0, |p| p.primary.base.color)),
    }
    // An action's row is in the accent, as the phones draw it.
    let accent = r.accent;
    let title = text(r.title.get()).size(14).style(move |t| text::Style {
        color: accent.then(|| crate::theme::of(t).primary.base.color),
        ..Default::default()
    });
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
    if let Some(v) = &r.value {
        line = line.push(text(v.get()).size(14).style(style::dim));
    }
    if r.pushes {
        line = line.push(icon::plain(glyphs::CHEVRON, true));
    }
    // A row that does nothing is no button: a button without a press is
    // drawn disabled, faded.
    if r.tap.is_none() && r.menu.is_empty() {
        return container(line).width(Length::Fill).padding([2, 0]).into();
    }
    let b = button(line).width(Length::Fill).padding([2, 0]).style(style::bare).on_press_maybe(r.tap);
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

/// A headline amount, bold: red below nothing.
fn amount_bold<M: 'static>(cents: i64, size: f32) -> Element<'static, M> {
    text(ark_format::amount(cents, &ark_format::NumberStyle::us()))
        .size(size)
        .font(crate::hero::BOLD)
        .style(move |t| text::Style {
            color: Some(match cents < 0 {
                true => crate::theme::of(t).danger.base.color,
                false => crate::theme::of(t).background.base.text,
            }),
            ..Default::default()
        })
        .into()
}

/// An amount: red below nothing, or quiet.
fn amount<M: 'static>(cents: i64, size: u32, quiet: bool) -> Element<'static, M> {
    text(ark_format::amount(cents, &ark_format::NumberStyle::us()))
        .size(size)
        .wrapping(text::Wrapping::None)
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

// * A table

/// A column's width as iced lays it out.
fn column_width(c: &TableColumn) -> Length {
    match c.width {
        ColumnWidth::Figure => Length::Fixed(crate::layout_tokens::AMOUNT_COLUMN_WIDTH),
        ColumnWidth::Mark => Length::Fixed(crate::layout_tokens::MARK_COLUMN_WIDTH),
        ColumnWidth::Share(n) => Length::FillPortion(n.max(1)),
    }
}

/// A table: its headings in a row of their own, then its rows, each the
/// whole width, the cursor's lit (`arkui::table`).
fn data_table<M: Clone + 'static>(columns: Vec<TableColumn>, rows: View<M>) -> Element<'static, M> {
    let mut head = row![crate::table::gutter()];
    for c in &columns {
        head = head.push(crate::table::heading(c.heading, column_width(c)));
    }
    let mut list = Column::new().push(crate::table::row(head, false, false, false));
    let mut odd = 0;
    for v in flatten(rows) {
        match v {
            View::Part(Part::TableRow(r)) => {
                odd += 1;
                list = list.push(table_row(&columns, *r, odd));
            }
            View::Part(Part::TableHeading { title }) => {
                odd = 0;
                list = list.push(crate::table::section(title.get(), crate::table::GUTTER));
            }
            v => list = list.extend(nodes(v)),
        }
    }
    list.width(Length::Fill).into()
}

/// A cell's tint, from the palette: the ink, the quiet ink, or a signal.
fn cell_colour(p: &iced::theme::palette::Extended, tint: &str) -> iced::Color {
    match tint {
        "quiet" => p.background.base.text.scale_alpha(style::DIM),
        "positive" => p.success.base.color,
        "caution" => p.warning.base.color,
        "negative" => p.danger.base.color,
        "accent" => p.primary.base.color,
        _ => p.background.base.text,
    }
}

/// One cell's words in its column, in its tint.
fn table_cell<M: 'static>(c: TableCell, column: Option<&TableColumn>) -> Element<'static, M> {
    let width = column.map(column_width).unwrap_or(Length::Shrink);
    let align = column.map(|c| c.align).unwrap_or(ColumnAlign::Start);
    let mut line = row![].spacing(4).align_y(Alignment::Center);
    if !c.glyph.is_empty() {
        let tint = c.tint.clone();
        line = line.push(icon::tinted(shape(&c.glyph), 13.0, move |p| cell_colour(p, &tint)));
    }
    if !c.text.is_empty() {
        let tint = c.tint.clone();
        line = line.push(
            text(c.text)
                .size(crate::table::CELL_SIZE)
                .wrapping(text::Wrapping::None)
                .style(move |t| text::Style {
                    color: Some(cell_colour(crate::theme::of(t), &tint)),
                    ..Default::default()
                }),
        );
    }
    let x = match align {
        ColumnAlign::Start => Alignment::Start,
        ColumnAlign::End => Alignment::End,
        ColumnAlign::Center => Alignment::Center,
    };
    container(line)
        .width(width)
        .align_x(x)
        .padding(Padding {
            right: crate::table::SPACING,
            ..Padding::ZERO
        })
        .clip(true)
        .into()
}

/// A table's row: its mark in the gutter, a cell for each column; pressed,
/// its tap; right-clicked, its menu.
fn table_row<M: Clone + 'static>(columns: &[TableColumn], r: TableRow<M>, odd: usize) -> Element<'static, M> {
    let selected = r.selected.as_ref().is_some_and(|s| s.get());
    let marked = r.marked.as_ref().is_some_and(|m| m.get());
    let mut line = row![].align_y(Alignment::Center);
    line = line.push(match marked {
        true => container(text("\u{25cf}").size(8).style(|t| text::Style {
            color: Some(crate::theme::of(t).primary.base.color),
            ..Default::default()
        }))
        .center_x(Length::Fixed(crate::table::GUTTER))
        .into(),
        false => crate::table::gutter(),
    });
    let cells = r.table_cells();
    let title = cells.first().map(|c| c.text.clone()).unwrap_or_default();
    for (i, c) in cells.into_iter().enumerate() {
        line = line.push(table_cell(c, columns.get(i)));
    }
    let body = crate::table::row(line, selected, true, odd % 2 == 1);
    let b = button(body).width(Length::Fill).padding(0).style(style::bare).on_press_maybe(r.tap);
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
        false => context_menu::context_menu(b, title, items),
    }
}

/// A bar chart's three lists as its bars.
fn bars(labels: Vec<String>, values: Vec<i64>, said: Vec<String>) -> Vec<crate::figures::Bar> {
    labels
        .into_iter()
        .zip(values)
        .zip(said)
        .map(|((label, value), said)| crate::figures::Bar { label, value, said })
        .collect()
}

/// A slip of paper ([`Part::Paper`]): white whatever the theme, so drawn in
/// the light side's colours — the theme's text colour is the page's, light
/// on a dark one, which white paper would hide.
fn paper<M: 'static>(
    heading: String,
    date: String,
    names: Vec<String>,
    figures: Vec<String>,
    notes: Vec<String>,
    total: String,
) -> Element<'static, M> {
    use crate::tokens::light;
    let mono = |s: String, size: u32, c: iced::Color, bold: bool| {
        text(s)
            .font(iced::Font {
                weight: if bold { iced::font::Weight::Bold } else { iced::font::Weight::Normal },
                ..crate::fonts::MONO
            })
            .size(size)
            .style(move |_| iced::widget::text::Style {
                color: Some(c),
                ..Default::default()
            })
    };
    let rule = || mono("-".repeat(36), 12, light::QUIET, false);
    let mut slip: Column<'static, M> = column![
        container(mono(heading, 13, light::INK, true)).center_x(Length::Fill),
        container(mono(date, 12, light::INK, false)).center_x(Length::Fill),
        rule(),
    ]
    .spacing(3);
    for (i, name) in names.into_iter().enumerate() {
        let figure = figures.get(i).cloned().unwrap_or_default();
        slip = slip.push(row![mono(name, 12, light::INK, false).width(Length::Fill), mono(figure, 12, light::INK, false)].spacing(8));
        if let Some(n) = notes.get(i).filter(|n| !n.is_empty()) {
            slip = slip.push(mono(format!("  {n}"), 11, light::QUIET, false));
        }
    }
    slip = slip.push(rule()).push(row![
        mono("TOTAL".into(), 12, light::INK, true).width(Length::Fill),
        mono(total, 12, light::INK, true)
    ]);
    container(slip)
        .padding(14)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(light::SURFACE)),
            text_color: Some(light::INK),
            border: iced::Border {
                color: light::HAIRLINE,
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 0, 0, 0.18),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 6.0,
            },
            ..Default::default()
        })
        .into()
}
