//! What the window draws: the sidebar, the page the sidebar picked, the
//! now-playing bar, and the layers over them.
//!
//! Nothing here writes a colour down: every one is asked of
//! `arkui::theme::of`, which answers with harken's palette for whichever
//! theme iced picked. And nothing here decides anything — which grid has the
//! keyboard, where a panel goes, how many cards fit are all answered in
//! `main.rs`, the same numbers the keyboard uses.
use arkui::fit::Cut;
use arkui::format::{clock, plural, span, spell};
use arkui::line::Line;
use arkui::{account, art, cards, fit, glyphs, hero, icon, layer, menu, panel, picker, style, table, SCROLLBAR};
use iced::widget::{button, column, container, mouse_area, row, rule, scrollable, slider, stack, text, text_input, Row};
use iced::{Alignment, Element, Length, Padding};

use crate::listening::{self, Clock, Output};
use crate::nav::{Layout, Mark};
use crate::places::{Focus, Pane, Source};
use crate::player::Player;
use crate::{
    credit, App, Level, Message, BAR_HEIGHT, DEVICES_WIDTH, PAGE_PADDING, PANE_GAP, QUEUE, QUEUE_ROWS, QUEUE_WIDTH, SHELF, SIDEBAR, SIDEBAR_WIDTH,
    TRACKS,
};

// The table's columns. Portions rather than pixels, so the text columns share
// whatever is left after the fixed ones and none can push another off a
// narrow window.
const TRANSPORT: Length = Length::Fixed(table::GUTTER);
const TRACK: Length = Length::Fixed(30.0);
const NAME: Length = Length::FillPortion(5);
const ARTIST: Length = Length::FillPortion(3);
const ALBUM: Length = Length::FillPortion(4);
const TIME: Length = Length::Fixed(56.0);
/// Where a section heading starts: over the track numbers, not out in the
/// transport's gutter.
const SECTION_INDENT: f32 = 35.0;

/// The one shape in the table, on the row making a sound: play or pause, in
/// the accent — and a button, because the place you look to see what is
/// playing is the place you reach to stop it.
fn playing<'a>(paused: bool, on_cursor: bool) -> iced::widget::Svg<'a> {
    icon::accent(if paused { glyphs::PLAY } else { glyphs::PAUSE }, on_cursor)
}

/// The speaker at the end of the bar: gold when the sound is here.
fn devices<'a>(here: bool) -> iced::widget::Svg<'a> {
    icon::tinted(glyphs::DEVICES, icon::SIZE, move |p| match here {
        true => p.primary.base.color,
        false => p.background.base.text.scale_alpha(0.75),
    })
}

/// How wide the bar's volume slider is, and a picker row's.
const VOLUME_WIDTH: f32 = 84.0;

/// The mute button: the speaker, or the speaker struck through. Dimmed
/// beside the transport, which it should not compete with for the eye; on a
/// lit picker row, the one colour legible there.
fn mute_button<'a>(level: Level, muted: bool, lit: bool) -> Element<'a, Message> {
    let glyph = if muted { glyphs::MUTED } else { glyphs::VOLUME };
    let drawn = match lit {
        true => icon::line(glyph, true),
        false => icon::plain(glyph, !muted),
    };
    button(drawn).style(button::text).padding(2).on_press(Message::Mute(level)).into()
}

