//! What the explorer draws. Nothing here writes a colour down — every one is
//! asked of the host's palette through `arkui` — and nothing here decides
//! anything: what the cursor is on, what an edit goes through and what the
//! switch says are answered in [`crate::model`].

use arkui::{fit, style, table};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, column, container, mouse_area, row, rule, scrollable, text, text_input, Column, Row, Space};
use iced::{Alignment, Element, Length, Padding};

use crate::data::{Source, Writer};
use crate::form::{Field, Form};
use crate::model::{Called, Explorer, Grid, Msg, RowLight, Screen, PAGE_ROWS};
use crate::value::{ty_name, Ty};

const NAME: Length = Length::FillPortion(3);
const WIDE: Length = Length::FillPortion(6);
const NARROW: Length = Length::Fixed(64.0);
/// A form's label column.
const LABEL: f32 = 130.0;
/// A grid's row: a cell's whole height, which is also its click target;
/// one character of a cell, erring wide, in pixels.
const ROW_HEIGHT: f32 = 23.0;
const CHAR_PX: f32 = 7.5;

fn small<'a>(s: impl Into<String>) -> Element<'a, Msg> {
    text(s.into()).size(12).style(style::dim).into()
}

fn link<'a>(label: impl Into<String>, msg: Msg) -> Element<'a, Msg> {
    button(text(label.into()).size(12))
        .style(button::text)
        .padding([2, 6])
        .on_press(msg)
        .into()
}

