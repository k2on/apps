//! What the explorer draws. Nothing here writes a colour down — every one is
//! asked of the host's palette through `arkui` — and nothing here decides
//! anything: what the cursor is on, what an edit goes through and what the
//! switch says are answered in [`crate::model`].

use arkui::{fit, style, table};
use iced::widget::{button, column, container, mouse_area, row, rule, scrollable, text, text_input, Column, Row};
use iced::{Alignment, Element, Length};

use crate::data::{Source, Writer};
use crate::edit::cell_text;
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
        let columns = Explorer::columns(src, t);
        let key: Vec<String> = src.store.schema().lookup_table(t).map(|tbl| tbl.key.clone()).unwrap_or_default();
        let all = src.store.scan(t);
        let (page, pages) = self.page(all.len());
        let from = page * PAGE_ROWS;
        let head = columns.iter().fold(Row::new(), |r, c| {
            let label = if key.contains(c) { format!("{c} \u{b7} key") } else { c.clone() };
            r.push(table::heading(label, Length::Fill))
        });
        let rows = all.iter().enumerate().skip(from).take(PAGE_ROWS).fold(Column::new(), |col, (i, r)| {
            let on_row = i == self.row_at;
            let line = columns.iter().enumerate().fold(Row::new().align_y(Alignment::Center), |line, (j, c)| {
                let editing = self.editing.as_ref().filter(|e| on_row && e.column == *c && e.table == t);
                let cell: Element<'a, Msg> = match editing {
                    Some(e) => text_input(c, &e.text)
                        .on_input(Msg::EditText)
                        .on_submit(Msg::Commit)
                        .size(13)
                        .width(Length::Fill)
                        .into(),
                    None => mouse_area(table::cell(
                        r.get(c).map(cell_text).unwrap_or_default(),
                        Length::Fill,
                        on_row && j == self.column_at,
                        on_row,
                        !on_row,
                    ))
                    .on_press(Msg::Cell(i, j))
                    .into(),
                };
                line.push(container(cell).width(Length::Fill))
            });
            col.push(table::row(line, false, true, i % 2 == 1))
        });
        let through = self.writes_through(w, t);
        let foot = row![
            small(format!(
                "{t}: rows {}\u{2013}{} of {} \u{b7} page {} of {pages} \u{b7} {through}",
                if all.is_empty() { 0 } else { from + 1 },
                (from + PAGE_ROWS).min(all.len()),
                all.len(),
                page + 1
            )),
            container(text("")).width(Length::Fill),
            button(text("edit").size(12)).style(button::text).on_press(Msg::Edit),
            button(text("delete row").size(12)).style(button::text).on_press(Msg::Delete),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        column![head, scrollable(rows).style(style::bars).height(Length::Fill), foot]
            .spacing(4)
            .into()
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
