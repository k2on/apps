//! What the explorer draws. Nothing here writes a colour down — every one is
//! asked of the host's palette through `arkui` — and nothing here decides
//! anything: what the cursor is on, what an edit goes through and what the
//! switch says are answered in [`crate::model`].

use arkui::{fit, style, table};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, column, container, mouse_area, row, rule, scrollable, text, text_input, Column, Row, Space};
use iced::{Alignment, Element, Length, Padding};

use crate::cells::{self, Grid};
use crate::data::{Source, Writer};
use crate::model::{Explorer, Msg, Screen, PAGE_ROWS};

const NAME: Length = Length::FillPortion(3);
const WIDE: Length = Length::FillPortion(6);
const NARROW: Length = Length::Fixed(64.0);
const SEQ: Length = Length::Fixed(56.0);

fn small<'a>(s: impl Into<String>) -> Element<'a, Msg> {
    text(s.into()).size(12).style(style::dim).into()
}

fn tab<'a>(label: &'static str, lit: bool, to: Screen) -> Element<'a, Msg> {
    let t = text(label).size(13);
    let t = if lit { t.style(style::accent) } else { t.style(style::dim) };
    button(t).style(button::text).padding([2, 6]).on_press(Msg::Show(to)).into()
}

impl Explorer {
    /// The whole explorer: which screen, the screen, and the status line.
    pub fn view<'a>(&'a self, src: &Source<'_>, w: &dyn Writer) -> Element<'a, Msg> {
        let on_tables = matches!(self.screen, Screen::Tables | Screen::Table(_));
        let mut head = Row::new()
            .spacing(4)
            .align_y(Alignment::Center)
            .push(text("explorer").size(16))
            .push(tab("tables", on_tables, Screen::Tables))
            .push(tab("log", self.screen == Screen::Log, Screen::Log))
            .push(tab("console", self.screen == Screen::Console, Screen::Console))
            .push(
                container(small(match w.who() {
                    who if who.is_empty() => String::new(),
                    who => format!("writes as {who}"),
                }))
                .width(Length::Fill),
            );
        // The switch to write raw, where the writer can: what it switches is
        // said beside it. A client's host cannot, and it is not drawn.
        if w.can_raw() {
            head = head.push(
                button(text(if self.raw { "raw: on" } else { "raw: off" }).size(12))
                    .style(if self.raw { style::action } else { button::text })
                    .padding([2, 8])
                    .on_press(Msg::ToggleRaw),
            );
        }
        let body = match &self.screen {
            Screen::Tables => self.view_tables(src, w),
            Screen::Table(t) => self.view_table(src, w, t),
            Screen::Console => self.view_console(),
            Screen::Log => self.view_log(src),
        };
        let status = row![
            text(self.note.clone()).size(12).style(style::dim).width(Length::Fill),
            text(self.keys.pending()).size(12).style(style::accent),
        ]
        .spacing(12);
        container(column![head, rule::horizontal(1), body, status].spacing(8))
            .padding(12)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_tables<'a>(&'a self, src: &Source<'_>, w: &dyn Writer) -> Element<'a, Msg> {
        let head = row![
            table::heading("Table", NAME),
            table::heading("Rows", NARROW),
            table::heading("Columns", NARROW),
            table::heading("Exposed", WIDE),
        ];
        let rows = Explorer::tables(src).into_iter().enumerate().fold(Column::new(), |col, (i, t)| {
            let on = i == self.tables_at;
            let n = src.store.scan(&t).len();
            let cols = Explorer::columns(src, &t).len();
            let exposed = match Explorer::verbs(w, &t) {
                Some(v) => {
                    let names = v.names(&t).join(" ");
                    if v.may_author {
                        names
                    } else {
                        format!("{names} (not this host's to call)")
                    }
                }
                None if w.can_raw() => "nothing: raw writes".into(),
                None => "nothing".into(),
            };
            let line = row![
                table::cell(t.clone(), NAME, on, false, false),
                table::cell(n.to_string(), NARROW, on, false, true),
                table::cell(cols.to_string(), NARROW, on, false, true),
                table::cell(exposed, WIDE, on, false, true),
            ];
            col.push(
                mouse_area(table::row(line, on, true, i % 2 == 1))
                    .on_enter(Msg::At(i))
                    .on_press(Msg::Open(t)),
            )
        });
        column![head, scrollable(rows).style(style::bars).height(Length::Fill)].spacing(4).into()
    }

    fn view_table<'a>(&'a self, src: &Source<'_>, w: &dyn Writer, t: &str) -> Element<'a, Msg> {
        let all = src.store.scan(t).len();
        let (page, pages) = self.page(all);
        let from = page * PAGE_ROWS;
        let grid = self.table_grid(src, t);
        let editing = self
            .editing
            .as_ref()
            .filter(|e| e.table == t)
            .map(|e| (self.row_at - from, self.column_at, e.text.clone()));
        let body = view_grid(
            grid,
            Some((self.row_at.saturating_sub(from), self.column_at)),
            true,
            editing,
            move |r, c| Msg::Cell(from + r, c),
        );
        let through = self.writes_through(w, t);
        let foot = row![
            small(format!(
                "{t}: rows {}\u{2013}{} of {} \u{b7} page {} of {pages} \u{b7} {through}",
                if all == 0 { 0 } else { from + 1 },
                (from + PAGE_ROWS).min(all),
                all,
                page + 1
            )),
            Space::new().width(Length::Fill),
            button(text("edit").size(12)).style(button::text).on_press(Msg::Edit),
            button(text("delete row").size(12)).style(button::text).on_press(Msg::Delete),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        column![body, detail_line(self.detail(src)), foot].spacing(4).into()
    }

    fn view_console(&self) -> Element<'_, Msg> {
        let answer: Element<'_, Msg> = match &self.answer {
            None => small("a query by name, its arguments as {\"field\": value} in the vectors' dialect; or a plan, as a module writes one"),
            Some(Ok(v)) => text(v.clone()).size(13).into(),
            Some(Err(why)) => text(why.clone()).size(13).style(style::accent).into(),
        };
        column![
            row![
                text_input("a query's name, or a plan {\"t\":\"plan\", …}", &self.query)
                    .on_input(Msg::Query)
                    .on_submit(Msg::Run)
                    .size(13)
                    .width(Length::FillPortion(2)),
                text_input("arguments, {} for none", &self.args)
                    .on_input(Msg::Args)
                    .on_submit(Msg::Run)
                    .size(13)
                    .width(Length::FillPortion(3)),
                button(text("run").size(13)).style(style::action).on_press(Msg::Run),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            scrollable(container(answer).padding(4)).style(style::bars).height(Length::Fill),
        ]
        .spacing(8)
        .into()
    }

    fn view_log<'a>(&'a self, src: &Source<'_>) -> Element<'a, Msg> {
        let log = src.log;
        let mut out = Column::new().spacing(4);
        out = out.push(text(format!("head {}", log.head())).size(13));
        let conns = log.connections();
        out = out.push(small(match conns.len() {
            0 => "no connections".to_string(),
            n => format!("{n} connection{}", if n == 1 { "" } else { "s" }),
        }));
        for (i, c) in conns.iter().enumerate() {
            let pending = c.pending.map_or(String::new(), |n| format!(" \u{b7} {n} pending"));
            out = out.push(table::row(
                row![
                    table::cell(c.who.clone(), NAME, false, false, false),
                    table::cell(format!("at {}{pending}", c.cursor), WIDE, false, false, true),
                    table::cell(c.note.clone(), WIDE, false, false, true),
                ],
                false,
                true,
                i % 2 == 1,
            ));
        }
        let verifies = log.verifies();
        if !verifies.is_empty() {
            out = out.push(small("verify answers"));
            for (i, v) in verifies.iter().enumerate() {
                let answer = match v.answer {
                    Some(true) => "agreed",
                    Some(false) => "disagreed",
                    None => "cannot say",
                };
                out = out.push(table::row(
                    row![
                        table::cell(v.who.clone(), NAME, false, false, false),
                        table::cell(format!("at {}: {answer}", v.seq), WIDE, false, v.answer == Some(false), true),
                    ],
                    false,
                    true,
                    i % 2 == 1,
                ));
            }
        }
        out = out.push(rule::horizontal(1));
        out = out.push(row![
            table::heading("Seq", SEQ),
            table::heading("Function", NAME),
            table::heading("By", NAME),
            table::heading("Standing", NAME),
            table::heading("Facts", WIDE),
        ]);
        for (i, l) in log.lines().into_iter().enumerate() {
            let on = i == self.log_at;
            let facts = match &l.facts {
                None => "\u{2014}".to_string(),
                Some(f) => f
                    .iter()
                    .map(|c| match c {
                        ark::store::Change::Add(t, _) => format!("+{t}"),
                        ark::store::Change::Remove(t, _) => format!("\u{2212}{t}"),
                        ark::store::Change::Edit(t, _, _) => format!("~{t}"),
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            };
            let line = row![
                table::cell(l.seq.map_or("\u{2026}".into(), |n| n.to_string()), SEQ, on, false, true),
                table::cell(l.function.clone(), NAME, on, false, false),
                table::cell(
                    format!("{} \u{b7} {}", l.entry.actor, fit::tail(&l.entry.session, 12)),
                    NAME,
                    on,
                    false,
                    true
                ),
                table::cell(l.standing.clone(), NAME, on, false, true),
                table::cell(facts, WIDE, on, false, true),
            ];
            out = out.push(mouse_area(table::row(line, on, true, i % 2 == 1)).on_enter(Msg::At(i)));
        }
        scrollable(out).style(style::bars).height(Length::Fill).into()
    }
}

/// The whole of the cell under the cursor, on a line of its own: what a cell
/// cut to its column leaves out.
fn detail_line<'a>(detail: Option<String>) -> Element<'a, Msg> {
    container(
        text(detail.unwrap_or_default())
            .size(12)
            .wrapping(text::Wrapping::WordOrGlyph)
            .style(style::dim),
    )
    .width(Length::Fill)
    .max_height(52.0)
    .clip(true)
    .into()
}

/// A [`Grid`] as a table: every column its own width, every cell clipped to
/// it on one line, the headings in the same columns as the cells, rows
/// scrolling under the headings and both scrolling sideways together when
/// the columns are wider than the pane. `cursor` is the cell lit, filled
/// with the accent when `focused`; `editing` is a cell drawn as its editor.
fn view_grid<'a>(
    g: Grid,
    cursor: Option<(usize, usize)>,
    focused: bool,
    editing: Option<(usize, usize, String)>,
    on_cell: impl Fn(usize, usize) -> Msg + 'a,
) -> Element<'a, Msg> {
    let width = g.width_px() + table::ROW_PADDING[1] * 2.0;
    let cell_box = |chars: usize| Length::Fixed(cells::px(chars));
    let pad = Padding {
        top: 0.0,
        bottom: 0.0,
        left: 3.0,
        right: cells::GAP_PX - 3.0,
    };
    let head = (0..g.columns.len()).fold(Row::new(), |r, c| {
        r.push(
            container(table::heading(g.heading(c), Length::Shrink))
                .width(cell_box(g.widths[c]))
                .padding(pad)
                .clip(true),
        )
    });
    let head = container(head)
        .width(Length::Fixed(width))
        .padding(Padding::from([0.0, table::ROW_PADDING[1]]));
    let mut rows = Column::new();
    for r in 0..g.rows.len() {
        let on_row = cursor.is_some_and(|(cr, _)| cr == r);
        let mut line = Row::new().align_y(Alignment::Center);
        for c in 0..g.columns.len() {
            let on = on_row && cursor.is_some_and(|(_, cc)| cc == c);
            let cell: Element<'a, Msg> = match &editing {
                Some((er, ec, t)) if *er == r && *ec == c => text_input(&g.columns[c], t)
                    .on_input(Msg::EditText)
                    .on_submit(Msg::Commit)
                    .size(13)
                    .width(cell_box(g.widths[c].max(24)))
                    .into(),
                _ => mouse_area(
                    container(
                        text(g.cell(r, c))
                            .size(13)
                            .wrapping(text::Wrapping::None)
                            .style(move |theme| text::Style {
                                color: Some(style::on_row(theme, on && focused, on_row && !on, false)),
                            }),
                    )
                    .width(cell_box(g.widths[c]))
                    .padding(pad)
                    .clip(true)
                    .style(move |theme| style::row(theme, on, focused, false)),
                )
                .on_press(on_cell(r, c))
                .into(),
            };
            line = line.push(cell);
        }
        rows = rows.push(
            container(line)
                .width(Length::Fixed(width))
                .padding(Padding::from(table::ROW_PADDING))
                .style(move |theme| style::row(theme, false, focused, r % 2 == 1)),
        );
    }
    if g.rows.is_empty() {
        rows = rows.push(container(small("no rows")).padding(4));
    }
    let body = scrollable(rows)
        .style(style::bars)
        .height(Length::Fill)
        .width(Length::Fixed(width + arkui::SCROLLBAR + 4.0));
    scrollable(column![head, body].spacing(2))
        .direction(Direction::Horizontal(Scrollbar::default()))
        .style(style::bars)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