fn plural(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

impl Explorer {
    /// The whole explorer: which screen, the screen, and the status line.
    pub fn view<'a>(&'a self, src: &Source<'_>, w: &dyn Writer) -> Element<'a, Msg> {
        let tab = |label: &'static str, lit: bool, to: Screen| {
            let t = text(label).size(13).style(if lit { style::accent } else { style::dim });
            button(t).style(button::text).padding([2, 6]).on_press(Msg::Show(to))
        };
        let s = &self.screen;
        let who = w.who();
        let mut head = row![
            text("explorer").size(16),
            tab("tables", matches!(s, Screen::Tables | Screen::Table(_)), Screen::Tables),
            tab("log", *s == Screen::Log, Screen::Log),
            tab("functions", matches!(s, Screen::Functions | Screen::Call(_)), Screen::Functions),
            container(small(if who.is_empty() { who } else { format!("writes as {who}") })).width(Length::Fill),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        // The switch to write raw, where the writer can: a client cannot,
        // and it is not drawn.
        if w.can_raw() {
            let label = text(if self.raw { "raw: on" } else { "raw: off" }).size(12);
            head = head.push(
                button(label)
                    .style(if self.raw { style::action } else { button::text })
                    .padding([2, 8])
                    .on_press(Msg::ToggleRaw),
            );
        }
        let body = match s {
            Screen::Tables => self.view_tables(src, w),
            Screen::Table(t) => self.view_table(src, w, t),
            Screen::Log => self.view_log(src),
            Screen::Functions => self.view_functions(src),
            Screen::Call(f) => self.view_call(src, w, f),
        };
        let status = row![
            text(self.note.clone()).size(12).style(style::dim).width(Length::Fill),
            text(self.keys.pending()).size(12).style(style::accent)
        ];
        container(column![head, rule::horizontal(1), body, status.spacing(12)].spacing(8))
            .padding(12)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// One line of a list, lit where the cursor is.
    fn line<'a>(&self, cells: Vec<(String, Length, bool)>, i: usize, on: bool) -> iced::widget::Container<'a, Msg> {
        let line = cells
            .into_iter()
            .fold(Row::new(), |r, (s, wd, dim)| r.push(table::cell(s, wd, on, false, dim)));
        table::row(line, on, self.has_keys, i % 2 == 1)
    }

    fn view_tables<'a>(&'a self, src: &Source<'_>, w: &dyn Writer) -> Element<'a, Msg> {
        let head = row![
            table::heading("Table", NAME),
            table::heading("Rows", NARROW),
            table::heading("Columns", NARROW),
            table::heading("Exposed", WIDE)
        ];
        let rows = Explorer::tables(src).into_iter().enumerate().fold(Column::new(), |col, (i, t)| {
            let exposed = match Explorer::verbs(w, &t) {
                Some(v) if v.may_author => v.names(&t).join(" "),
                Some(v) => format!("{} (not this host's to call)", v.names(&t).join(" ")),
                None if w.can_raw() => "nothing: raw writes".into(),
                None => "nothing".into(),
            };
            let n = (src.store.rows(&t).count().to_string(), Explorer::columns(src, &t).len().to_string());
            let line = self.line(
                vec![(t.clone(), NAME, false), (n.0, NARROW, true), (n.1, NARROW, true), (exposed, WIDE, true)],
                i,
                i == self.tables_at,
            );
            col.push(mouse_area(line).on_enter(Msg::At(i)).on_press(Msg::Open(t)))
        });
        column![head, scrollable(rows).style(style::bars).height(Length::Fill)].spacing(4).into()
    }

    fn view_table<'a>(&'a self, src: &Source<'_>, w: &dyn Writer, t: &str) -> Element<'a, Msg> {
        let all = src.store.rows(t).count();
        let (page, pages) = self.page(all);
        let from = page * PAGE_ROWS;
        // The editor is drawn on the row it edits, found by its key.
        let editing = self.editing.as_ref().filter(|e| e.table == t).and_then(|e| {
            let column = Explorer::columns(src, t).iter().position(|c| *c == e.column)?;
            Some((self.editing_at(src)?.checked_sub(from)?, column, e.text.clone()))
        });
        let (grid, lights) = self.lit_table(src, t);
        let cursor = (self.row_at.saturating_sub(from), self.column_at);
        let body = grid_view(grid, lights, cursor, self.has_keys, editing, Some(Msg::Sort), move |r, c| {
            Msg::Cell(from + r, c)
        });
        let said = format!(
            "{t}: rows {}\u{2013}{} of {all} \u{b7} page {} of {pages} \u{b7} {}",
            (from + 1).min(all),
            (from + PAGE_ROWS).min(all),
            page + 1,
            self.writes_through(w, t)
        );
        let foot = row![
            small(said),
            Space::new().width(Length::Fill),
            link("edit", Msg::Edit),
            link("delete row", Msg::Delete)
        ];
        column![body, detail_line(self.detail(src)), foot.spacing(8).align_y(Alignment::Center)]
            .spacing(4)
            .into()
    }

    fn view_log<'a>(&'a self, src: &Source<'_>) -> Element<'a, Msg> {
        let log = src.log;
        let conns = log.connections();
        let mut out = column![text(format!("head {}", log.head())).size(13), small(plural(conns.len(), "connection"))].spacing(4);
        for (i, c) in conns.iter().enumerate() {
            let pending = c.pending.map_or(String::new(), |n| format!(" \u{b7} {n} pending"));
            out = out.push(self.line(
                vec![
                    (c.who.clone(), NAME, false),
                    (format!("at {}{pending}", c.cursor), WIDE, true),
                    (c.note.clone(), WIDE, true),
                ],
                i,
                false,
            ));
        }
        let heads = [
            ("Seq", Length::Fixed(56.0)),
            ("Function", NAME),
            ("By", NAME),
            ("Standing", NAME),
            ("Facts", WIDE),
        ];
        out = out
            .push(rule::horizontal(1))
            .push(heads.into_iter().fold(Row::new(), |r, (h, wd)| r.push(table::heading(h, wd))));
        for (i, l) in log.lines().into_iter().enumerate() {
            let fact = |c: &ark::store::Change| match c {
                ark::store::Change::Add { table, .. } => format!("+{table}"),
                ark::store::Change::Remove { table, .. } => format!("\u{2212}{table}"),
                ark::store::Change::Edit { table, .. } => format!("~{table}"),
            };
            let facts = l
                .facts
                .as_ref()
                .map_or("\u{2014}".into(), |f| f.iter().map(fact).collect::<Vec<_>>().join(" "));
            let cells = vec![
                (l.seq.map_or("\u{2026}".into(), |n| n.to_string()), heads[0].1, true),
                (l.function.clone(), NAME, false),
                (format!("{} \u{b7} {}", l.entry.actor, fit::tail(&l.entry.session, 12)), NAME, true),
                (l.standing.clone(), NAME, true),
                (facts, WIDE, true),
            ];
            out = out.push(mouse_area(self.line(cells, i, i == self.log_at)).on_enter(Msg::At(i)));
        }
        scrollable(out).style(style::bars).height(Length::Fill).into()
    }

    fn view_functions<'a>(&'a self, src: &Source<'_>) -> Element<'a, Msg> {
        let all = crate::form::callables(src.module, src.lib);
        let mut list = Column::new();
        for (i, c) in all.iter().enumerate() {
            if i == 0 || all[i - 1].router != c.router {
                list = list.push(table::section(
                    if c.router.is_empty() { "(no router)".into() } else { c.router.clone() },
                    4.0,
                ));
            }
            let input: Vec<String> = c.input.iter().map(|(n, f)| format!("{n}: {}", ty_name(&f.ty))).collect();
            let input = if input.is_empty() { "no input".into() } else { input.join(", ") };
            let cells = vec![
                (c.name.clone(), NAME, false),
                (c.kind.name().into(), Length::Fixed(80.0), true),
                (input, Length::FillPortion(8), true),
            ];
            list = list.push(
                mouse_area(self.line(cells, i, i == self.functions_at))
                    .on_enter(Msg::At(i))
                    .on_press(Msg::Function(i)),
            );
        }
        if all.is_empty() {
            list = list.push(small("this module has no queries and no mutations"));
        }
        let head = row![
            table::heading("Function", NAME),
            table::heading("Kind", Length::Fixed(80.0)),
            table::heading("Input", Length::FillPortion(8))
        ];
        column![head, scrollable(list).style(style::bars).height(Length::Fill)].spacing(4).into()
    }

    fn view_call<'a>(&'a self, src: &Source<'_>, w: &dyn Writer, name: &str) -> Element<'a, Msg> {
        let Some(f) = self.forms.get(name) else {
            return small(format!("no form for {name}"));
        };
        let fields = if f.fields.is_empty() {
            "no input".into()
        } else {
            plural(f.fields.len(), "field")
        };
        let title = row![text(name.to_string()).size(15), small(format!("{} \u{b7} {fields}", f.kind.name()))].spacing(10);
        let keys_here = !self.in_answer && self.picker.is_none() && self.has_keys;
        let mut lines = Column::new().spacing(2);
        for (i, ((n, field), typed)) in f.fields.iter().zip(&f.values).enumerate() {
            lines = lines.push(self.view_field(f, i, n, field, typed, keys_here && i == f.at));
            if let Some(p) = self.picker.as_ref().filter(|p| p.field == i) {
                lines = lines.push(self.view_picker(src, &p.table, &p.filter, p.at));
            }
        }
        if let Some(why) = f.message("") {
            lines = lines.push(text(why.to_string()).size(12).style(style::accent));
        }
        let last = self.last_call();
        let failed = matches!(last.map(|c| &c.called), Some(Called::Refused(_) | Called::Failed(_)));
        let verb = if f.kind == crate::form::Kind::Query {
            "run the query"
        } else {
            "author the mutation"
        };
        let hint = if keys_here && f.at == f.fields.len() {
            "\u{2190} <Enter> calls"
        } else {
            "r calls from anywhere"
        };
        let said = text(last.map_or(String::new(), |c| Explorer::outcome(c, w))).size(13);
        let run = row![
            button(text(verb).size(13)).style(style::action).padding([3, 12]).on_press(Msg::Call),
            small(hint),
            Space::new().width(Length::Fill),
            said.style(if failed { style::accent } else { style::dim }),
        ];
        let form = container(scrollable(lines).style(style::bars)).max_height(300.0);
        let mut out = column![title, form, run.spacing(10).align_y(Alignment::Center)].spacing(8);
        if let Some(g) = self.answer_grid() {
            let (r, c) = self.answer_at;
            let detail = g.detail(r, c);
            out = out.push(grid_view(g, vec![], (r, c), self.in_answer && self.has_keys, None, None, Msg::AnswerCell));
            out = out.push(detail_line(detail));
        }
        out.into()
    }

    /// One field of a form: its name, its type and its checks (or what is
    /// wrong with it), and the line it is typed on — with a picker beside an
    /// id's.
    fn view_field<'a>(&'a self, f: &Form, i: usize, name: &str, field: &Field, typed: &str, on: bool) -> Element<'a, Msg> {
        let aside = match f.message(name) {
            Some(why) => text(why.to_string()).size(12).style(style::accent),
            None => text(field.checks.join(" \u{b7} ")).size(11).style(style::faint(0.55)),
        };
        let hint = match &field.ty {
            Ty::Text => "text".to_string(),
            Ty::Option(_) => format!("{}, or nothing for none", ty_name(&field.ty)),
            t => ty_name(t),
        };
        let input = text_input(&hint, typed)
            .on_input(move |s| Msg::FormText(vec![i], s))
            .on_submit(Msg::Call)
            .size(13);
        let mut editor = row![input.width(Length::Fixed(320.0))].spacing(8).align_y(Alignment::Center);
        if matches!(&field.ty, Ty::Id(_)) || matches!(&field.ty, Ty::Option(t) if matches!(**t, Ty::Id(_))) {
            editor = editor.push(link("choose", Msg::Pick(vec![i])));
        }
        let label = text(name.to_string()).size(13).style(move |th| text::Style {
            color: Some(style::on_row(th, on, false, false)),
        });
        let head = row![
            container(label).width(Length::Fixed(LABEL)).clip(true),
            small(fit::tail(&ty_name(&field.ty), 40)),
            aside
        ]
        .spacing(10);
        let line = container(column![head, editor].spacing(3)).width(Length::Fill).padding(4);
        mouse_area(line.style(move |th| style::row(th, on, true, i % 2 == 1)))
            .on_press(Msg::FormAt(i))
            .into()
    }

    /// The picker under an id field: the rows of its table, each as its
    /// cells say, filtered by what is typed, the one under the cursor lit.
    fn view_picker<'a>(&'a self, src: &Source<'_>, table: &str, filter: &str, at: usize) -> Element<'a, Msg> {
        let (all, shown) = self.pick_rows(src);
        let head = row![
            small(format!("choose a row of {table}")),
            text_input("filter", filter)
                .on_input(Msg::PickFilter)
                .on_submit(Msg::Picked(at))
                .size(12)
                .width(Length::Fixed(200.0)),
            small(format!("{} of {}", shown.len(), all.len())),
            link("close", Msg::ClosePicker),
        ];
        let rows = shown.iter().enumerate().fold(Column::new(), |col, (k, &i)| {
            let color = move |th: &iced::Theme| text::Style {
                color: Some(style::entry_text(th, k == at)),
            };
            let body = container(text(fit::tail(&all[i].1, 90)).size(13).wrapping(text::Wrapping::None).style(color));
            col.push(mouse_area(body.width(Length::Fill).padding([2, 8]).style(move |th| style::entry(th, k == at, true))).on_press(Msg::Picked(k)))
        });
        let body = column![head.spacing(8).align_y(Alignment::Center), scrollable(rows).style(style::bars)];
        container(container(body).max_height(260.0).max_width(640.0))
            .padding(Padding { left: 16.0, ..Padding::ZERO })
            .into()
    }
}