/// A volume slider, 0 to 100, sending as it is dragged and once more when
/// it is let go.
fn volume_slider<'a>(level: Level, at: f32) -> Element<'a, Message> {
    let dragged = level.clone();
    slider(0.0..=100.0, at, move |v| Message::Volume(dragged.clone(), v))
        .on_release(Message::VolumeDone)
        .style(style::seek)
        .width(Length::Fixed(VOLUME_WIDTH))
        .into()
}

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let base = container(
            column![
                // The sidebar and the page share the height left once the bar
                // has taken its own, so the bar stays at the bottom.
                row![
                    self.view_sidebar(),
                    rule::vertical(1),
                    // The page, not the sidebar, is what a swipe pulls.
                    arkui::swipe::view(
                        column![self.view_top(), self.view_page()].spacing(10).into(),
                        &self.swipe,
                        iced::time::Instant::now(),
                    )
                ]
                .spacing(PANE_GAP)
                .height(Length::Fill),
                rule::horizontal(1),
                self.view_bar(),
            ]
            .spacing(12),
        )
        .padding(PAGE_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
        // The window's own ground is the one thing a style closure cannot
        // otherwise reach; painted here, so harken's near-black is on screen.
        .style(style::page);

        // Everything above the page is a layer of one stack, and the pointer
        // is tracked on the root so that `pin` shares its origin.
        let mut layers = stack![mouse_area(base).on_move(Message::Hover)];
        let focus = self.focus();

        if let Some(m) = &self.ctx.menu {
            // A backdrop under it: a click away closes it, and the wheel does
            // not reach a list that would scroll out from under a pinned menu.
            layers = layers.push(layer::backdrop(Message::CloseMenu, Message::Swallow)).push(layer::pinned(
                menu::view(m, focus == Focus::Menu, Message::MenuAt, Message::MenuActivate),
                m.origin,
            ));
        }

        if let Some(p) = &self.ctx.picker {
            let drawn = picker::view(
                p,
                focus == Focus::Picker,
                picker::Messages {
                    at: Box::new(Message::PickerAt),
                    activate: Message::PickerActivate,
                    name: Box::new(Message::PickerName),
                    create: Message::PickerCreate,
                },
            );
            layers = match p.origin {
                // A submenu gets no backdrop of its own: the menu's is under
                // both, and a second one *over* the menu would eat every click
                // on the menu's own entries.
                Some(at) => layers.push(layer::pinned(drawn, at)),
                None => layers
                    .push(layer::dimmed(Message::ClosePicker, Message::Swallow))
                    .push(layer::centred(drawn)),
            };
        }

        if let Some(at) = self.queue_panel {
            let origin = App::queue_origin(self.window, self.queue_rows().len());
            layers = layers
                .push(layer::backdrop(Message::CloseQueue, Message::Swallow))
                .push(layer::pinned(self.view_queue(at), origin));
        }

        if let Some(at) = self.devices {
            let origin = App::devices_origin(self.window, self.picker_rows().len());
            layers = layers
                .push(layer::backdrop(Message::CloseDevices, Message::Swallow))
                .push(layer::pinned(self.view_devices(at), origin));
        }

        layers.into()
    }

    /// Music, then the lists somebody made — each read back by the domain.
    fn view_sidebar(&self) -> Element<'_, Message> {
        let cursor = self.at(Pane::Sidebar);
        let focused = self.has_keys(Pane::Sidebar);
        let mut side = column![].spacing(0).padding(Padding {
            top: 0.0,
            right: 10.0,
            bottom: 0.0,
            left: 0.0,
        });
        let mut under: Option<&str> = None;
        for (i, choice) in self.peer.choices.iter().enumerate() {
            // Emitted when the kind changes rather than stored as a line, which
            // keeps the cursor's line N and the reader's the same.
            let heading = choice.source.heading();
            if heading.is_some() && heading != under {
                side = side.push(container(text(heading.unwrap_or_default()).size(10).style(style::faint(0.5))).padding([8, 10]));
            }
            under = heading;
            // One highlight, not two: the sidebar's cursor *is* what the page
            // is showing.
            let on_cursor = i == cursor;
            let lit = on_cursor && focused;
            side = side.push(
                mouse_area(
                    container(
                        row![
                            icon::line(choice.source.glyph(), lit),
                            text(choice.label.clone())
                                .size(13)
                                .width(Length::Fill)
                                .wrapping(text::Wrapping::None)
                                .style(move |theme| text::Style {
                                    color: Some(style::entry_text(theme, lit)),
                                }),
                            text(choice.count.map(|n| n.to_string()).unwrap_or_default())
                                .size(10)
                                .style(move |theme| {
                                    let p = arkui::theme::of(theme);
                                    text::Style {
                                        color: Some(match lit {
                                            true => p.primary.base.text.scale_alpha(0.7),
                                            false => p.background.base.text.scale_alpha(0.5),
                                        }),
                                    }
                                }),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([4, 10])
                    .style(move |theme| style::row(theme, on_cursor, focused, false)),
                )
                .on_press(Message::Select(choice.source.clone())),
            );
        }
        container(scrollable(side).id(SIDEBAR).style(style::bars))
            .width(Length::Fixed(SIDEBAR_WIDTH))
            .height(Length::Fill)
            .into()
    }

    /// The cover if one has arrived, the derived square until then.
    fn picture(&self, seed: &str, art_path: &str, side: f32, corner: f32) -> Element<'_, Message> {
        art::picture(seed, self.covers.handle(&self.art_url(art_path)), side, corner)
    }

    fn card(&self, title: &str, under: String, tally: i64, open: Source, round: bool, art_path: &str) -> cards::Card<Message> {
        cards::Card {
            // The square is derived from the name, so it is the square this
            // record has on every page and on the phone.
            seed: title.to_string(),
            title: title.to_string(),
            // The tally when nothing else is known, which is a fact rather
            // than a dash where one should be.
            under: match under.is_empty() {
                true => format!("{tally} {}", plural(tally, "track")),
                false => under,
            },
            round,
            picture: self.covers.handle(&self.art_url(art_path)).cloned(),
            open: Message::Select(open),
        }
    }

    /// A grid of cards and the cursor on one of them — laid out `columns()`
    /// across, the same division the keyboard walks.
    fn view_cards(&self, cards: Vec<cards::Card<Message>>, empty: &'static str) -> Element<'_, Message> {
        let cursor = self.has_keys(Pane::Tracks).then_some(self.at(Pane::Tracks));
        cards::view(SHELF, &cards, self.columns(), cursor, empty, Message::HoverAt, TRACKS)
    }

    /// The header every page that is *about* something opens with.
    fn view_header(&self, name: &str, under: &str, open_under: Option<Source>, facts: String, art_path: &str, round: bool) -> Element<'_, Message> {
        let side = 116.0;
        cards::header(
            self.picture(name, art_path, side, if round { side / 2.0 } else { 8.0 }),
            self.peer.source.kind(),
            name.to_string(),
            under.to_string(),
            open_under.map(Message::Select),
            facts,
            PAGE_PADDING,
        )
    }

    /// An album's or an artist's header: its tracks and how long they run —
    /// and, on an artist page, how many albums, because that is the fact that
    /// differs there.
    fn view_record(&self, name: &str, under: &str, open_under: Option<Source>, albums: Option<usize>, art_path: &str) -> Element<'_, Message> {
        let rows = self.peer.rows();
        let tracks = rows.len() as i64;
        let ms: i64 = rows.iter().map(|i| i.duration_ms).sum();
        let mut facts = match albums {
            Some(n) => format!("{n} {}  \u{b7}  ", plural(n as i64, "album")),
            None => String::new(),
        };
        facts.push_str(&format!("{tracks} {}", plural(tracks, "track")));
        if ms > 0 {
            facts.push_str(&format!("  \u{b7}  {}", spell(ms)));
        }
        self.view_header(name, under, open_under, facts, art_path, albums.is_some())
    }

    /// Which page the sidebar's selection is: a table, a table under a
    /// header, a grid of cards, or a work's recordings.
    fn view_page(&self) -> Element<'_, Message> {
        let p = &self.peer;
        match &p.source {
            Source::Albums => self.view_cards(
                p.albums()
                    .iter()
                    // When it came out, as a catalogue's shelf says; the year
                    // unknown, whoever made it.
                    .map(|a| {
                        let under = if a.released > 0 { a.released.to_string() } else { a.creator.clone() };
                        self.card(&a.name, under, a.tracks, Source::Album(a.name.clone()), false, &a.art)
                    })
                    .collect(),
                "No albums yet.",
            ),
            // A person is a circle and a record is a square.
            Source::Artists => self.view_cards(
                p.artists()
                    .iter()
                    .map(|a| self.card(&a.name, String::new(), a.tracks, Source::Artist(a.name.clone()), true, &a.art))
                    .collect(),
                "Nobody yet.",
            ),
            Source::Composers => self.view_cards(
                p.composers()
                    .iter()
                    .map(|c| {
                        let mut card = self.card(&c.name, span(c.born, c.died), c.works, Source::Works(c.name.clone()), true, &c.art);
                        if c.born == 0 && c.died == 0 {
                            card.under = format!("{} {}", c.works, plural(c.works, "work"));
                        }
                        card
                    })
                    .collect(),
                "Nobody yet.",
            ),
            // The keymap and the numbers are drawn where the table is, so
            // they keep the page they have always been drawn on.
            Source::Album(name) | Source::Artist(name) | Source::Recording(_, name) | Source::Playlist(_, name) if self.help || self.debug => {
                with_header(self.view_record(name, "", None, None, ""), self.view_list())
            }
            Source::Album(name) => self.view_album(name),
            Source::Playlist(_, name) => self.view_playlist(name),
            Source::Artist(name) => self.view_artist(name),
            // One composer's page, as a catalogue opens one: their banner, then
            // their works as a list in columns — the catalogue number under
            // each, the one name a work has that survives translation — then
            // their records.
            Source::Works(name) => self.view_composer(name),
            // One work and every performance of it: the rows are the same
            // music, and what tells them apart is who played it.
            Source::Work(id, title) => self.view_work(id, title),
            // One performance, in the order the work goes.
            Source::Recording(id, who) => self.view_recording(id, who),
            _ => self.view_list(),
        }
    }

    /// A page as one scrollable, so its top scrolls away with it: `top`
    /// edge to edge (a banner runs under the scrollbar), the rest clear of
    /// it — and, on a page of tracks, the status line under it all.
    fn scrolled<'a>(&'a self, top: Option<Element<'a, Message>>, body: iced::widget::Column<'a, Message>, status: bool) -> Element<'a, Message> {
        let body = container(body.width(Length::Fill)).padding(Padding {
            right: SCROLLBAR + 6.0,
            bottom: PAGE_PADDING,
            ..Padding::ZERO
        });
        let page = match top {
            Some(top) => column![top, body],
            None => column![body],
        };
        let page = scrollable(page.width(Length::Fill)).id(TRACKS).style(style::bars).height(Length::Fill);
        match status {
            true => column![page, self.view_status()].spacing(10).width(Length::Fill).into(),
            false => page.into(),
        }
    }

    /// Where the page's cursor is, while the page has the keyboard.
    fn page_cursor(&self) -> Option<usize> {
        self.has_keys(Pane::Tracks).then_some(self.at(Pane::Tracks))
    }

    /// Whether `mark` is where the cursor is.
    fn on(&self, l: &Layout, mark: Mark) -> bool {
        l.cell(mark).is_some_and(|c| Some(c) == self.page_cursor())
    }

    /// Something a click opens, drawn as `body`: the pointer over it is the
    /// cursor on it, and a click (or `<Enter>` on it) does what its cell
    /// does. Not on the page — nobody to open — and it is only `body`.
    fn opens<'a>(&self, l: &Layout, mark: Mark, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        match l.cell(mark).zip(l.cell(mark).and_then(|c| l.message(c)).cloned()) {
            Some((cell, go)) => mouse_area(body).on_enter(Message::HoverAt(cell)).on_press(go).into(),
            None => body.into(),
        }
    }

    /// A name a click opens, in `size` and `tone` — and, under the cursor,
    /// filled as the cursor's row is, so it is seen wherever it is.
    fn name<'a>(&self, l: &Layout, mark: Mark, words: String, size: f32, tone: fn(&iced::Theme) -> text::Style) -> Element<'a, Message> {
        let on = self.on(l, mark);
        let body = container(text(words).size(size).wrapping(text::Wrapping::Word).style(move |t| match on {
            true => text::Style {
                color: Some(arkui::theme::of(t).primary.base.text),
            },
            false => tone(t),
        }))
        .padding([0, 2])
        .style(move |t| match on {
            true => style::row(t, true, true, false),
            false => container::Style::default(),
        });
        self.opens(l, mark, body)
    }

    /// Names side by side, each its own link, comma between: who played it.
    fn names<'a>(
        &self,
        l: &Layout,
        names: &[String],
        mark: impl Fn(usize) -> Mark,
        size: f32,
        tone: fn(&iced::Theme) -> text::Style,
    ) -> Row<'a, Message> {
        let mut line = Row::new().align_y(Alignment::Center);
        for (k, n) in names.iter().enumerate() {
            if k > 0 {
                line = line.push(text(", ").size(size).style(tone));
            }
            line = line.push(self.name(l, mark(k), n.clone(), size, tone));
        }
        line
    }

    /// The header a record's page opens with, as a catalogue sets one: the
    /// cover beside a line of small capitals (`over`, whose it is), the title
    /// in the serif, who played it in the accent — each name a link — a
    /// small line under (`small`), and a Play button for the first track.
    #[allow(clippy::too_many_arguments)]
    fn view_cover<'a>(
        &'a self,
        l: &Layout,
        name: &str,
        art: Element<'a, Message>,
        over: Option<String>,
        title: &str,
        by: &str,
        small: String,
    ) -> Element<'a, Message> {
        let mut words = column![].spacing(4).width(Length::Fill);
        if let Some(line) = over.filter(|o| !o.is_empty()) {
            words = words.push(self.name(l, Mark::Over, line.to_uppercase(), 11.0, |t| style::faint(0.6)(t)));
        }
        words = words.push(text(title.to_string()).size(26).font(hero::SERIF).wrapping(text::Wrapping::Word));
        let by: Vec<String> = crate::nav::names(by).into_iter().map(str::to_string).collect();
        if !by.is_empty() {
            words = words.push(self.names(l, &by, Mark::By, 16.0, style::accent));
        }
        if !small.is_empty() {
            words = words.push(text(small).size(11).style(style::faint(0.5)));
        }
        if l.cell(Mark::Play).is_some() {
            let on = self.on(l, Mark::Play);
            let play = row![icon::tinted(glyphs::PLAY, 12.0, |p| p.primary.base.text), text("Play").size(13),]
                .spacing(6)
                .align_y(Alignment::Center);
            // Under the cursor, ringed: the button is already the accent.
            let ring = container(button(play).style(style::action).padding([5, 14]).on_press(Message::Swallow))
                .padding(2)
                .style(move |t| container::Style {
                    border: iced::border::rounded(8.0)
                        .width(if on { 2.0 } else { 0.0 })
                        .color(arkui::theme::of(t).background.base.text),
                    ..container::Style::default()
                });
            words = words.push(container(self.opens(l, Mark::Play, ring)).padding(Padding { top: 12.0, ..Padding::ZERO }));
        }
        let _ = name;
        container(row![art, words].spacing(28).align_y(Alignment::Center))
            .padding(Padding {
                top: PAGE_PADDING,
                bottom: 18.0,
                ..Padding::ZERO
            })
            .into()
    }

    /// How big a header's cover is drawn.
    fn cover_side(&self) -> f32 {
        if self.pane_width() < 560.0 {
            140.0
        } else {
            220.0
        }
    }

    /// The tracks of a page of them, as a catalogue lists a record's: each
    /// over who played it — every name a link — and, with `headings`, in
    /// runs of one work under its heading, whose composer and title are
    /// links too. `creator` is whose the page is: a track by somebody else
    /// says whose it is.
    fn view_tracks<'a>(&'a self, l: &Layout, creator: &str, headings: bool) -> iced::widget::Column<'a, Message> {
        let p = &self.peer;
        let rows = p.rows();
        let details: Vec<_> = rows.iter().map(|i| p.detail_of(i.id)).collect();
        let (playing_id, sounding) = self.highlighted().map_or((None, false), |(id, sounding)| (Some(id), sounding));
        let focused = self.has_keys(Pane::Tracks);
        let cursor = focused.then_some(self.at(Pane::Tracks));
        let track = |at: usize| -> Element<'a, Message> {
            let (item, d) = (&rows[at], &details[at]);
            let on_cursor = cursor == Some(at);
            let here = playing_id == Some(item.id);
            let mut line = Row::new().spacing(0).align_y(Alignment::Center).push(match here {
                true => Element::from(
                    button(playing(!sounding, on_cursor))
                        .style(button::text)
                        .padding([0, 8])
                        .on_press(Message::PlayPause),
                ),
                false => table::gutter(),
            });
            // A movement's number is in its title; a song's is its place.
            if d.work.is_empty() {
                let n = match d.track > 0 {
                    true => d.track.to_string(),
                    false => String::new(),
                };
                line = line.push(table::cell(n, TRACK, on_cursor, false, true));
            }
            let mut body = column![Line::new(item.title.clone(), 13.0, Cut::Middle, move |t| style::on_row(
                t, on_cursor, here, false
            ))]
            .spacing(1);
            let under = self.under(creator, &item.creator, d);
            if !under.is_empty() {
                let tone: fn(&iced::Theme) -> text::Style = match on_cursor {
                    true => |t| text::Style {
                        color: Some(style::on_row(t, true, false, true).scale_alpha(0.8)),
                    },
                    false => |t| text::Style {
                        color: Some(style::on_row(t, false, false, true).scale_alpha(0.8)),
                    },
                };
                let mut names = self.names(l, &under, |k| Mark::Performer(at, k), 11.0, tone);
                if !d.licence.is_empty() && !d.performer.is_empty() {
                    names = names.push(text(format!(" ({})", d.licence)).size(11).style(tone));
                }
                body = body.push(names);
            }
            let line = line
                .push(container(body).width(Length::Fill).padding([3, 0]))
                .push(table::cell(clock(item.duration_ms as f64 / 1000.0), TIME, on_cursor, false, true))
                .push(
                    button(icon::more(on_cursor))
                        .style(button::text)
                        .padding([0, 6])
                        .on_press(Message::RowMenu(item.id, crate::Anchor::Dots)),
                );
            mouse_area(table::row(line, on_cursor, focused, at % 2 == 1))
                .on_enter(Message::HoverAt(at))
                .on_press(Message::PlayItem(item.id))
                .on_right_press(Message::RowMenu(item.id, crate::Anchor::Pointer))
                .into()
        };
        let mut list = column![].spacing(0).width(Length::Fill);
        for (start, end) in crate::nav::runs(&details, headings) {
            let d = &details[start];
            if headings && !d.work.is_empty() {
                let ms: i64 = rows[start..end].iter().map(|i| i.duration_ms).sum();
                let composer = self.name(l, Mark::Composer(start), d.composer.to_uppercase(), 10.0, |t| style::faint(0.5)(t));
                let work = self.opens(
                    l,
                    Mark::Work(start),
                    container(text(d.work.clone()).size(15).font(hero::SERIF).wrapping(text::Wrapping::Word).style({
                        let on = self.on(l, Mark::Work(start));
                        move |t| text::Style {
                            color: Some(match on {
                                true => arkui::theme::of(t).primary.base.color,
                                false => arkui::theme::of(t).background.base.text,
                            }),
                        }
                    })),
                );
                list = list.push(work_heading(composer, work, ms, start == 0));
            }
            for j in start..end {
                list = list.push(track(j));
            }
        }
        if rows.is_empty() {
            list = list.push(text("Nothing here yet.").size(13).style(style::faint(0.5)));
        }
        list
    }

    /// A record's small print: when, how much, and whose label.
    fn view_foot(&self, year: i64, label: &str) -> Element<'_, Message> {
        let rows = self.peer.rows();
        let n = rows.len() as i64;
        let ms: i64 = rows.iter().map(|i| i.duration_ms).sum();
        let mut foot = column![].spacing(3);
        if year > 0 {
            foot = foot.push(text(year.to_string()).size(12).style(style::faint(0.5)));
        }
        let mut length = format!("{n} {}", plural(n, "track"));
        if ms > 0 {
            length.push_str(&format!(", {}", spell(ms)));
        }
        foot = foot.push(text(length).size(12).style(style::faint(0.5)));
        if !label.is_empty() {
            foot = foot.push(
                column![
                    text("RECORD LABEL").size(10).style(style::faint(0.5)),
                    text(label.to_string()).size(13).style(style::accent)
                ]
                .spacing(2)
                .padding(Padding { top: 14.0, ..Padding::ZERO }),
            );
        }
        container(foot)
            .padding(Padding {
                top: 24.0,
                left: SECTION_INDENT,
                ..Padding::ZERO
            })
            .into()
    }

    /// Everybody on the page's record as faces, the composers first, each
    /// opening their page — "On This Album", "On This Recording".
    fn view_faces<'a>(&'a self, l: &Layout, heading: &str, people: Vec<(String, String)>) -> iced::widget::Column<'a, Message> {
        let p = &self.peer;
        let mut out = column![].width(Length::Fill);
        if people.is_empty() {
            return out;
        }
        out = out.push(hero::heading(heading));
        let mut faces = row![].spacing(18).align_y(Alignment::Start);
        for (i, (name, role)) in people.into_iter().enumerate() {
            let art = p
                .artists()
                .iter()
                .find(|a| a.name == name)
                .map(|a| a.art.clone())
                .or_else(|| p.composers().iter().find(|c| c.name == name).map(|c| c.art.clone()))
                .unwrap_or_default();
            let on = self.on(l, Mark::Person(i));
            let face = column![
                self.picture(&name, &art, 96.0, 48.0),
                text(name.clone())
                    .size(12)
                    .align_x(Alignment::Center)
                    .wrapping(text::Wrapping::Word)
                    .style(move |t| text::Style {
                        color: Some(style::entry_text(t, on)),
                    }),
                text(role)
                    .size(11)
                    .style(style::faint(0.55))
                    .align_x(Alignment::Center)
                    .wrapping(text::Wrapping::Word),
            ]
            .spacing(4)
            .align_x(Alignment::Center)
            .width(Length::Fixed(116.0));
            faces = faces.push(self.opens(l, Mark::Person(i), face));
        }
        out.push(faces.wrap().vertical_spacing(18.0))
    }

    /// An album's page, laid out as a catalogue lays one out: the cover
    /// beside its title, who played it and the year it came out, then its
    /// tracks under the works they are movements of — each work's composer
    /// over its title, its length at its right — the small print, and
    /// everybody on it. A record of no works (a pop record) is the same page
    /// with no work headings, and its track numbers instead.
    fn view_album(&self, name: &str) -> Element<'_, Message> {
        let p = &self.peer;
        let l = self.layout();
        let album = p.albums().iter().find(|a| a.name == name);
        let creator = album.map(|a| a.creator.clone()).unwrap_or_default();
        let year = album.map_or(0, |a| a.released);
        let small = if year > 0 { year.to_string() } else { String::new() };
        let art = self.picture(name, album.map(|a| a.art.as_str()).unwrap_or_default(), self.cover_side(), 8.0);
        let header = self.view_cover(&l, name, art, None, name, &self.album_by(name), small);
        let label = album.map(|a| a.label.as_str()).unwrap_or_default();
        let people = self.people(&self.composers_here());
        let page = column![
            header,
            self.view_tracks(&l, &creator, true),
            self.view_foot(year, label),
            self.view_faces(&l, "On This Album", people)
        ];
        self.scrolled(None, page, true)
    }

    /// A playlist's page, laid out as an album's: its cover — the covers of
    /// the first records on it, four to a square — beside its name, how much
    /// is on it and a Play button, then its tracks as the table has them.
    fn view_playlist(&self, name: &str) -> Element<'_, Message> {
        let l = self.layout();
        let rows = self.peer.rows();
        let ms: i64 = rows.iter().map(|i| i.duration_ms).sum();
        let mut small = format!("PLAYLIST  \u{b7}  {} {}", rows.len(), plural(rows.len() as i64, "track"));
        if ms > 0 {
            small.push_str(&format!("  \u{b7}  {}", spell(ms)));
        }
        let header = self.view_cover(&l, name, self.mosaic(name, self.cover_side()), None, name, "", small);
        with_header(header, self.view_list())
    }

    /// A list's cover: the first four different records on it, two by two —
    /// or the one, when it has only one — as a music library draws a
    /// playlist; nothing on it, and it is the square its name makes.
    fn mosaic(&self, name: &str, side: f32) -> Element<'_, Message> {
        let p = &self.peer;
        let mut arts: Vec<String> = vec![];
        for i in p.rows() {
            let album = p.detail_of(i.id).album;
            if let Some(a) = p.albums().iter().find(|a| a.name == album && !a.art.is_empty()) {
                if !arts.contains(&a.art) {
                    arts.push(a.art.clone());
                }
            }
            if arts.len() == 4 {
                break;
            }
        }
        match arts.len() {
            0 => self.picture(name, "", side, 8.0),
            1..=3 => self.picture(name, &arts[0], side, 8.0),
            _ => {
                let q = side / 2.0;
                let quarter = |a: &str| self.picture(a, a, q, 0.0);
                container(column![
                    row![quarter(&arts[0]), quarter(&arts[1])],
                    row![quarter(&arts[2]), quarter(&arts[3])]
                ])
                .clip(true)
                .style(|_| container::Style {
                    border: iced::border::rounded(8.0),
                    ..container::Style::default()
                })
                .into()
            }
        }
    }

    /// An artist's page: their banner (or, for somebody with no wide
    /// picture, their portrait beside their name), their songs as an album
    /// lists them, and the records they made.
    fn view_artist(&self, name: &str) -> Element<'_, Message> {
        let p = &self.peer;
        let l = self.layout();
        let artist = p.artists().iter().find(|a| a.name == name);
        let composer = p.composers().iter().find(|c| c.name == name);
        let albums = p.albums_by(name);
        let tracks = p.rows().len() as i64;
        let counts = format!(
            "{} {}  \u{b7}  {tracks} {}",
            albums.len(),
            plural(albums.len() as i64, "album"),
            plural(tracks, "track")
        );
        // What they are and when they lived, as a catalogue heads a person;
        // nobody's years known, and it is how much of them there is here.
        let life = match artist.map(|a| span(a.born, a.died)).filter(|s| !s.is_empty()) {
            Some(s) => s,
            None => composer.map(|c| span(c.born, c.died)).unwrap_or_default(),
        };
        let kind = if composer.is_some() { "Composer" } else { "Artist" };
        let mut body = column![].width(Length::Fill);
        let top = match artist.filter(|a| !a.banner.is_empty()) {
            Some(a) => {
                let kind = match life.is_empty() {
                    true => format!("{kind}  \u{b7}  {counts}"),
                    false => format!("{kind}  \u{b7}  {life}"),
                };
                body = body.push(hero::heading("Songs"));
                Some(hero::banner(
                    self.banners.handle(&self.art_url(&a.banner)),
                    &kind,
                    name,
                    self.pane_width(),
                ))
            }
            None => {
                let art_path = artist.map(|a| a.art.as_str()).unwrap_or_default();
                let side = self.cover_side();
                let mut small = kind.to_uppercase();
                if !life.is_empty() {
                    small.push_str(&format!("  \u{b7}  {life}"));
                }
                small.push_str(&format!("  \u{b7}  {counts}"));
                body = body.push(self.view_cover(&l, name, self.picture(name, art_path, side, side / 2.0), None, name, "", small));
                None
            }
        };
        body = body.push(self.view_tracks(&l, name, true));
        if !albums.is_empty() {
            body = body.push(hero::heading("Albums"));
            body = body.push(self.view_shelf(&l, albums.into_iter(), self.columns()));
        }
        self.scrolled(top, body, true)
    }

    /// Records as a shelf of cards, `columns` across, each a cell of the
    /// page's.
    fn view_shelf<'a>(&'a self, l: &Layout, albums: impl Iterator<Item = &'a crate::rows::Album>, columns: usize) -> Element<'a, Message> {
        let cursor = self.page_cursor();
        let cards: Vec<(cards::Card<Message>, Option<usize>)> = albums
            .enumerate()
            .map(|(i, a)| {
                let under = if a.released > 0 { a.released.to_string() } else { String::new() };
                (
                    self.card(&a.name, under, a.tracks, Source::Album(a.name.clone()), false, &a.art),
                    l.cell(Mark::Album(i)),
                )
            })
            .collect();
        let mut shelf = column![].spacing(SHELF.gap);
        for chunk in cards.chunks(columns.max(1)) {
            let line = chunk.iter().map(|(c, cell)| {
                let hover = cell.map_or(Message::Swallow, Message::HoverAt);
                cards::view_card(SHELF, c, cell.is_some() && *cell == cursor, hover)
            });
            shelf = shelf.push(Row::with_children(line).spacing(SHELF.gap).align_y(Alignment::Start));
        }
        shelf.into()
    }

    /// A composer's page: the banner (or the header, for somebody with no
    /// wide picture), their works, and the records their works are on.
    fn view_composer(&self, name: &str) -> Element<'_, Message> {
        let p = &self.peer;
        let l = self.layout();
        let composer = p.composers().iter().find(|c| c.name == name);
        let life = composer.map(|c| span(c.born, c.died)).unwrap_or_default();
        let works = p.works().len() as i64;
        let tracks: i64 = p.works().iter().map(|w| w.tracks).sum();
        let mut page = column![].width(Length::Fill);
        let top = match composer.filter(|c| !c.banner.is_empty()) {
            Some(c) => {
                let kind = match life.is_empty() {
                    true => "Composer".to_string(),
                    false => format!("Composer  \u{b7}  {life}"),
                };
                Some(hero::banner(
                    self.banners.handle(&self.art_url(&c.banner)),
                    &kind,
                    name,
                    self.pane_width(),
                ))
            }
            None => {
                let facts = format!("{works} {}  \u{b7}  {tracks} {}", plural(works, "work"), plural(tracks, "track"));
                let art_path = composer.map(|c| c.art.as_str()).unwrap_or_default();
                page = page.push(self.view_header(name, &life, None, facts, art_path, true));
                None
            }
        };
        let by = initials(name);
        let entries: Vec<hero::Entry<Message>> = p
            .works()
            .iter()
            .map(|w| hero::Entry {
                over: by.clone(),
                title: w.title.clone(),
                under: w.catalogue.clone(),
                count: w.tracks.to_string(),
                open: Message::Select(Source::Work(w.id.clone(), w.title.clone())),
            })
            .collect();
        page = page.push(hero::heading("Works"));
        page = match entries.is_empty() {
            true => page.push(text("Nothing by them yet.").size(13).style(style::faint(0.5))),
            false => page.push(hero::listing(&entries, self.listing_columns(), self.page_cursor(), Message::HoverAt)),
        };
        let albums = p.albums_by(name);
        if !albums.is_empty() {
            page = page.push(hero::heading("Albums"));
            page = page.push(self.view_shelf(&l, albums.into_iter(), self.columns()));
        }
        self.scrolled(top, page, false)
    }

    /// A work's page, as a catalogue sets one: its composer over its title,
    /// what it is under, a hairline; then the fullest performance of it set
    /// apart, and every performance in columns — cover, who played it, when
    /// — the same music told apart by who played it.
    fn view_work(&self, id: &str, title: &str) -> Element<'_, Message> {
        let p = &self.peer;
        let l = self.layout();
        let work = p.works().iter().find(|w| w.id == id);
        let composer = work.map(|w| w.composer.clone()).unwrap_or_default();
        let mut head = column![].spacing(4).width(Length::Fill);
        if !composer.is_empty() {
            head = head.push(self.name(&l, Mark::Over, composer.clone(), 13.0, |t| style::faint(0.9)(t)));
        }
        head = head.push(text(title.to_string()).size(28).font(hero::SERIF).wrapping(text::Wrapping::Word));
        let facts = work
            .map(|w| {
                [&w.catalogue, &w.form, &w.period]
                    .into_iter()
                    .filter(|f| !f.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
            .join("  \u{b7}  ");
        if !facts.is_empty() {
            head = head.push(text(facts).size(12).style(style::faint(0.55)));
        }
        let mut page = column![
            container(head).padding(Padding {
                top: PAGE_PADDING,
                bottom: 18.0,
                ..Padding::ZERO
            }),
            rule::horizontal(1).style(style::hairline),
        ]
        .width(Length::Fill);

        let takes = p.recordings();
        if takes.is_empty() {
            page = page.push(container(text("No recordings of it yet.").size(13).style(style::faint(0.5))).padding([18, 0]));
            return self.scrolled(None, page, false);
        }
        let who = |r: &crate::rows::Recording| match r.performers.is_empty() {
            // A recording nobody is credited on — the demo has one — says
            // so, rather than guessing who played it.
            true => "Performer not named".to_string(),
            false => r.performers.clone(),
        };
        let facts = |r: &crate::rows::Recording| {
            let mut f = vec![];
            if r.recorded > 0 {
                f.push(r.recorded.to_string());
            }
            f.push(format!("{} {}", r.tracks, plural(r.tracks, "track")));
            if !r.licence.is_empty() {
                f.push(r.licence.clone());
            }
            f.join("  \u{b7}  ")
        };
        let open = |r: &crate::rows::Recording| Message::Select(Source::Recording(r.id.clone(), r.performers.clone()));

        // The fullest performance, set apart: the one to start with.
        if l.cell(Mark::Featured).is_some() {
            let first = &takes[0];
            let on = self.on(&l, Mark::Featured);
            let words = column![
                text("MOST COMPLETE").size(10).style(style::faint(0.5)),
                text(who(first)).size(15).wrapping(text::Wrapping::Word).style(move |t| text::Style {
                    color: Some(style::entry_text(t, on)),
                }),
                text(facts(first)).size(12).style(style::faint(0.55)),
            ]
            .spacing(3)
            .width(Length::Fill);
            page = page.push(
                self.opens(
                    &l,
                    Mark::Featured,
                    container(
                        row![self.picture(&first.id, &first.art, 120.0, 6.0), words]
                            .spacing(20)
                            .align_y(Alignment::Center),
                    )
                    .padding(Padding {
                        top: 22.0,
                        bottom: 4.0,
                        ..Padding::ZERO
                    }),
                ),
            );
        }

        page = page.push(hero::heading("Recordings"));
        let cursor = self.page_cursor();
        let entries: Vec<Element<'_, Message>> = takes
            .iter()
            .enumerate()
            .map(|(at, r)| {
                let e = hero::Entry {
                    over: String::new(),
                    title: who(r),
                    under: facts(r),
                    count: String::new(),
                    open: open(r),
                };
                hero::entry(Some(self.picture(&r.id, &r.art, 64.0, 4.0)), &e, Some(at) == cursor, Message::HoverAt(at))
            })
            .collect();
        page = page.push(hero::grid(entries, self.listing_columns()));
        self.scrolled(None, page, false)
    }

    /// One performance's page, laid out as its album's is: the cover beside
    /// the work and who played it, the movements, the small print — then the
    /// records it is on, and everybody on it, the composer first.
    fn view_recording(&self, id: &str, who: &str) -> Element<'_, Message> {
        let p = &self.peer;
        let l = self.layout();
        let take = p.recordings().iter().find(|r| r.id == id);
        let work = p.works().first();
        let composer = work.map(|w| w.composer.clone()).unwrap_or_default();
        let title = match work {
            Some(w) if !w.catalogue.is_empty() => format!("{}, {}", w.title, w.catalogue),
            Some(w) => w.title.clone(),
            None => who.to_string(),
        };
        let year = take.map_or(0, |r| r.recorded);
        let small = if year > 0 { year.to_string() } else { String::new() };
        let art = self.picture(id, take.map(|r| r.art.as_str()).unwrap_or_default(), self.cover_side(), 8.0);
        let header = self.view_cover(&l, id, art, Some(composer.clone()), &title, who, small);
        let label = take.map(|r| r.label.as_str()).unwrap_or_default();
        let mut page = column![header, self.view_tracks(&l, "", false), self.view_foot(year, label)].width(Length::Fill);

        // The records its tracks are on, each once.
        let on = self.albums_here();
        if !on.is_empty() {
            page = page.push(hero::heading(if on.len() == 1 { "From the Album" } else { "From the Albums" }));
            let mut shelf = row![].spacing(28);
            for (i, a) in on.into_iter().enumerate() {
                let lit = self.on(&l, Mark::Album(i));
                let words = column![
                    text(a.name.clone()).size(14).wrapping(text::Wrapping::Word).style(move |t| text::Style {
                        color: Some(style::entry_text(t, lit)),
                    }),
                    text(a.creator.clone()).size(11).style(style::faint(0.55)),
                ]
                .spacing(2)
                .width(Length::Fixed(220.0));
                shelf = shelf.push(
                    self.opens(
                        &l,
                        Mark::Album(i),
                        row![self.picture(&a.name, &a.art, 96.0, 4.0), words]
                            .spacing(14)
                            .align_y(Alignment::Center),
                    ),
                );
            }
            page = page.push(shelf.wrap().vertical_spacing(18.0));
        }
        let composers: Vec<String> = (!composer.is_empty()).then_some(composer).into_iter().collect();
        page = page.push(self.view_faces(&l, "On This Recording", self.people(&composers)));
        self.scrolled(None, page, true)
    }

    /// The track table.
    fn view_list(&self) -> Element<'_, Message> {
        let p = &self.peer;
        let (playing_id, sounding) = self.highlighted().map_or((None, false), |(id, sounding)| (Some(id), sounding));
        // Only drawn while this pane has the keyboard: the sidebar's highlight
        // is a selection and persists, this one only means "where the next
        // `j` goes".
        let focused = self.has_keys(Pane::Tracks);
        let cursor = focused.then_some(self.at(Pane::Tracks));
        // An album is the one place a track number means anything, and where
        // the performer replaces the album column that every row would repeat.
        let album_page = matches!(p.source, Source::Album(_));
        let mut part = String::new();

        let rows = p.rows().iter().enumerate().fold(column![].spacing(0), |mut col, (i, item)| {
            let on_cursor = cursor == Some(i);
            let detail = p.detail_of(item.id);
            // A section heading when the part changes: the rows are in the
            // work's order, so that is the whole of what a boundary is.
            if album_page && detail.part != part {
                part = detail.part.clone();
                if !part.is_empty() {
                    col = col.push(table::section(part.clone(), SECTION_INDENT));
                }
            }
            let here = playing_id == Some(item.id);
            let mut line = Row::new().spacing(0).align_y(Alignment::Center).push(match here {
                true => Element::from(
                    button(playing(!sounding, on_cursor))
                        .style(button::text)
                        .padding([0, 8])
                        .on_press(Message::PlayPause),
                ),
                // As tall as the button it stands in for: an empty container
                // has no height, and the other rows came out shorter.
                false => table::gutter(),
            });
            if album_page {
                let n = match detail.track > 0 {
                    true => detail.track.to_string(),
                    false => String::new(),
                };
                line = line.push(table::cell(n, TRACK, on_cursor, false, true));
            }
            let line = line
                .push(table::cell(item.title.clone(), NAME, on_cursor, here, false))
                // People's names lose their end — `Johann S…` — and titles
                // their middle, which is where they are alike.
                .push(table::cell_cut(item.creator.clone(), ARTIST, Cut::End, on_cursor, false, true))
                .push(table::cell_cut(
                    // …and the terms it was given on, beside whoever gave it:
                    // a credit nobody draws is a condition nobody met.
                    match album_page {
                        true => credit(&detail.performer, &detail.licence),
                        false => detail.album.clone(),
                    },
                    ALBUM,
                    match album_page {
                        true => Cut::End,
                        false => Cut::Middle,
                    },
                    on_cursor,
                    false,
                    true,
                ))
                .push(table::cell(clock(item.duration_ms as f64 / 1000.0), TIME, on_cursor, false, true))
                // The same menu a right click opens, for anyone who does not
                // know a right click opens one.
                .push(
                    button(icon::more(on_cursor))
                        .style(button::text)
                        .padding([0, 6])
                        .on_press(Message::RowMenu(item.id, crate::Anchor::Dots)),
                );
            col.push(
                // The background belongs to a container spanning the width: a
                // stripe that stops where the text does is not a row.
                mouse_area(table::row(line, on_cursor, focused, i % 2 == 1))
                    .on_enter(Message::HoverAt(i))
                    .on_press(Message::PlayItem(item.id))
                    .on_right_press(Message::RowMenu(item.id, crate::Anchor::Pointer)),
            )
        });

        let mut headings = Row::new()
            .spacing(0)
            .align_y(Alignment::Center)
            .push(container(text("")).width(TRANSPORT));
        if album_page {
            headings = headings.push(table::heading("#", TRACK));
        }
        let head = container(
            headings
                .push(table::heading("Name", NAME))
                .push(table::heading("Artist", ARTIST))
                .push(table::heading(if album_page { "Performer" } else { "Album" }, ALBUM))
                .push(table::heading("Time", TIME)),
        )
        .width(Length::Fill)
        .padding([2, 4]);

        // A page with a header already says the name and counts the tracks;
        // everywhere else this *is* the title.
        let titled = !matches!(
            p.source,
            Source::Album(_) | Source::Artist(_) | Source::Recording(..) | Source::Playlist(..)
        );
        let mut main = column![].spacing(10).width(Length::Fill);
        if titled {
            main = main.push(
                row![
                    // A search narrowing the Songs page says so where the
                    // page's name is (`docs/plan-db.md` D4).
                    // Cut rather than run on past the page, and the count
                    // after it keeps its room: the title gives way.
                    Line::new(
                        match (&p.source, &p.search) {
                            (Source::Library, Some(n)) => format!("{} matching “{n}”", p.source.title()),
                            _ => p.source.title().to_string(),
                        },
                        22.0,
                        Cut::Middle,
                        |theme| arkui::theme::of(theme).background.base.text,
                    )
                    .gives_way(),
                    text(format!("{} {}", p.rows().len(), plural(p.rows().len() as i64, "track")))
                        .size(12)
                        .style(style::dim),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            );
        }
        if self.help {
            // The status line stays: which pane has the cursor is exactly what
            // somebody reading the keymap is trying to work out.
            return main.push(view_help()).push(self.view_status()).into();
        }
        if self.debug {
            return main.push(self.view_debug()).push(self.view_status()).into();
        }
        main.push(head)
            .push(rule::horizontal(1))
            .push(scrollable(rows).id(TRACKS).style(style::bars).height(Length::Fill))
            .push(self.view_status())
            .into()
    }

    /// The top of the page, on every page: joining a server while this
    /// window is alone on the left, and at the right-hand end the account —
    /// who, and the menu that goes offline and signs out (`App::account`).
    /// Exactly [`arkui::account::HEIGHT`] tall, because the menu hangs off
    /// its bottom before anything is laid out.
    fn view_top(&self) -> Element<'_, Message> {
        let left: Element<'_, Message> = match !cfg!(feature = "demo") && self.peer.client.is_alone() {
            // Alone: everything works here, and this is how it starts
            // syncing — a server, then its sign-in (`docs/plan-alone.md` §4).
            true => row![
                text_input("server address", &self.connect)
                    .on_input(Message::ConnectUrl)
                    .on_submit(Message::Connect)
                    .size(12)
                    .width(Length::Fixed(220.0)),
                button("connect").style(style::action).on_press(Message::Connect),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
            false => text("").into(),
        };
        let mut bar = row![container(left).width(Length::Fill)].align_y(Alignment::Center);
        if let Some(account) = self.account() {
            bar = bar.push(account::button(&account, Message::AccountMenu));
        }
        container(bar).height(Length::Fixed(account::HEIGHT)).width(Length::Fill).into()
    }

    /// The engine showing through: how much of the log is applied, what this
    /// peer has done that nobody has confirmed, the last thing worth saying —
    /// and which grid has the keys, with anything half-typed.
    ///
    /// **All of it only under `D`** (and `?`). Otherwise only what somebody needs to
    /// see as they type: the last thing worth saying, and what is half-typed
    /// — a count, a `g`, a search — and nothing at all when there is neither.
    fn view_status(&self) -> Element<'_, Message> {
        let p = &self.peer;
        let typing = !self.keys.pending().is_empty() || matches!(self.keys.mode(), arkui::vim::Mode::Search(_));
        // The keymap keeps it whole too: which pane has the cursor is what
        // somebody reading it is trying to work out.
        let whole = self.debug || self.help;
        if !whole && self.note.is_empty() && !typing {
            return iced::widget::Space::new().height(Length::Fixed(0.0)).into();
        }
        let mut line = format!(
            "{} songs \u{b7} cursor {} \u{b7} {} pending",
            p.items().len(),
            p.client.cursor(),
            p.client.pending_len()
        );
        if !cfg!(feature = "demo") && p.client.is_alone() {
            line = format!("alone \u{b7} {line}");
        } else if !cfg!(feature = "demo") {
            // Who the replica's session row says, once it has synced.
            let who = match (&self.login, p.client.current()) {
                (Some(_), Some(s)) if !s.name.is_empty() => s.name,
                (Some(_), Some(s)) => s.user,
                (Some(l), None) => crate::auth::who(l),
                (None, _) => String::new(),
            };
            // Nobody yet is a sign-in on its way, or one the note says
            // came back refused: there is no one to name.
            let link = match (&self.login, p.client.linked) {
                (None, _) => None,
                (Some(_), true) => Some("online"),
                (Some(_), false) => Some("offline"),
            };
            if let Some(link) = link {
                line = format!("{who} \u{b7} {link} \u{b7} {line}");
            }
        }
        if !self.note.is_empty() {
            line = format!("{line}  \u{b7}  {}", self.note);
        }
        if !whole {
            line = self.note.clone();
        }
        let mode = match self.focus() {
            Focus::Pane(Pane::Sidebar) => "browse",
            Focus::Pane(Pane::Tracks) => "tracks",
            // The overlay rather than the pane under it: the pane under it is
            // not where the next key goes.
            Focus::Menu => "menu",
            Focus::Picker => "playlists",
            Focus::Devices => "devices",
            Focus::Queue => "queue",
        };
        row![
            text(line).size(12).style(style::dim).width(Length::Fill),
            // A search shows a caret — drawn, because U+2582 is not in the
            // font — so a half-typed query does not look like one that matched
            // nothing.
            row![
                text(self.keys.pending()).size(12).style(style::accent),
                match self.keys.mode() {
                    arkui::vim::Mode::Search(_) => Element::from(icon::tinted(glyphs::STOP, 9.0, |p| p.primary.base.color)),
                    arkui::vim::Mode::Normal => Element::from(text("")),
                },
            ]
            .align_y(Alignment::Center),
            text(if whole { mode } else { "" }).size(12).style(style::dim),
        ]
        .spacing(12)
        .into()
    }

    /// Every number this end of the session holds, as a table — `D` — and
    /// every change this window made, with where it stands.
    fn view_debug(&self) -> Element<'_, Message> {
        let l = &self.listening;
        let stats = l.stats();
        let client = &self.peer.client;
        let status = client.status();
        let live = crate::client::live_status(client);
        let session = self.session.as_ref();
        let yes = |b: bool| if b { "yes" } else { "no" };
        let mut rows: Vec<(String, String)> = vec![
            ("server".into(), self.server.clone()),
            (
                "login".into(),
                self.login
                    .as_ref()
                    .map(|l| format!("{} \u{b7} session {}", l.user.id, l.session))
                    .unwrap_or_else(|| "none".into()),
            ),
            ("this device".into(), format!("{} \u{b7} {}", l.name(), l.me())),
            ("can play".into(), yes(Player::AUDIBLE).into()),
            (String::new(), String::new()),
            ("link".into(), status.link.clone()),
            (
                "client linked \u{b7} epoch".into(),
                format!("{} \u{b7} {}", yes(status.linked), status.epoch),
            ),
            (
                "live seq \u{b7} pending \u{b7} synced".into(),
                match live {
                    Some(s) => format!("{} \u{b7} {} \u{b7} {}", s.seq, s.pending, yes(s.synced)),
                    None => "no instance".into(),
                },
            ),
            ("part of the session".into(), yes(l.joined(client)).into()),
            (
                "calls \u{b7} refused \u{b7} plays \u{b7} came back playing".into(),
                format!(
                    "{} \u{b7} {} \u{b7} {} \u{b7} {}",
                    stats.calls, stats.refused, stats.plays, stats.came_back
                ),
            ),
            ("last refusal".into(), stats.last_refusal.clone().unwrap_or_else(|| "none".into())),
            (String::new(), String::new()),
        ];
        match session.and_then(|s| s.session.as_ref()) {
            None => rows.push(("session".into(), if session.is_some() { "idle".into() } else { "none".into() })),
            Some(a) => {
                rows.push((
                    "output".into(),
                    match &a.output {
                        Output::Device { id, moving } => format!("{id}{}", if *moving { " (moving)" } else { "" }),
                        Output::Away { id, name } => format!("{name} \u{b7} {id} (away)"),
                        Output::Nowhere => "nowhere".into(),
                    },
                ));
                rows.push((
                    "here \u{b7} elsewhere".into(),
                    format!(
                        "{} \u{b7} {}",
                        yes(session.is_some_and(|s| l.outputs_here(s))),
                        yes(session.is_some_and(|s| l.elsewhere(s)))
                    ),
                ));
                rows.push((
                    "clock".into(),
                    match a.clock {
                        Clock::Paused { at_ms } => format!("paused at {}", clock(at_ms as f64 / 1000.0)),
                        Clock::Playing { at_ms, since } => {
                            format!("playing from {} since {since}", clock(at_ms as f64 / 1000.0))
                        }
                    },
                ));
                rows.push((
                    "at \u{b7} queue \u{b7} position".into(),
                    format!(
                        "{} \u{b7} {} \u{b7} {}",
                        a.at,
                        a.queue.len(),
                        clock(listening::Remote::position_ms(a) as f64 / 1000.0)
                    ),
                ));
                rows.push(("from".into(), a.from.clone()));
            }
        }
        rows.push((
            "player".into(),
            format!(
                "{} \u{b7} {}",
                self.player.track().map(|t| t.title.as_str()).unwrap_or("nothing loaded"),
                if self.player.is_playing() { "playing" } else { "paused" }
            ),
        ));
        let devices = session.map(|s| s.devices.as_slice()).unwrap_or_default();
        rows.push((
            "devices".into(),
            match devices.len() {
                0 => "none".into(),
                n => n.to_string(),
            },
        ));
        for d in devices {
            rows.push((
                format!("  {}", d.name),
                format!(
                    "{} \u{b7} {} \u{b7} can play {}",
                    d.id,
                    listening::Kind::parse(&d.kind).name(),
                    yes(d.can_play)
                ),
            ));
            rows.push((
                String::new(),
                format!(
                    "volume {} \u{b7} family {} \u{b7} follows {}",
                    match (d.can_volume, d.muted) {
                        (false, _) => "none".to_string(),
                        (true, false) => d.volume.to_string(),
                        (true, true) => format!("{} (muted)", d.volume),
                    },
                    d.family.as_deref().unwrap_or("none"),
                    d.follows.as_deref().unwrap_or("nobody"),
                ),
            ));
        }
        // The group that plays: its leader, its members and its level.
        rows.push((
            "group".into(),
            match session.and_then(|s| s.leader().map(|l| (s, l))) {
                Some((s, l)) if s.grouped() => format!(
                    "{l} + {} \u{b7} volume {}{}",
                    s.followers(l).iter().map(|d| d.id.as_str()).collect::<Vec<_>>().join(", "),
                    s.group_volume(l).map_or("none".to_string(), |v| v.to_string()),
                    if s.group_muted(l) { " (muted)" } else { "" }
                ),
                _ => "none".into(),
            },
        ));
        // The ephemeral slots: where each device's scrubber is being dragged.
        let scrub = session.map(|s| &s.scrub);
        rows.push((
            "scrubbing (ephemeral)".into(),
            match scrub.map_or(0, |m| m.len()) {
                0 => "nobody".into(),
                n => n.to_string(),
            },
        ));
        for (device, ms) in scrub.into_iter().flatten() {
            rows.push((format!("  {device}"), clock(*ms as f64 / 1000.0)));
        }
        rows.push((String::new(), String::new()));
        rows.push((
            "log cursor \u{b7} pending \u{b7} songs".into(),
            format!("{} \u{b7} {} \u{b7} {}", status.cursor, status.pending, self.peer.items().len()),
        ));
        // What this window did, and what became of each: pending, in the log,
        // or refused and why — the sentence every replica reaches.
        for e in self.edits.iter().rev().take(12) {
            let standing = match self.peer.client.standing(&e.id) {
                crate::client::Standing::Pending => "pending".to_string(),
                crate::client::Standing::Confirmed => "in the log".to_string(),
                crate::client::Standing::Rejected(why) => format!("refused: {why}"),
                crate::client::Standing::Unknown => "unknown".to_string(),
            };
            rows.push((format!("  {}", fit::tail(&e.what, 34)), standing));
        }
        let table = rows.into_iter().fold(column![].spacing(4), |col, (k, v)| {
            col.push(row![text(k).size(13).style(style::dim).width(Length::Fixed(260.0)), text(v).size(13)].spacing(12))
        });
        container(scrollable(column![text("the session, in numbers").size(16), table].spacing(12)).style(style::bars))
            .padding(12)
            .height(Length::Fill)
            .into()
    }

    /// The row the list lights, and whether it sounds: [`App::now`]'s
    /// track, as the bar and the title read it.
    pub(crate) fn highlighted(&self) -> Option<(crate::rows::Id, bool)> {
        self.now().map(|n| (n.track().id.0, n.playing))
    }

    /// Which device has the sound, as the button that opens the picker —
    /// drawn only where there is a session to draw.
    fn view_output(&self) -> Option<Element<'_, Message>> {
        let s = self.session.as_ref()?;
        let me = self.listening.me();
        let name_of = |id: &str| s.device(id).map_or(id.to_string(), |d| d.name.clone());
        // A session with no output is this device's to play on: the next
        // press plays here, so this device is what the button says and draws
        // lit, rather than "no device".
        let (label, here) = match s.output() {
            Some(Output::Device { id, moving: true }) => (format!("{}\u{2026}", fit::middle(&name_of(id), 14)), id == me),
            Some(Output::Device { id, .. }) if id != me => (fit::middle(&name_of(id), 16), false),
            Some(Output::Away { name, .. }) => (format!("{} (away)", fit::middle(name, 12)), false),
            _ => (fit::middle(self.listening.name(), 16), true),
        };
        Some(
            button(
                row![
                    devices(here),
                    text(label).size(12).style(move |theme| {
                        let p = arkui::theme::of(theme);
                        text::Style {
                            color: Some(match here {
                                true => p.primary.base.color,
                                false => p.background.base.text.scale_alpha(0.75),
                            }),
                        }
                    }),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .style(button::text)
            .on_press(Message::OpenDevices)
            .into(),
        )
    }

    /// The button that opens the queue, beside the device picker's.
    fn view_queue_button(&self) -> Element<'_, Message> {
        let open = self.queue_panel.is_some();
        button(icon::line(glyphs::PLAYLIST, open))
            .style(button::text)
            .padding(2)
            .on_press(Message::OpenQueue)
            .into()
    }

    /// The queue: the track playing, then what plays after it, in the order
    /// it will — the session's, or this device's own. A click or `<Enter>`
    /// plays a row; `x` takes it out, `J` and `K` move it.
    fn view_queue(&self, at: usize) -> Element<'_, Message> {
        let rows_ = self.queue_rows();
        let width = QUEUE_WIDTH;
        let chars = panel::chars(width, panel::ENTRY_CHROME);
        let row_of = |i: usize, index: usize, track: &crate::listening::Track| -> Element<'_, Message> {
            let on_cursor = at == i;
            let mark = (i == 0).then(|| Element::from(icon::line(glyphs::PLAYING, on_cursor)));
            let label = format!("{} \u{2014} {}", track.title, track.creator);
            let entry =
                panel::entry(mark, panel::label(fit::tail(&label, chars), on_cursor), None).style(move |theme| style::entry(theme, on_cursor, true));
            mouse_area(entry)
                .on_enter(Message::QueueAt(i))
                .on_press(Message::QueueAt(i))
                .on_release(Message::QueueJump(index))
                .into()
        };
        let mut body = column![panel::title("Now playing".into())].spacing(0);
        let mut upcoming = column![].spacing(0);
        match rows_.first() {
            Some((index, track)) => body = body.push(row_of(0, *index, track)),
            None => body = body.push(panel::entry(None, panel::label("nothing queued".into(), false), None)),
        }
        for (i, (index, track)) in rows_.iter().enumerate().skip(1) {
            upcoming = upcoming.push(row_of(i, *index, track));
        }
        body = body.push(panel::title(match rows_.len() {
            0 | 1 => "Up next \u{2014} nothing after this one".into(),
            n => format!("Up next \u{2014} {}", plural((n - 1) as i64, "track")),
        }));
        let visible = rows_.len().saturating_sub(1).min(QUEUE_ROWS - 1);
        body = body.push(
            scrollable(upcoming)
                .id(QUEUE)
                .style(style::bars)
                .height(Length::Fixed(panel::ENTRY * visible as f32)),
        );
        panel::panel(body, width).into()
    }

    /// Where a slider for `level` draws: where it is being dragged, or
    /// where its volume is.
    fn slider_at(&self, level: &Level, volume: i64) -> f32 {
        match &self.volume_drag {
            Some((dragged, at)) if dragged == level => *at,
            _ => volume as f32,
        }
    }

    /// The volume of whatever is playing — the group's when it is one — as a
    /// mute and a slider; nothing where there is nothing to turn up.
    fn view_volume(&self) -> Option<Element<'_, Message>> {
        let v = self.volumed(&Level::Output)?;
        let (volume, muted) = self.level_of(&v)?;
        Some(
            row![
                mute_button(Level::Output, muted, false),
                volume_slider(Level::Output, self.slider_at(&Level::Output, volume)),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into(),
        )
    }

    /// The now-playing bar, honest about silence: a build with no audio
    /// device says so rather than drawing a transport that does nothing.
    fn view_bar(&self) -> Element<'_, Message> {
        let Some(bar) = self.now() else {
            let idle = text(match Player::AUDIBLE {
                true => "nothing playing \u{2014} pick a track",
                false => "nothing playing \u{2014} pick a track (the desktop build has no audio device; the browser one streams)",
            })
            .size(12)
            .style(style::dim);
            let mut line = row![idle, container(text("")).width(Length::Fill)].spacing(12).align_y(Alignment::Center);
            if let Some(volume) = self.view_volume() {
                line = line.push(volume);
            }
            if let Some(output) = self.view_output() {
                line = line.push(output);
            }
            return container(line)
                .padding([8, 4])
                .height(Length::Fixed(BAR_HEIGHT))
                .align_y(Alignment::Center)
                .into();
        };
        let duration = bar.duration.max(0.1);
        // A scrubber being dragged draws where it is being dragged to.
        let position = self.scrubbing.map_or(bar.position, f64::from);
        let track = bar.track();
        // Enabled whenever there is a session to send it to: being a remote
        // control is a use, and `AUDIBLE` only decides whether *this* device
        // can be the one playing.
        let workable = Player::AUDIBLE || self.session.is_some();
        let transport = row![
            button(icon::plain(glyphs::PREVIOUS, true))
                .style(button::text)
                .on_press(Message::Skip(-1)),
            button(icon::plain(if bar.playing { glyphs::PAUSE } else { glyphs::PLAY }, false))
                .style(button::text)
                .on_press_maybe(workable.then_some(Message::PlayPause)),
            button(icon::plain(glyphs::NEXT, true)).style(button::text).on_press(Message::Skip(1)),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        let mut line = row![
            transport,
            column![
                text(track.title.as_str()).size(14),
                text(track.creator.as_str()).size(12).style(style::dim)
            ]
            .spacing(2)
            .width(Length::Fixed(260.0)),
            text(clock(position)).size(11).style(style::dim),
            slider(0.0..=duration as f32, position as f32, Message::Seek)
                .on_release(Message::SeekDone)
                .style(style::seek)
                .width(Length::Fill),
            text(clock(duration)).size(11).style(style::dim),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if let Some(volume) = self.view_volume() {
            line = line.push(volume);
        }
        line = line.push(self.view_queue_button());
        if let Some(output) = self.view_output() {
            line = line.push(output);
        }
        // A declared height: the device picker is placed on top of this bar
        // before iced has laid any of it out.
        container(line)
            .padding([6, 4])
            .height(Length::Fixed(BAR_HEIGHT))
            .align_y(Alignment::Center)
            .into()
    }

    /// Where the sound is, and everywhere it could be: a row per device of
    /// this account, ticked where the sound is — every member of the group
    /// that plays, when it is one, and, while it is nowhere, this device,
    /// which is where the next press plays. Each row carries its volume, and
    /// a speaker that can join what plays a `+`, one in its group a `−`; a
    /// group comes first as itself, its slider every member's. Pausing is
    /// the transport's; there is no row that stops it everywhere.
    fn view_devices(&self, at: usize) -> Element<'_, Message> {
        let rows_ = self.picker_rows();
        let me = self.listening.me().to_string();
        let session = self.session.as_ref();
        let output = session.and_then(|s| s.output()).and_then(|o| o.id()).unwrap_or(&me).to_string();
        let mut rows = column![].spacing(0);
        for (i, device) in rows_.into_iter().enumerate() {
            let on_cursor = at == i;
            // Drawn, and not a target, when it cannot be heard or is not
            // answering: a laptop controlling the session should see itself
            // listed, and the device the music still belongs to is the one
            // most worth drawing.
            let takeable = device.takeable;
            let label = if device.id == me {
                fit::middle(&device.name, 18)
            } else if session.is_some_and(|s| s.moving_to(&device.id)) {
                format!("{}\u{2026}", fit::middle(&device.name, 15))
            } else if device.away {
                format!("{} (not answering)", fit::middle(&device.name, 8))
            } else {
                fit::middle(&device.name, 18)
            };
            let mark = match device.group {
                true => Some(Element::from(icon::line(glyphs::PLAYING, on_cursor))),
                false => (device.id == output || device.playing).then(|| Element::from(icon::tick(on_cursor))),
            };
            let name = text(label)
                .size(13)
                .width(Length::Fill)
                .wrapping(text::Wrapping::None)
                .style(move |theme| text::Style {
                    color: Some(match (on_cursor, takeable || device.group) {
                        (false, false) => arkui::theme::of(theme).background.base.text.scale_alpha(0.4),
                        (lit, _) => style::entry_text(theme, lit),
                    }),
                });
            // The name is what hands the sound over; the controls beside it
            // are their own targets.
            let name: Element<'_, Message> = match takeable {
                true => mouse_area(name)
                    .on_press(Message::DeviceAt(i))
                    .on_release(Message::PickDevice(device.id.clone()))
                    .into(),
                false => name.into(),
            };
            let mut controls = row![].spacing(2).align_y(Alignment::Center);
            if let Some(join) = device.join {
                let (glyph, message) = match join {
                    true => (glyphs::PLUS, Message::Join(device.id.clone())),
                    false => (glyphs::MINUS, Message::Unjoin(device.id.clone())),
                };
                controls = controls.push(button(icon::line(glyph, on_cursor)).style(button::text).padding(2).on_press(message));
            }
            if let Some(volume) = device.volume {
                let level = device.level();
                controls = controls
                    .push(mute_button(level.clone(), device.muted, on_cursor))
                    .push(volume_slider(level.clone(), self.slider_at(&level, volume)));
            }
            let entry = panel::entry(mark, name, Some(controls.into())).style(move |theme| style::entry(theme, on_cursor, true));
            rows = rows.push(mouse_area(entry).on_enter(Message::DeviceAt(i)));
        }
        panel::panel(column![panel::title("Playing on".into()), rows].spacing(0), DEVICES_WIDTH).into()
    }
}

