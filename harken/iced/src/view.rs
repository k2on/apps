//! What the window draws: the sidebar, the page the sidebar picked, the
//! now-playing bar, and the layers over them.
//!
//! Nothing here writes a colour down: every one is asked of
//! `arkui::theme::of`, which answers with harken's palette for whichever
//! theme iced picked. And nothing here decides anything — which grid has the
//! keyboard, where a panel goes, how many cards fit are all answered in
//! `main.rs`, the same numbers the keyboard uses.
use arkui::format::{clock, plural, span, spell};
use arkui::{art, cards, fit, glyphs, icon, layer, menu, panel, picker, style, table};
use iced::widget::{button, column, container, mouse_area, row, rule, scrollable, slider, stack, text, Row};
use iced::{Alignment, Element, Length, Padding};

use crate::places::{Focus, Pane, Source};
use crate::player::Player;
use crate::{credit, App, Message, BAR_HEIGHT, DEVICES_WIDTH, PAGE_PADDING, SHELF, SIDEBAR, SIDEBAR_WIDTH, TRACKS};

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

/// What the now-playing bar draws, from wherever it came: this device's
/// element, or what the session says another device is doing. One struct, so
/// a view that branched on that twice cannot branch differently.
struct Bar {
    title: String,
    creator: String,
    playing: bool,
    position: f64,
    duration: f64,
}

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

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let base = container(
            column![
                // The sidebar and the page share the height left once the bar
                // has taken its own, so the bar stays at the bottom.
                row![self.view_sidebar(), rule::vertical(1), self.view_page()]
                    .spacing(16)
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

        if let Some(at) = self.devices {
            let origin = App::devices_origin(self.window, self.listening.devices().len() + 1);
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
                p.albums
                    .iter()
                    .map(|a| self.card(&a.name, a.creator.clone(), a.tracks, Source::Album(a.name.clone()), false, &a.art))
                    .collect(),
                "No albums yet.",
            ),
            // A person is a circle and a record is a square.
            Source::Artists => self.view_cards(
                p.artists
                    .iter()
                    .map(|a| self.card(&a.name, String::new(), a.tracks, Source::Artist(a.name.clone()), true, &a.art))
                    .collect(),
                "Nobody yet.",
            ),
            Source::Composers => self.view_cards(
                p.composers
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
            Source::Album(name) => {
                let album = p.albums.iter().find(|a| a.name == *name);
                let creator = album.map(|a| a.creator.clone()).unwrap_or_default();
                let open = (!creator.is_empty()).then(|| Source::Artist(creator.clone()));
                with_header(
                    self.view_record(name, &creator, open, None, album.map(|a| a.art.as_str()).unwrap_or_default()),
                    self.view_list(),
                )
            }
            Source::Artist(name) => {
                let art_path = p.artists.iter().find(|a| a.name == *name).map(|a| a.art.as_str()).unwrap_or_default();
                with_header(
                    self.view_record(name, "", None, Some(p.albums_by(name).len()), art_path),
                    self.view_list(),
                )
            }
            // One composer's works, as cards: the catalogue number under each,
            // the one name a work has that survives translation.
            Source::Works(name) => {
                let composer = p.composers.iter().find(|c| c.name == *name);
                let works = p.works.len() as i64;
                let tracks: i64 = p.works.iter().map(|w| w.tracks).sum();
                let facts = format!("{works} {}  \u{b7}  {tracks} {}", plural(works, "work"), plural(tracks, "track"));
                let life = composer.map(|c| span(c.born, c.died)).unwrap_or_default();
                let cards = p
                    .works
                    .iter()
                    .map(|w| {
                        self.card(
                            &w.title,
                            w.catalogue.clone(),
                            w.tracks,
                            Source::Work(w.id.clone(), w.title.clone()),
                            false,
                            &w.art,
                        )
                    })
                    .collect();
                with_header(
                    self.view_header(name, &life, None, facts, composer.map(|c| c.art.as_str()).unwrap_or_default(), true),
                    self.view_cards(cards, "Nothing by them yet."),
                )
            }
            // One work and every performance of it: the rows are the same
            // music, and what tells them apart is who played it.
            Source::Work(id, title) => {
                let work = p.works.iter().find(|w| w.id == *id);
                let takes = p.recordings.len() as i64;
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
                let take = p.recordings.iter().find(|r| r.id == *id);
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

    /// The recordings of one work, as rows rather than cards: every one has
    /// the same picture and the same title, so a grid of them would be a grid
    /// of identical squares. What tells them apart is text.
    fn view_takes(&self) -> Element<'_, Message> {
        let p = &self.peer;
        if p.recordings.is_empty() {
            return cards::empty_page("No recordings of it yet.", PAGE_PADDING);
        }
        let focused = self.has_keys(Pane::Tracks);
        let cursor = focused.then_some(self.at(Pane::Tracks));
        let mut list = column![];
        for (at, take) in p.recordings.iter().enumerate() {
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
        let playing_id = self.player.track().map(|t| t.id);
        let sounding = self.player.is_playing();
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
                .push(table::cell(item.creator.clone(), ARTIST, on_cursor, false, true))
                .push(table::cell(
                    // …and the terms it was given on, beside whoever gave it:
                    // a credit nobody draws is a condition nobody met.
                    match album_page {
                        true => credit(&detail.performer, &detail.licence),
                        false => detail.album.clone(),
                    },
                    ALBUM,
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
                    text(p.source.title().to_string()).size(22),
                    text(format!("{} {}", p.rows().len(), plural(p.rows().len() as i64, "track")))
                        .size(12)
                        .style(style::dim),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            );
        }
        if let Some(actions) = self.view_actions() {
            main = main.push(actions);
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

    /// Signing in and out, and going offline. The demo has none of it.
    fn view_actions(&self) -> Option<Element<'_, Message>> {
        if cfg!(feature = "demo") {
            return None;
        }
        let sign_in = |label: &'static str| {
            button(label)
                .style(style::action)
                .on_press_maybe((!self.signing_in).then_some(Message::SignIn))
        };
        let actions: Element<'_, Message> = match &self.login {
            // Nobody yet: everything works, and this is how it starts syncing.
            None => row![
                sign_in(if self.signing_in { "signing in\u{2026}" } else { "sign in" }),
                text("not signed in \u{2014} what you do is kept on this device")
                    .size(12)
                    .style(style::dim),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
            // Turned away by the server: the one button that helps.
            Some(login) if login.token.is_empty() => row![
                sign_in("sign in again"),
                button("sign out").style(button::text).on_press(Message::SignOut)
            ]
            .spacing(12)
            .into(),
            Some(_) => row![
                button(if self.offline { "go online" } else { "go offline" })
                    .style(style::action)
                    .on_press(Message::ToggleLink),
                button("sign out").style(button::text).on_press(Message::SignOut),
            ]
            .spacing(12)
            .into(),
        };
        Some(actions)
    }

    /// The engine showing through: how much of the log is applied, what this
    /// peer has done that nobody has confirmed, the last thing worth saying —
    /// and which grid has the keys, with anything half-typed.
    fn view_status(&self) -> Element<'_, Message> {
        let p = &self.peer;
        let mut line = format!(
            "{} songs \u{b7} cursor {} \u{b7} {} pending",
            p.items.len(),
            p.client.cursor(),
            p.client.pending_len()
        );
        if !cfg!(feature = "demo") {
            let who = match &self.login {
                Some(l) => crate::auth::who(l),
                None => "nobody".into(),
            };
            let link = match (&self.login, p.client.linked()) {
                (None, _) => "not signed in",
                (Some(_), true) => "online",
                (Some(_), false) => "offline",
            };
            line = format!("{who} \u{b7} {link} \u{b7} {line}");
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
        let status = self.peer.client.status();
        let session = l.session();
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
            ("audible".into(), yes(Player::AUDIBLE).into()),
            (String::new(), String::new()),
            ("link".into(), status.link.clone()),
            ("client linked".into(), yes(status.linked).into()),
            (
                "epoch \u{b7} introduced on".into(),
                format!("{} \u{b7} {}", status.epoch, l.introduced_on()),
            ),
            (
                "said here \u{b7} report \u{b7} do \u{b7} transfer".into(),
                format!(
                    "{} \u{b7} {} \u{b7} {} \u{b7} {}",
                    stats.said_here, stats.said_report, stats.said_do, stats.said_transfer
                ),
            ),
            ("outbox".into(), l.outbox().to_string()),
            ("heard frames (wire)".into(), status.heard_frames.to_string()),
            (
                "heard state \u{b7} do (decoded)".into(),
                format!("{} \u{b7} {}", stats.heard_state, stats.heard_do),
            ),
            (
                "last state".into(),
                match l.state_age_ms() {
                    Some(ms) => format!("{:.1}s ago", ms / 1000.0),
                    None => "never".into(),
                },
            ),
            (String::new(), String::new()),
            ("session".into(), yes(session.is_some()).into()),
            (
                "output \u{b7} moving".into(),
                format!(
                    "{} \u{b7} {}",
                    session.and_then(|s| s.output.as_deref()).unwrap_or("none"),
                    session.and_then(|s| s.moving.as_deref()).unwrap_or("none"),
                ),
            ),
            (
                "here \u{b7} elsewhere".into(),
                format!("{} \u{b7} {}", yes(l.outputs_here()), yes(l.elsewhere())),
            ),
            (
                "playing \u{b7} at \u{b7} queue \u{b7} position".into(),
                match session {
                    Some(s) => format!(
                        "{} \u{b7} {} \u{b7} {} \u{b7} {}",
                        yes(s.playing),
                        s.at,
                        s.queue.len(),
                        clock(l.position_ms() as f64 / 1000.0)
                    ),
                    None => "\u{2014}".into(),
                },
            ),
            (
                "player".into(),
                format!(
                    "{} \u{b7} {}",
                    self.player.track().map(|t| t.title.as_str()).unwrap_or("nothing loaded"),
                    if self.player.is_playing() { "playing" } else { "paused" }
                ),
            ),
            (
                "devices".into(),
                match l.devices().len() {
                    0 => "none".into(),
                    n => n.to_string(),
                },
            ),
        ];
        for d in l.devices() {
            rows.push((
                format!("  {}", d.name),
                format!(
                    "{} \u{b7} {:?} \u{b7} audible {} \u{b7} here {}",
                    d.id,
                    d.kind,
                    yes(d.audible),
                    yes(d.here)
                ),
            ));
        }
        rows.push((String::new(), String::new()));
        rows.push((
            "log cursor \u{b7} pending \u{b7} songs".into(),
            format!("{} \u{b7} {} \u{b7} {}", status.cursor, status.pending, self.peer.items.len()),
        ));
        // What this window did, and what became of each: pending, in the log,
        // or refused and why — the sentence every replica reaches.
        for e in self.edits.iter().rev().take(12) {
            let standing = match self.peer.client.standing(&e.id) {
                ark_client::Standing::Pending => "pending".to_string(),
                ark_client::Standing::Confirmed => "in the log".to_string(),
                ark_client::Standing::Rejected(why) => format!("refused: {why}"),
                ark_client::Standing::Unknown => "unknown".to_string(),
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

    /// What the bar is drawing, whichever device is making it: the *session*,
    /// so a laptop watching a phone draws the phone's track, and the local
    /// player when the sound is here (or nowhere yet), a frame fresher than
    /// any broadcast.
    fn bar(&self) -> Option<Bar> {
        if self.listening.elsewhere() {
            let track = self.listening.now()?;
            return Some(Bar {
                title: track.title.clone(),
                creator: track.creator.clone(),
                playing: self.listening.playing(),
                position: self.listening.position_ms() as f64 / 1000.0,
                duration: track.duration_ms as f64 / 1000.0,
            });
        }
        let track = self.player.track()?;
        Some(Bar {
            title: track.title.clone(),
            creator: track.creator.clone(),
            playing: self.player.is_playing(),
            position: self.player.position(),
            duration: self.player.duration(),
        })
    }

    /// Which device has the sound, as the button that opens the picker —
    /// drawn only where there is a session to draw.
    fn view_output(&self) -> Option<Element<'_, Message>> {
        if !self.listening.live() {
            return None;
        }
        let session = self.listening.session()?;
        let here = self.listening.outputs_here();
        // Three things a device can be: a hand-off in flight, the output and
        // answering, the output and not answering (the sound stays with it).
        let label = match (session.moving.as_deref(), session.output_device()) {
            (Some(id), _) => format!("{}\u{2026}", fit::middle(session.device(id).map_or(id, |d| d.name.as_str()), 14)),
            (None, Some(_)) if here => "this device".to_string(),
            (None, Some(device)) if !device.here => format!("{} (away)", fit::middle(&device.name, 12)),
            (None, Some(device)) => fit::middle(&device.name, 16),
            (None, None) => "no device".to_string(),
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

    /// The now-playing bar, honest about silence: a build with no audio
    /// device says so rather than drawing a transport that does nothing.
    fn view_bar(&self) -> Element<'_, Message> {
        let Some(bar) = self.bar() else {
            let idle = text(match Player::AUDIBLE {
                true => "nothing playing \u{2014} pick a track",
                false => "nothing playing \u{2014} pick a track (the desktop build has no audio device; the browser one streams)",
            })
            .size(12)
            .style(style::dim);
            let mut line = row![idle].spacing(12).align_y(Alignment::Center);
            if let Some(output) = self.view_output() {
                line = line.push(container(text("")).width(Length::Fill)).push(output);
            }
            return container(line)
                .padding([8, 4])
                .height(Length::Fixed(BAR_HEIGHT))
                .align_y(Alignment::Center)
                .into();
        };
        let duration = bar.duration.max(0.1);
        // Enabled whenever there is a session to send it to: being a remote
        // control is a use, and `AUDIBLE` only decides whether *this* device
        // can be the one playing.
        let workable = Player::AUDIBLE || self.listening.elsewhere();
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
            column![text(bar.title).size(14), text(bar.creator).size(12).style(style::dim)]
                .spacing(2)
                .width(Length::Fixed(260.0)),
            text(clock(bar.position)).size(11).style(style::dim),
            slider(0.0..=duration as f32, bar.position as f32, Message::Seek)
                .style(style::seek)
                .width(Length::Fill),
            text(clock(duration)).size(11).style(style::dim),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
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
    /// this account, and a last row that stops it everywhere — the playlist
    /// picker's shape, so `j` reaches it without a second key to learn.
    fn view_devices(&self, at: usize) -> Element<'_, Message> {
        let devices = self.listening.devices();
        let output = self.listening.session().and_then(|s| s.output.as_deref()).unwrap_or("");
        let mut rows = column![].spacing(0);
        for (i, device) in devices.iter().enumerate() {
            let on_cursor = at == i;
            // Drawn, and not a target, when it cannot be heard or is not
            // answering: a laptop controlling the session should see itself
            // listed, and the device the music still belongs to is the one
            // most worth drawing.
            let takeable = device.audible && device.here;
            let label = if device.id == self.listening.me() {
                format!("{} (this one)", fit::middle(&device.name, 14))
            } else if self.listening.moving() == Some(device.id.as_str()) {
                format!("{}\u{2026}", fit::middle(&device.name, 20))
            } else if !device.here {
                format!("{} (not answering)", fit::middle(&device.name, 14))
            } else {
                fit::middle(&device.name, 24)
            };
            let mark = (device.id == output).then(|| Element::from(icon::tick(on_cursor)));
            let entry = panel::entry(
                mark,
                text(label)
                    .size(13)
                    .style(move |theme| text::Style {
                        color: Some(match (on_cursor, takeable) {
                            (false, false) => arkui::theme::of(theme).background.base.text.scale_alpha(0.4),
                            (lit, _) => style::entry_text(theme, lit),
                        }),
                    })
                    .into(),
                None,
            )
            .style(move |theme| style::entry(theme, on_cursor, true));
            let entry: Element<'_, Message> = match takeable {
                true => mouse_area(entry)
                    .on_enter(Message::DeviceAt(i))
                    .on_press(Message::DeviceAt(i))
                    .on_release(Message::PickDevice(Some(device.id.clone())))
                    .into(),
                false => entry.into(),
            };
            rows = rows.push(entry);
        }
        let last = devices.len();
        let on_cursor = at == last;
        rows = rows.push(
            mouse_area(
                panel::entry(
                    None,
                    text("Stop everywhere")
                        .size(13)
                        .style(move |theme| text::Style {
                            color: Some(match on_cursor {
                                true => arkui::theme::of(theme).primary.base.text,
                                false => arkui::theme::of(theme).background.base.text.scale_alpha(0.7),
                            }),
                        })
                        .into(),
                    None,
                )
                .style(move |theme| style::entry(theme, on_cursor, true)),
            )
            .on_enter(Message::DeviceAt(last))
            .on_press(Message::DeviceAt(last))
            .on_release(Message::PickDevice(None)),
        );
        panel::panel(column![panel::title("Playing on".into()), rows].spacing(0), DEVICES_WIDTH).into()
    }
}

/// A page about something: its header, and what is on it.
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
        ("d", "which device is making the sound, and move it"),
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