/// The whole of the cell under the cursor, on a line of its own.
fn detail_line<'a>(detail: Option<String>) -> Element<'a, Msg> {
    container(small(detail.unwrap_or_default()))
        .width(Length::Fill)
        .max_height(52.0)
        .clip(true)
        .into()
}

/// A [`Grid`] as a table: every column its own width, every cell clipped to
/// it, headings over the same columns (a click sorts, where `sort` says how),
/// rows scrolling under them and both sideways together. `cursor` is the
/// cell lit, filled with the accent when `focused`; `editing` a cell drawn
/// as its editor; a cell lit by a change washes the host's accent over it.
fn grid_view<'a>(
    g: Grid,
    lights: Vec<RowLight>,
    cursor: (usize, usize),
    focused: bool,
    editing: Option<(usize, usize, String)>,
    sort: Option<fn(usize) -> Msg>,
    on_cell: impl Fn(usize, usize) -> Msg + 'a,
) -> Element<'a, Msg> {
    let px = |c: usize| Length::Fixed(g.widths[c] as f32 * CHAR_PX + table::SPACING);
    let width = g.widths.iter().map(|w| *w as f32 * CHAR_PX + table::SPACING).sum::<f32>() + table::ROW_PADDING[1] * 2.0;
    let pad = Padding {
        left: 3.0,
        right: table::SPACING - 3.0,
        ..Padding::ZERO
    };
    let head = (0..g.columns.len()).fold(Row::new(), |r, c| {
        let h = mouse_area(table::heading(fit::tail(&g.headings[c], g.widths[c]), Length::Shrink));
        r.push(
            container(match sort {
                Some(s) => h.on_press(s(c)),
                None => h,
            })
            .width(px(c))
            .padding(pad)
            .clip(true),
        )
    });
    let mut rows = Column::new();
    for r in 0..g.rows.len() {
        let mut line = Row::new().align_y(Alignment::Center);
        for c in 0..g.columns.len() {
            let on = cursor == (r, c);
            let lit = lights.get(r).and_then(|l| l.cells.get(c)).copied().unwrap_or(0.0);
            let cell: Element<'a, Msg> = match &editing {
                Some((er, ec, t)) if (*er, *ec) == (r, c) => {
                    let input = text_input(&g.columns[c], t)
                        .on_input(Msg::EditText)
                        .on_submit(Msg::Commit)
                        .size(13)
                        .padding([1, 3]);
                    container(input)
                        .width(px(c))
                        .height(Length::Fixed(ROW_HEIGHT))
                        .align_y(Alignment::Center)
                        .into()
                }
                _ => {
                    let color = move |th: &iced::Theme| text::Style {
                        color: Some(style::on_row(th, on && focused, cursor.0 == r && !on, false)),
                    };
                    let body = container(text(g.cell(r, c)).size(13).wrapping(text::Wrapping::None).style(color))
                        .width(px(c))
                        .height(Length::Fixed(ROW_HEIGHT))
                        .align_y(Alignment::Center)
                        .padding(pad)
                        .clip(true)
                        .style(move |th| match on {
                            true => style::row(th, on, focused, false),
                            false => container::Style {
                                background: (lit > 0.0)
                                    .then(|| iced::Background::Color(arkui::theme::of(th).primary.base.color.scale_alpha(0.55 * lit))),
                                ..container::Style::default()
                            },
                        });
                    mouse_area(body).on_press(on_cell(r, c)).into()
                }
            };
            line = line.push(cell);
        }
        let pad = Padding::from([0.0, table::ROW_PADDING[1]]);
        rows = rows.push(
            container(line)
                .width(Length::Fixed(width))
                .padding(pad)
                .style(move |th| style::row(th, false, focused, r % 2 == 1)),
        );
    }
    if g.rows.is_empty() {
        rows = rows.push(container(small("no rows")).padding(4));
    }
    let head = container(head)
        .width(Length::Fixed(width))
        .padding(Padding::from([0.0, table::ROW_PADDING[1]]));
    let body = scrollable(rows)
        .style(style::bars)
        .height(Length::Fill)
        .width(Length::Fixed(width + arkui::SCROLLBAR + 4.0));
    let both = scrollable(column![head, body].spacing(2)).direction(Direction::Horizontal(Scrollbar::default()));
    both.style(style::bars).width(Length::Fill).height(Length::Fill).into()
}
