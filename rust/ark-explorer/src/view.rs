//! What the explorer draws. Nothing here writes a colour down — every one is
//! asked of the host's palette through `arkui` — and nothing here decides
//! anything: what the cursor is on, what an edit goes through and what the
//! switch says are answered in [`crate::model`].

use arkui::{fit, panel, style, table};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, column, container, mouse_area, row, rule, scrollable, text, text_input, Column, Row, Space};
use iced::{Alignment, Element, Length, Padding};

use ark::schema::Ty;

use crate::cells::{self, Grid};
use crate::data::{Source, Writer};
use crate::form::{self, Called, Input};
use crate::model::{Explorer, Msg, Screen, PAGE_ROWS};

const NAME: Length = Length::FillPortion(3);
const WIDE: Length = Length::FillPortion(6);
const NARROW: Length = Length::Fixed(64.0);
const SEQ: Length = Length::Fixed(56.0);
const FN_NAME: Length = Length::FillPortion(3);
const KIND: Length = Length::Fixed(80.0);
/// A form's name column, its type column, and how far a nested line moves in.
const LABEL: f32 = 130.0;
const TYPE: f32 = 130.0;
const INDENT: f32 = 16.0;
/// The tallest a form is drawn before it scrolls, so a query's answer below
/// it keeps its room.
const FORM_HEIGHT: f32 = 300.0;
const PICKER_WIDTH: f32 = 640.0;
const PICKER_HEIGHT: f32 = 260.0;

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
            .push(tab(
                "functions",
                matches!(self.screen, Screen::Functions | Screen::Call(_)),
                Screen::Functions,
            ))
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
            Screen::Functions => self.view_functions(src, w),
            Screen::Call(f) => self.view_call(src, w, f),
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
        // The editor is drawn on the row it edits, found by its key.
        let editing = self.editing.as_ref().filter(|e| e.table == t).and_then(|e| {
            let at = self.editing_at(src)?.checked_sub(from)?;
            let column = Explorer::columns(src, t).iter().position(|c| *c == e.column)?;
            Some((at, column, e.text.clone()))
        });
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

    fn view_functions<'a>(&'a self, src: &Source<'_>, w: &dyn Writer) -> Element<'a, Msg> {
        let all = Explorer::callables(src);
        let mut list = Column::new();
        let mut router: Option<&str> = None;
        for (i, c) in all.iter().enumerate() {
            if router != Some(c.router.as_str()) {
                router = Some(c.router.as_str());
                let name = if c.router.is_empty() {
                    "(no router)".to_string()
                } else {
                    c.router.clone()
                };
                list = list.push(table::section(name, 4.0));
            }
            let on = i == self.functions_at;
            let input = if c.input.is_empty() {
                "no input".to_string()
            } else {
                c.input
                    .iter()
                    .map(|(n, f)| format!("{n}: {}", form::ty_name(&f.ty)))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let line = row![
                table::cell(c.name.clone(), FN_NAME, on, false, false),
                table::cell(form::kind_name(c.kind).to_string(), KIND, on, false, true),
                table::cell(input, Length::FillPortion(8), on, false, true),
            ];
            list = list.push(
                mouse_area(table::row(line, on, true, i % 2 == 1))
                    .on_enter(Msg::At(i))
                    .on_press(Msg::Function(i)),
            );
        }
        if all.is_empty() {
            list = list.push(small("this module has no queries and no mutations"));
        }
        let head = row![
            table::heading("Function", FN_NAME),
            table::heading("Kind", KIND),
            table::heading("Input", Length::FillPortion(8)),
        ];
        column![
            head,
            scrollable(list).style(style::bars).height(Length::FillPortion(3)),
            rule::horizontal(1),
            self.view_recent(w),
        ]
        .spacing(4)
        .into()
    }

    /// The calls made this session, newest first, each with its arguments
    /// and what it came to, to call again or to put back in its form.
    fn view_recent<'a>(&'a self, w: &dyn Writer) -> Element<'a, Msg> {
        let mut out = Column::new().spacing(2);
        out = out.push(small(match self.calls.len() {
            0 => "no calls yet: <Enter> on a function opens its form".to_string(),
            n => format!("{n} call{} this session \u{b7} . calls the last one again", if n == 1 { "" } else { "s" }),
        }));
        for (i, c) in self.calls.iter().enumerate().rev().take(8) {
            let line = row![
                table::cell(c.function.clone(), FN_NAME, false, false, false),
                table::cell(c.args_text(), Length::FillPortion(5), false, false, true),
                table::cell(
                    Explorer::outcome(c, w),
                    Length::FillPortion(4),
                    false,
                    matches!(c.called, Called::Refused(_) | Called::Failed(_)),
                    true
                ),
                button(text("again").size(12)).style(button::text).padding([0, 6]).on_press(Msg::Again(i)),
                button(text("load").size(12)).style(button::text).padding([0, 6]).on_press(Msg::Recall(i)),
            ]
            .align_y(Alignment::Center);
            out = out.push(table::row(line, false, true, i % 2 == 1));
        }
        scrollable(out).style(style::bars).height(Length::FillPortion(2)).into()
    }

    fn view_call<'a>(&'a self, src: &Source<'_>, w: &dyn Writer, name: &str) -> Element<'a, Msg> {
        let Some(f) = self.forms.get(name) else {
            return small(format!("no form for {name}"));
        };
        let title = row![
            text(name.to_string()).size(15),
            small(format!(
                "{} \u{b7} {}",
                form::kind_name(f.kind),
                if f.fields.is_empty() {
                    "no input".to_string()
                } else {
                    format!("{} field{}", f.fields.len(), if f.fields.len() == 1 { "" } else { "s" })
                }
            )),
        ]
        .spacing(10)
        .align_y(Alignment::End);
        let slots = f.slots();
        let keys_here = !self.in_answer && self.picker.is_none();
        let mut lines = Column::new().spacing(2);
        for (i, slot) in slots.iter().enumerate() {
            let on = keys_here && i == f.at;
            lines = lines.push(self.view_slot(src, f, slot, i, on));
            if let Some(p) = self.picker.as_ref().filter(|p| p.path == slot.path) {
                lines = lines.push(self.view_picker(src, p, slot.depth));
            }
        }
        if slots.is_empty() {
            lines = lines.push(small("nothing to fill in"));
        }
        if let Some(why) = f.message("") {
            lines = lines.push(text(why.to_string()).size(12).style(style::accent));
        }
        let on_run = keys_here && f.at == slots.len();
        let verb = if f.kind == ark::ir::FnKind::Query {
            "run the query"
        } else {
            "author the mutation"
        };
        let run = row![
            button(text(verb).size(13)).style(style::action).padding([3, 12]).on_press(Msg::Call),
            text(if on_run { "\u{2190} <Enter> calls" } else { "r calls from anywhere" })
                .size(12)
                .style(style::dim),
            Space::new().width(Length::Fill),
            text(self.last_call().map_or(String::new(), |c| Explorer::outcome(c, w))).size(13).style(
                if matches!(self.last_call().map(|c| &c.called), Some(Called::Refused(_) | Called::Failed(_))) {
                    style::accent
                } else {
                    style::dim
                }
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let form = container(scrollable(lines).style(style::bars)).max_height(FORM_HEIGHT);
        let mut out = column![title, form, run].spacing(8);
        if let Some(g) = self.answer_grid() {
            let (r, c) = self.answer_at;
            let detail = g.detail(r, c);
            out = out.push(view_grid(g, Some((r, c)), self.in_answer, None, Msg::AnswerCell));
            out = out.push(detail_line(detail));
        } else {
            out = out.push(Space::new().height(Length::Fill));
        }
        out.into()
    }

    /// One line of a form: its name and type, its editor, and its checks or
    /// what is wrong with it.
    fn view_slot<'a>(&'a self, src: &Source<'_>, f: &'a form::Form, slot: &form::Slot, i: usize, on: bool) -> Element<'a, Msg> {
        let path = slot.path.clone();
        let input = value_at(f, &path);
        let label = text(slot.label.clone())
            .size(13)
            .wrapping(text::Wrapping::None)
            .style(move |theme| text::Style {
                color: Some(style::on_row(theme, on, false, false)),
            });
        let ty = text(fit::tail(&form::ty_name(&slot.ty), 28))
            .size(11)
            .wrapping(text::Wrapping::None)
            .style(move |theme| text::Style {
                color: Some(style::on_row(theme, on, false, true)),
            });
        let editor: Element<'a, Msg> = match (&slot.ty, input) {
            (Ty::Bool, Some(Input::Bool(b))) => button(text(if *b { "true" } else { "false" }).size(13))
                .style(if *b { style::action } else { button::secondary })
                .padding([2, 10])
                .on_press(Msg::FormToggle(path.clone()))
                .into(),
            (Ty::Enum(vs), Some(Input::Choice(c))) => vs
                .iter()
                .enumerate()
                .fold(Row::new().spacing(4), |r, (k, v)| {
                    r.push(
                        button(text(v.clone()).size(12))
                            .style(if k == *c { style::action } else { button::secondary })
                            .padding([2, 8])
                            .on_press(Msg::FormChoose(path.clone(), k)),
                    )
                })
                .into(),
            (Ty::Option(_), Some(Input::Opt(some, _))) => button(text(if *some { "has a value" } else { "none" }).size(12))
                .style(if *some { button::secondary } else { style::action })
                .padding([2, 10])
                .on_press(Msg::FormToggle(path.clone()))
                .into(),
            (Ty::List(_), Some(Input::List(xs))) => row![
                small(format!("{} item{}", xs.len(), if xs.len() == 1 { "" } else { "s" })),
                button(text("add").size(12))
                    .style(button::secondary)
                    .padding([2, 10])
                    .on_press(Msg::FormAdd(path.clone())),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            (Ty::Struct(_), _) => small("fields below"),
            (Ty::Id(table), Some(Input::Text(t))) => {
                let named = form::parse_id(t).and_then(|id| form::pickables(src.store, table).into_iter().find(|p| p.id == id));
                let says = match (t.trim().is_empty(), named) {
                    (true, _) => String::new(),
                    (false, Some(p)) => fit::tail(&p.line(), 40),
                    (false, None) if form::parse_id(t).is_some() => format!("no row of {table} here"),
                    (false, None) => String::new(),
                };
                row![
                    text_input("paste an id, or choose", t)
                        .on_input({
                            let path = path.clone();
                            move |s| Msg::FormText(path.clone(), s)
                        })
                        .on_submit(Msg::Call)
                        .size(13)
                        .width(Length::Fixed(300.0)),
                    button(text("choose").size(12))
                        .style(button::secondary)
                        .padding([2, 10])
                        .on_press(Msg::Pick(path.clone())),
                    text(says).size(12).wrapping(text::Wrapping::None).style(style::dim),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            (_, Some(Input::Text(t))) => text_input(
                match slot.ty {
                    Ty::Int => "a whole number",
                    Ty::Bytes => "hex digits",
                    _ => "text",
                },
                t,
            )
            .on_input({
                let path = path.clone();
                move |s| Msg::FormText(path.clone(), s)
            })
            .on_submit(Msg::Call)
            .size(13)
            .width(Length::Fixed(if slot.ty == Ty::Int { 160.0 } else { 300.0 }))
            .into(),
            _ => small("?"),
        };
        let remove: Element<'a, Msg> = if slot.item {
            button(text("remove").size(12))
                .style(button::text)
                .padding([2, 6])
                .on_press(Msg::FormRemove(path.clone()))
                .into()
        } else {
            Space::new().width(Length::Shrink).into()
        };
        // Beside a field: what is wrong with it, or else what it is checked
        // against before the call.
        let top = slot.depth == 0;
        let name = f.fields.get(path[0]).map(|(n, _)| n.as_str()).unwrap_or_default();
        let aside: Element<'a, Msg> = match (top, f.message(name)) {
            (true, Some(why)) => text(why.to_string()).size(12).wrapping(text::Wrapping::None).style(style::accent).into(),
            (true, None) => {
                let checks = f.fields.get(path[0]).map(|(_, fd)| form::checks_said(fd)).unwrap_or_default();
                text(checks.join(" \u{b7} "))
                    .size(11)
                    .wrapping(text::Wrapping::None)
                    .style(style::faint(0.55))
                    .into()
            }
            (false, _) => Space::new().width(Length::Shrink).into(),
        };
        // Two lines, so a form fits a panel docked beside the app: what the
        // field is and what it is checked against, then its editor.
        let line = column![
            row![
                container(label).width(Length::Fixed(LABEL - slot.depth as f32 * INDENT)).clip(true),
                container(ty).width(Length::Fixed(TYPE)).clip(true),
                container(aside).width(Length::Fill).clip(true),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            container(row![editor, remove].spacing(8).align_y(Alignment::Center))
                .padding(Padding { left: 12.0, ..Padding::ZERO })
                .width(Length::Fill)
                .clip(true),
        ]
        .spacing(3);
        mouse_area(
            container(line)
                .width(Length::Fill)
                .padding(Padding {
                    top: 4.0,
                    bottom: 4.0,
                    right: 4.0,
                    left: 4.0 + slot.depth as f32 * INDENT,
                })
                .style(move |theme| style::row(theme, on, true, i % 2 == 1)),
        )
        .on_press(Msg::FormAt(i))
        .into()
    }

    /// The picker under an id field: the rows of its table, labelled,
    /// filtered by what is typed, the one under the cursor lit.
    fn view_picker<'a>(&'a self, src: &Source<'_>, p: &form::Picker, depth: usize) -> Element<'a, Msg> {
        let (all, shown) = self.pick_rows(src);
        let mut list = Column::new();
        if all.is_empty() {
            list = list.push(panel::title(format!("{} holds no rows here", p.table)));
        } else if shown.is_empty() {
            list = list.push(panel::title(format!("no row of {} holds \u{201c}{}\u{201d}", p.table, p.filter)));
        }
        for (k, &i) in shown.iter().enumerate() {
            let lit = k == p.at;
            let pick = &all[i];
            let body = row![
                text(fit::tail(&pick.label, 44))
                    .size(13)
                    .width(Length::FillPortion(5))
                    .wrapping(text::Wrapping::None)
                    .style(move |theme| text::Style {
                        color: Some(style::entry_text(theme, lit)),
                    }),
                text(fit::tail(pick.secondary.as_deref().unwrap_or_default(), 32))
                    .size(12)
                    .width(Length::FillPortion(3))
                    .wrapping(text::Wrapping::None)
                    .style(move |theme| text::Style {
                        color: Some(style::entry_text(theme, lit).scale_alpha(0.7)),
                    }),
                text(pick.short())
                    .size(12)
                    .width(Length::Fixed(cells::px(cells::ID_DIGITS)))
                    .wrapping(text::Wrapping::None)
                    .style(move |theme| text::Style {
                        color: Some(style::entry_text(theme, lit).scale_alpha(0.7)),
                    }),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            list = list.push(
                mouse_area(
                    container(body)
                        .width(Length::Fill)
                        .height(Length::Fixed(panel::ENTRY))
                        .align_y(Alignment::Center)
                        .padding(Padding::from([0.0, panel::ENTRY_PAD_X]))
                        .style(move |theme| style::entry(theme, lit, true)),
                )
                .on_enter(Msg::PickAt(k))
                .on_press(Msg::Picked(k)),
            );
        }
        let head = row![
            panel::title(format!("choose a row of {}", p.table)),
            text_input("filter", &p.filter)
                .on_input(Msg::PickFilter)
                .on_submit(Msg::Picked(p.at))
                .size(12)
                .width(Length::Fixed(200.0)),
            Space::new().width(Length::Fill),
            small(format!("{} of {}", shown.len(), all.len())),
            button(text("close").size(12)).style(button::text).on_press(Msg::ClosePicker),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let body = column![head, scrollable(list).style(style::bars).height(Length::Shrink)];
        let panel = panel::panel(container(body).max_height(PICKER_HEIGHT), PICKER_WIDTH)
            .width(Length::Fill)
            .max_width(PICKER_WIDTH);
        container(panel)
            .padding(Padding {
                left: 16.0 + depth as f32 * INDENT,
                top: 2.0,
                bottom: 4.0,
                right: 0.0,
            })
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

/// The input at `path` of a form, to draw.
fn value_at<'f>(f: &'f form::Form, path: &[usize]) -> Option<&'f Input> {
    let (&i, rest) = path.split_first()?;
    let mut ty = &f.fields.get(i)?.1.ty;
    let mut v = f.values.get(i)?;
    for &k in rest {
        (ty, v) = match (ty, v) {
            (Ty::Option(t), Input::Opt(_, inner)) => (&**t, &**inner),
            (Ty::List(t), Input::List(xs)) => (&**t, xs.get(k)?),
            (Ty::Struct(fs), Input::Fields(xs)) => (fs.values().nth(k)?, xs.get(k)?),
            _ => return None,
        };
    }
    Some(v)
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