/// How a catalogue writes who a work is by over its title: every name but
/// the last as an initial, a particle kept — `A. Vivaldi`, `J. S. Bach`,
/// `L. van Beethoven`.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let Some((last, first)) = words.split_last() else { return String::new() };
    let mut out: Vec<String> = first
        .iter()
        .map(|w| match w.chars().next() {
            Some(c) if c.is_uppercase() => format!("{c}."),
            _ => w.to_string(),
        })
        .collect();
    out.push(last.to_string());
    out.join(" ")
}

/// A page about something: its header, and what is on it.
/// A work's heading on an album: its composer in small capitals over its
/// title — each drawn by the caller, as the links they are — and how long the run of it under the heading lasts, in the column
/// the tracks' lengths are in.
fn work_heading<'a>(composer: Element<'a, Message>, work: Element<'a, Message>, ms: i64, first: bool) -> Element<'a, Message> {
    let words = column![composer, work].spacing(2).width(Length::Fill);
    let line = row![
        container(text("")).width(TRANSPORT),
        words,
        table::cell(clock(ms as f64 / 1000.0), TIME, false, false, true),
        // Where a track's menu button is, so the lengths line up.
        container(text("")).width(Length::Fixed(icon::SIZE + 12.0)),
    ]
    .align_y(Alignment::End);
    container(line)
        .width(Length::Fill)
        .padding(Padding {
            top: if first { 4.0 } else { 26.0 },
            right: table::ROW_PADDING[1],
            bottom: 6.0,
            left: table::ROW_PADDING[1],
        })
        .into()
}

