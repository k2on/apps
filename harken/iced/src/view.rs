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
                    column![self.view_top(), self.view_page()].spacing(10)
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
                    .map(|a| self.card(&a.name, a.creator.clone(), a.tracks, Source::Album(a.name.clone()), false, &a.art))
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
            Source::Album(name) if self.help || self.debug => {
                let album = p.albums().iter().find(|a| a.name == *name);
                let creator = album.map(|a| a.creator.clone()).unwrap_or_default();
                let open = (!creator.is_empty()).then(|| Source::Artist(creator.clone()));
                with_header(
                    self.view_record(name, &creator, open, None, album.map(|a| a.art.as_str()).unwrap_or_default()),
                    self.view_list(),
                )
            }
            Source::Album(name) => self.view_album(name),
            Source::Artist(name) => {
                let artist = p.artists().iter().find(|a| a.name == *name);
                let art_path = artist.map(|a| a.art.as_str()).unwrap_or_default();
                let albums = p.albums_by(name).len();
                let top = match artist.filter(|a| !a.banner.is_empty()) {
                    // A wide picture opens the page as a banner, the counts on
                    // it where a composer's years are.
                    Some(a) => {
                        let tracks = a.tracks;
                        let kind = format!(
                            "Artist  \u{b7}  {albums} {}  \u{b7}  {tracks} {}",
                            plural(albums as i64, "album"),
                            plural(tracks, "track")
                        );
                        container(hero::banner(
                            self.banners.handle(&self.art_url(&a.banner)),
                            &kind,
                            name,
                            self.pane_width(),
                        ))
                        .padding(Padding {
                            bottom: 12.0,
                            ..Padding::ZERO
                        })
                        .into()
                    }
                    None => self.view_record(name, "", None, Some(albums), art_path),
                };
                with_header(top, self.view_list())
            }
            // One composer's page, as a catalogue opens one: their banner, then
            // their works as a list in columns — the catalogue number under
            // each, the one name a work has that survives translation — then
            // their records.
            Source::Works(name) => self.view_composer(name),
            // One work and every performance of it: the rows are the same
            // music, and what tells them apart is who played it.
            Source::Work(id, title) => {
                let work = p.works().iter().find(|w| w.id == *id);
                let takes = p.recordings().len() as i64;
                let mut facts = match work.map(|w| w.catalogue.clone()).unwrap_or_default() {
                    c if c.is_empty() => String::new(),
                    c => format!("{c}  \u{b7}  "),
                };
                facts.push_str(&format!("{takes} {}", plural(takes, "recording")));
                if let Some(w) = work {
                    for fact in [&w.form, &w.period] {
                        if !fact.is_empty() {
                            facts.push_str(&format!("  \u{b7}  {fact}"));
                        }
                    }
                }
                let composer = work.map(|w| w.composer.clone()).unwrap_or_default();
                let open = (!composer.is_empty()).then(|| Source::Works(composer.clone()));
                with_header(
                    self.view_header(title, &composer, open, facts, work.map(|w| w.art.as_str()).unwrap_or_default(), false),
                    self.view_takes(),
                )
            }
            // One performance, in the order the work goes.
            Source::Recording(id, who) => {
                let take = p.recordings().iter().find(|r| r.id == *id);
                let rows = p.rows();
                let ms: i64 = rows.iter().map(|i| i.duration_ms).sum();
                let mut facts = format!("{} {}", rows.len(), plural(rows.len() as i64, "track"));
                if ms > 0 {
                    facts.push_str(&format!("  \u{b7}  {}", spell(ms)));
                }
                if let Some(r) = take {
                    for fact in [
                        (r.recorded > 0).then(|| r.recorded.to_string()),
                        (!r.label.is_empty()).then(|| r.label.clone()),
                        (!r.licence.is_empty()).then(|| r.licence.clone()),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        facts.push_str(&format!("  \u{b7}  {fact}"));
                    }
                }
                with_header(
                    self.view_header(who, "", None, facts, take.map(|r| r.art.as_str()).unwrap_or_default(), false),
                    self.view_list(),
                )
            }
            _ => self.view_list(),
        }
    }

    /// An album's page, laid out as a catalogue lays one out: the cover
    /// beside its title and who played it, then its tracks under the works
    /// they are movements of — each work's composer over its title, its
    /// length at its right — and at the foot when it came out, how long it
    /// runs and who put it out. One scrollable, so the cover scrolls away
    /// with the page. A record of no works (a pop record) is the same page
    /// with no work headings, and its track numbers instead.
    fn view_album(&self, name: &str) -> Element<'_, Message> {
        let p = &self.peer;
        let album = p.albums().iter().find(|a| a.name == name);
        let creator = album.map(|a| a.creator.clone()).unwrap_or_default();
        let rows = p.rows();
        let details: Vec<_> = rows.iter().map(|i| p.detail_of(i.id)).collect();
        let ms: i64 = rows.iter().map(|i| i.duration_ms).sum();
        let year = album.map_or(0, |a| a.released);

        // Who played it, in the accent: whoever is credited on the most of
        // it, which on a record by one orchestra is the orchestra. Nobody
        // credited, and it is whoever made it.
        let performer = most(details.iter().map(|d| d.performer.as_str()));
        let mut words = column![text(name.to_string()).size(26).font(hero::SERIF).wrapping(text::Wrapping::Word)]
            .spacing(4)
            .width(Length::Fill);
        let by = performer.map_or(creator.clone(), str::to_string);
        if !by.is_empty() {
            words = words.push(text(by).size(16).style(style::accent).wrapping(text::Wrapping::Word));
        }
        let small = [
            performer.is_some().then(|| creator.to_uppercase()).filter(|c| !c.is_empty()),
            (year > 0).then(|| year.to_string()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("  \u{b7}  ");
        if !small.is_empty() {
            let line = text(small).size(11).style(style::faint(0.5));
            words = words.push(match creator.is_empty() {
                true => Element::from(line),
                false => mouse_area(line).on_press(Message::Select(Source::Artist(creator.clone()))).into(),
            });
        }
        if let Some(first) = rows.first() {
            let play = row![icon::tinted(glyphs::PLAY, 12.0, |p| p.primary.base.text), text("Play").size(13),]
                .spacing(6)
                .align_y(Alignment::Center);
            words = words.push(
                container(button(play).style(style::action).padding([5, 14]).on_press(Message::PlayItem(first.id)))
                    .padding(Padding { top: 14.0, ..Padding::ZERO }),
            );
        }
        let side = if self.pane_width() < 560.0 { 140.0 } else { 220.0 };
        let art_path = album.map(|a| a.art.as_str()).unwrap_or_default();
        let header = container(
            row![self.picture(name, art_path, side, 8.0), words]
                .spacing(28)
                .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: PAGE_PADDING,
            bottom: 18.0,
            ..Padding::ZERO
        });

        let (playing_id, sounding) = self.highlighted().map_or((None, false), |(id, sounding)| (Some(id), sounding));
        let focused = self.has_keys(Pane::Tracks);
        let cursor = focused.then_some(self.at(Pane::Tracks));
        let track = |at: usize| -> Element<'_, Message> {
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
            // Under the title, who played it — or, on a record of several
            // people's songs, whose this one is.
            let under = match d.performer.is_empty() {
                false => credit(&d.performer, &d.licence),
                true if item.creator != creator => item.creator.clone(),
                true => String::new(),
            };
            let mut body = column![Line::new(item.title.clone(), 13.0, Cut::Middle, move |t| style::on_row(
                t, on_cursor, here, false
            ))]
            .spacing(1);
            if !under.is_empty() {
                body = body.push(Line::new(under, 11.0, Cut::End, move |t| {
                    style::on_row(t, on_cursor, false, true).scale_alpha(0.8)
                }));
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
        // The tracks in runs of one work, each run under its heading.
        let mut list = column![].spacing(0).width(Length::Fill);
        let mut at = 0;
        while at < rows.len() {
            let of = |d: &crate::rows::TrackDetail| (d.composer.clone(), d.work.clone());
            let run = of(&details[at]);
            let end = (at..rows.len()).find(|&j| of(&details[j]) != run).unwrap_or(rows.len());
            if !run.1.is_empty() {
                let ms: i64 = rows[at..end].iter().map(|i| i.duration_ms).sum();
                list = list.push(work_heading(&run.0, &run.1, ms, at == 0));
            }
            for j in at..end {
                list = list.push(track(j));
            }
            at = end;
        }
        if rows.is_empty() {
            list = list.push(text("Nothing on it yet.").size(13).style(style::faint(0.5)));
        }

        // The small print: when, how much, and whose label.
        let n = rows.len() as i64;
        let mut foot = column![].spacing(3);
        if year > 0 {
            foot = foot.push(text(year.to_string()).size(12).style(style::faint(0.5)));
        }
        let mut length = format!("{n} {}", plural(n, "track"));
        if ms > 0 {
            length.push_str(&format!(", {}", spell(ms)));
        }
        foot = foot.push(text(length).size(12).style(style::faint(0.5)));
        if let Some(label) = album.map(|a| a.label.clone()).filter(|l| !l.is_empty()) {
            foot = foot.push(
                column![
                    text("RECORD LABEL").size(10).style(style::faint(0.5)),
                    text(label).size(13).style(style::accent)
                ]
                .spacing(2)
                .padding(Padding { top: 14.0, ..Padding::ZERO }),
            );
        }
        let foot = container(foot).padding(Padding {
            top: 24.0,
            left: SECTION_INDENT,
            ..Padding::ZERO
        });

        let page = scrollable(container(column![header, list, foot].width(Length::Fill)).padding(Padding {
            right: SCROLLBAR + 6.0,
            bottom: PAGE_PADDING,
            ..Padding::ZERO
        }))
        .id(TRACKS)
        .style(style::bars)
        .height(Length::Fill);
        column![page, self.view_status()].spacing(10).width(Length::Fill).into()
    }

    /// A composer's page: the banner (or the header, for somebody with no
    /// wide picture), their works, and the records their works are on — all
    /// one scrollable, so the banner scrolls away with the page.
    fn view_composer(&self, name: &str) -> Element<'_, Message> {
        let p = &self.peer;
        let composer = p.composers().iter().find(|c| c.name == name);
        let life = composer.map(|c| span(c.born, c.died)).unwrap_or_default();
        let works = p.works().len() as i64;
        let tracks: i64 = p.works().iter().map(|w| w.tracks).sum();
        let top: Element<'_, Message> = match composer.filter(|c| !c.banner.is_empty()) {
            Some(c) => {
                let kind = match life.is_empty() {
                    true => "Composer".to_string(),
                    false => format!("Composer  \u{b7}  {life}"),
                };
                hero::banner(self.banners.handle(&self.art_url(&c.banner)), &kind, name, self.pane_width())
            }
            None => {
                let facts = format!("{works} {}  \u{b7}  {tracks} {}", plural(works, "work"), plural(tracks, "track"));
                let art_path = composer.map(|c| c.art.as_str()).unwrap_or_default();
                self.view_header(name, &life, None, facts, art_path, true)
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
        let cursor = self.has_keys(Pane::Tracks).then_some(self.at(Pane::Tracks));
        let mut page = column![top, hero::heading("Works")].width(Length::Fill);
        page = match entries.is_empty() {
            true => page.push(text("Nothing by them yet.").size(13).style(style::faint(0.5))),
            false => page.push(hero::listing(&entries, self.listing_columns(), cursor, Message::HoverAt)),
        };
        let albums: Vec<cards::Card<Message>> = p
            .albums_by(name)
            .into_iter()
            .map(|a| self.card(&a.name, String::new(), a.tracks, Source::Album(a.name.clone()), false, &a.art))
            .collect();
        if !albums.is_empty() {
            page = page.push(hero::heading("Albums"));
            let mut shelf = column![].spacing(SHELF.gap);
            for chunk in albums.chunks(self.columns()) {
                let line = chunk.iter().map(|c| cards::view_card(SHELF, c, false, Message::Swallow));
                shelf = shelf.push(Row::with_children(line).spacing(SHELF.gap).align_y(Alignment::Start));
            }
            page = page.push(shelf);
        }
        scrollable(container(page).padding(Padding {
            right: SCROLLBAR + 6.0,
            bottom: PAGE_PADDING,
            ..Padding::ZERO
        }))
        .id(TRACKS)
        .style(style::bars)
        .height(Length::Fill)
        .into()
    }

    /// The recordings of one work, as rows rather than cards: every one has
    /// the same picture and the same title, so a grid of them would be a grid
    /// of identical squares. What tells them apart is text.
    fn view_takes(&self) -> Element<'_, Message> {
        let p = &self.peer;
        if p.recordings().is_empty() {
            return cards::empty_page("No recordings of it yet.", PAGE_PADDING);
        }
        let focused = self.has_keys(Pane::Tracks);
        let cursor = focused.then_some(self.at(Pane::Tracks));
        let mut list = column![];
        for (at, take) in p.recordings().iter().enumerate() {
            let on_cursor = Some(at) == cursor;
            // A recording nobody is credited on — the demo has one — says so,
            // rather than guessing who played it.
            let who = match take.performers.is_empty() {
                true => "Performer not named".to_string(),
                false => take.performers.clone(),
            };
            let mut facts = format!("{} {}", take.tracks, plural(take.tracks, "track"));
            for fact in [
                (take.recorded > 0).then(|| take.recorded.to_string()),
                (!take.label.is_empty()).then(|| take.label.clone()),
                (!take.licence.is_empty()).then(|| take.licence.clone()),
            ]
            .into_iter()
            .flatten()
            {
                facts.push_str(&format!("  \u{b7}  {fact}"));
            }
            list = list.push(
                mouse_area(
                    container(
                        column![
                            text(who).size(13).style(move |theme| text::Style {
                                color: Some(style::entry_text(theme, on_cursor)),
                            }),
                            text(facts).size(11).style(move |theme| {
                                let p = arkui::theme::of(theme);
                                text::Style {
                                    color: Some(match on_cursor {
                                        true => p.primary.base.text.scale_alpha(0.7),
                                        false => p.background.base.text.scale_alpha(0.5),
                                    }),
                                }
                            }),
                        ]
                        .spacing(2),
                    )
                    .padding(Padding {
                        top: 8.0,
                        right: PAGE_PADDING,
                        bottom: 8.0,
                        left: PAGE_PADDING,
                    })
                    .width(Length::Fill)
                    .style(move |theme| style::row(theme, on_cursor, focused, at % 2 == 1)),
                )
                .on_enter(Message::HoverAt(at))
                .on_press(Message::Select(Source::Recording(take.id.clone(), take.performers.clone()))),
            );
        }
        container(scrollable(list).id(TRACKS).style(style::bars))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
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
        let titled = !matches!(p.source, Source::Album(_) | Source::Artist(_) | Source::Recording(..));
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
    fn view_status(&self) -> Element<'_, Message> {
        let p = &self.peer;
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
            text(mode).size(12).style(style::dim),
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
/// title, and how long the run of it under the heading lasts, in the column
/// the tracks' lengths are in.
fn work_heading<'a>(composer: &str, work: &str, ms: i64, first: bool) -> Element<'a, Message> {
    let words = column![
        text(composer.to_uppercase()).size(10).style(style::faint(0.5)),
        text(work.to_string()).size(15).font(hero::SERIF).wrapping(text::Wrapping::Word),
    ]
    .spacing(2)
    .width(Length::Fill);
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
fn most<'a>(values: impl Iterator<Item = &'a str>) -> Option<&'a str> {
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