/// The value most of `values` are, empties aside; the first of the most
/// common, so a tie goes to whoever came first.
pub fn most<'a>(values: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut counts: Vec<(&str, usize)> = vec![];
    for v in values.filter(|v| !v.is_empty()) {
        match counts.iter_mut().find(|(c, _)| *c == v) {
            Some((_, n)) => *n += 1,
            None => counts.push((v, 1)),
        }
    }
    counts
        .iter()
        .fold(None, |best: Option<(&str, usize)>, &(v, n)| match best {
            Some((_, b)) if b >= n => best,
            _ => Some((v, n)),
        })
        .map(|(v, _)| v)
}

fn with_header<'a>(header: Element<'a, Message>, body: Element<'a, Message>) -> Element<'a, Message> {
    column![header, body].spacing(0).width(Length::Fill).into()
}

/// The keymap: one that has to be read in the source is one nobody finds.
fn view_help() -> Element<'static, Message> {
    const KEYS: &[(&str, &str)] = &[
        ("j  k", "down, up"),
        ("h  l", "left, right \u{2014} out of a list, the next pane; in a menu, a submenu"),
        ("{n}j", "a count: 5j is five down"),
        ("gg  G", "first, last. 7G is the seventh"),
        ("^d  ^u", "a page down, a page up"),
        ("<Tab>", "swap between the sidebar and the page"),
        ("<Enter>  o", "in the sidebar, step into it; on a card, open it; in the table, play"),
        ("<Space>", "play or pause"),
        ("m", "this track's menu"),
        ("a", "which playlists this track is on \u{2014} and make one"),
        ("d", "which device is making the sound, and move it; each one's volume"),
        ("q", "the queue: <Enter> plays a row, x takes it out, J and K move it down and up"),
        (
            "+  -",
            "the volume of what is playing \u{2014} the group's, when it is one; in the picker, the row's",
        ),
        ("M", "mute it, or unmute it"),
        ("J", "in the picker: join this speaker to what is playing, or take it out"),
        ("D", "every number the session holds, and where your changes stand"),
        ("/", "search this pane; <Enter> accepts, <Esc> drops it"),
        ("n  N", "the next match, the one before"),
        ("{  }", "the previous track, the next one"),
        ("?", "this"),
    ];
    let rows = KEYS.iter().fold(column![].spacing(6), |col, (keys, what)| {
        col.push(row![text(*keys).size(13).width(Length::Fixed(110.0)), text(*what).size(13).style(style::dim)].spacing(12))
    });
    container(column![text("keys").size(16), rows].spacing(12))
        .padding(12)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whoever is credited on most of a record is who played it, a tie
    /// going to whoever came first and nobody to nobody. Falsified by
    /// counting the empties: the record with one credit had none.
    #[test]
    fn who_played_a_record_is_whoever_played_most_of_it() {
        assert_eq!(most(["", "", "Pinnock"].into_iter()), Some("Pinnock"));
        assert_eq!(most(["Hogwood", "Pinnock", "Pinnock"].into_iter()), Some("Pinnock"));
        assert_eq!(most(["Hogwood", "Pinnock"].into_iter()), Some("Hogwood"));
        assert_eq!(most(["", ""].into_iter()), None);
    }
}
