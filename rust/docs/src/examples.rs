//! One working example per module, built from arkui exactly as an app would
//! build it. The overlays (menu, picker, dialog) open the window's own layers
//! in `main.rs`, so they behave here as they do in an app.
use arkui::account::{self, Account, Who};
use arkui::cards::{self, Card};
use arkui::fit::Cut;
use arkui::line::Line;
use arkui::{art, format, glyphs, hero, icon, panel, sidebar, style, table, theme, url};
use iced::widget::{button, column, container, mouse_area, row, slider, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length};

use crate::{App, Message, Place};

/// Every glyph, by the name `arkui::glyphs` gives it.
pub const GLYPHS: &[(&str, &[u8])] = &[
    ("ADD_TO", glyphs::ADD_TO),
    ("ALBUM", glyphs::ALBUM),
    ("ARTIST", glyphs::ARTIST),
    ("BACK", glyphs::BACK),
    ("CHEVRON", glyphs::CHEVRON),
    ("CLOSE", glyphs::CLOSE),
    ("COMPOSER", glyphs::COMPOSER),
    ("DEBUG", glyphs::DEBUG),
    ("DEVICES", glyphs::DEVICES),
    ("DOWN", glyphs::DOWN),
    ("HOME", glyphs::HOME),
    ("LAPTOP", glyphs::LAPTOP),
    ("LIBRARY", glyphs::LIBRARY),
    ("MINUS", glyphs::MINUS),
    ("MORE", glyphs::MORE),
    ("MUTED", glyphs::MUTED),
    ("NEXT", glyphs::NEXT),
    ("NOTE", glyphs::NOTE),
    ("OFFLINE", glyphs::OFFLINE),
    ("ONLINE", glyphs::ONLINE),
    ("PAUSE", glyphs::PAUSE),
    ("PHONE", glyphs::PHONE),
    ("PLAY", glyphs::PLAY),
    ("PLAYING", glyphs::PLAYING),
    ("PLAYLIST", glyphs::PLAYLIST),
    ("PLUS", glyphs::PLUS),
    ("PREVIOUS", glyphs::PREVIOUS),
    ("PULPIT", glyphs::PULPIT),
    ("SEARCH", glyphs::SEARCH),
    ("SHOW", glyphs::SHOW),
    ("SHUFFLE", glyphs::SHUFFLE),
    ("SIGN_OUT", glyphs::SIGN_OUT),
    ("SKIP_BACK", glyphs::SKIP_BACK),
    ("SKIP_FORWARD", glyphs::SKIP_FORWARD),
    ("SPEAKER", glyphs::SPEAKER),
    ("STOP", glyphs::STOP),
    ("TICK", glyphs::TICK),
    ("TICKED", glyphs::TICKED),
    ("UNTICK", glyphs::UNTICK),
    ("VOLUME", glyphs::VOLUME),
];

/// Names to draw with: long enough to be cut, and with the characters a
/// typeface may lack.
const TRACKS: [(&str, &str, &str, &str); 4] = [
    ("Aria", "Glenn Gould", "Goldberg Variations, BWV 988", "3:05"),
    ("Variatio 1. a 1 Clav.", "Glenn Gould", "Goldberg Variations, BWV 988", "1:45"),
    (
        "Symphony No. 9 in D minor, Op. 125 \"Choral\": IV. Presto",
        "Wiener Philharmoniker",
        "Beethoven: Symphony No. 9",
        "24:12",
    ),
    ("Clair de lune", "Claudio Arrau", "Debussy: Suite bergamasque", "5:21"),
];

/// The example a component's Desktop half draws: its modules', together.
pub fn for_component<'a>(app: &'a App, id: &str) -> Element<'a, Message> {
    let both = |a: Element<'a, Message>, b: Element<'a, Message>| column![a, b].spacing(24).into();
    match id {
        "icons" => both(glyphs_(room(app)), icon_(room(app))),
        "text" => both(cut(room(app)), format_()),
        "art" => both(art_(room(app)), said(app, "images")),
        "navigation" => sidebar_(app),
        "rows" => table_(app),
        "header" => hero_(app),
        "dialog" => both(dialog_(), picker_(app)),
        "pages" => said(app, "swipe"),
        "keys" => keys(app),
        "routes" => both(route_(app), url_()),
        "waking" => wake_(app),
        other => view(app, other),
    }
}

pub fn view<'a>(app: &'a App, module: &str) -> Element<'a, Message> {
    match module {
        "theme" => theme(room(app)),
        "style" => style_(app),
        "glyphs" => glyphs_(room(app)),
        "icon" => icon_(room(app)),
        "format" => format_(),
        "fit" | "line" => cut(room(app)),
        "url" => url_(),
        "sidebar" => sidebar_(app),
        "panel" => panel_(),
        "table" => table_(app),
        "cards" => cards_(app),
        "hero" => hero_(app),
        "art" => art_(room(app)),
        "account" => account_(room(app)),
        "menu" | "context" => menu_(app),
        "picker" | "layer" => picker_(app),
        "dialog" => dialog_(),
        "vim" => keys(app),
        "route" => route_(app),
        "wake" => wake_(app),
        _ => said(app, module),
    }
}

/// `items`, each `item` wide, in as many rows as `room` needs: an example
/// on a phone's width wraps rather than running off its box.
fn wrap<'a>(items: Vec<Element<'a, Message>>, item: f32, gap: f32, room: f32) -> Element<'a, Message> {
    let per = (((room + gap) / (item + gap)).floor() as usize).max(1);
    let mut rows = column![].spacing(gap);
    let mut items = items.into_iter().peekable();
    while items.peek().is_some() {
        rows = rows.push(
            items
                .by_ref()
                .take(per)
                .fold(row![].spacing(gap).align_y(Alignment::Start), |r, e| r.push(e)),
        );
    }
    rows.into()
}

/// How wide an example may be: the page's content less the example box's
/// padding and outline.
fn room(app: &App) -> f32 {
    app.content() - 42.0
}

fn label<'a>(s: impl Into<String>) -> Element<'a, Message> {
    text(s.into()).size(11).style(style::faint(0.6)).into()
}

fn swatch<'a>(name: &'static str, colour: fn(&iced::Theme) -> Color) -> Element<'a, Message> {
    column![
        container(Space::new()).width(56).height(36).style(move |t| container::Style {
            background: Some(Background::Color(colour(t))),
            border: Border {
                color: style::edge_color(t),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..container::Style::default()
        }),
        label(name),
    ]
    .spacing(4)
    .into()
}

fn theme<'a>(room: f32) -> Element<'a, Message> {
    let colours = wrap(
        vec![
            swatch("background", |t| theme::of(t).background.base.color),
            swatch("weak", |t| theme::of(t).background.weak.color),
            swatch("strong", |t| theme::of(t).background.strong.color),
            swatch("surface", style::surface_color),
            swatch("text", |t| theme::of(t).background.base.text),
            swatch("primary", |t| theme::of(t).primary.base.color),
            swatch("danger", |t| theme::of(t).danger.base.color),
        ],
        64.0,
        12.0,
        room,
    );
    let squares = wrap(
        ["Bach", "Debussy", "Gould", "Arrau", "Callas", "Satie"]
            .iter()
            .map(|seed| column![art::square(seed, 48.0, 6.0), label(*seed)].spacing(4).into())
            .collect(),
        48.0,
        12.0,
        room,
    );
    column![
        colours,
        label("The art pairs, picked by name:"),
        squares,
        label("Switch the theme from the dropdown at the top right.")
    ]
    .spacing(14)
    .into()
}

fn style_(app: &App) -> Element<'_, Message> {
    let tones = row![
        text("plain").size(13),
        text("dim").size(13).style(style::dim),
        text("faint(0.5)").size(13).style(style::faint(0.5)),
        text("accent").size(13).style(style::accent),
    ]
    .spacing(16);
    let buttons = row![
        button(text("action").size(12)).style(style::action).on_press(Message::Ran("action")),
        button(text("quiet").size(12)).style(style::quiet).on_press(Message::Ran("quiet")),
        slider(0.0..=100.0, app.side as f32 * 20.0, |v| Message::SideAt((v / 20.0) as usize))
            .style(style::seek)
            .width(160),
    ]
    .spacing(16)
    .align_y(Alignment::Center);
    let rows = (0..4).fold(column![], |col, i| {
        let on = i == app.row;
        col.push(
            mouse_area(
                table::row(
                    text(format!("style::row — row {i}{}", if on { ", the cursor" } else { "" }))
                        .size(13)
                        .style(move |t| text::Style {
                            color: Some(style::on_row(t, on, false, false)),
                        }),
                    on,
                    true,
                    i % 2 == 1,
                )
                .width(room(app).min(360.0)),
            )
            .on_enter(Message::RowAt(i)),
        )
    });
    column![tones, buttons, label("Rows: the zebra, and the cursor under the pointer"), rows]
        .spacing(14)
        .into()
}

fn glyphs_<'a>(room: f32) -> Element<'a, Message> {
    let mut grid = column![].spacing(14);
    for chunk in GLYPHS.chunks((((room + 8.0) / 104.0) as usize).clamp(1, 6)) {
        let line = chunk.iter().fold(row![].spacing(8), |r, (name, shape)| {
            r.push(
                column![icon::tinted(shape, 20.0, |p| p.background.base.text), label(*name)]
                    .spacing(6)
                    .align_x(Alignment::Center)
                    .width(96),
            )
        });
        grid = grid.push(line);
    }
    grid.into()
}

fn icon_<'a>(room: f32) -> Element<'a, Message> {
    let tone = |name: &'static str, drawn: iced::widget::Svg<'a>| column![drawn, label(name)].spacing(6).align_x(Alignment::Center).width(110);
    let tones: Vec<Element<'a, Message>> = vec![
        tone("plain", icon::plain(glyphs::PLAY, false)).into(),
        tone("plain, dim", icon::plain(glyphs::PLAY, true)).into(),
        tone("accent", icon::accent(glyphs::PLAY, false)).into(),
        tone("line", icon::line(glyphs::PLAY, false)).into(),
        tone("faint", icon::faint(glyphs::PLAY, false)).into(),
        tone("tick", icon::tick(false)).into(),
        tone("more", icon::more(false)).into(),
        tone("chevron", icon::chevron(false)).into(),
    ];
    wrap(tones, 110.0, 4.0, room)
}

fn pair<'a>(call: &str, answer: String) -> Element<'a, Message> {
    row![
        text(call.to_string()).size(13).style(style::dim).width(Length::FillPortion(1)),
        text(answer).size(13).width(Length::FillPortion(1))
    ]
    .spacing(8)
    .into()
}

fn format_<'a>() -> Element<'a, Message> {
    column![
        pair("clock(261.0)", format::clock(261.0)),
        pair("clock(3725.0)", format::clock(3725.0)),
        pair("spell(3_725_000)", format::spell(3_725_000)),
        pair("plural(1, \"song\")", format::plural(1, "song")),
        pair("plural(12, \"song\")", format::plural(12, "song")),
        pair("span(1685, 1750)", format::span(1685, 1750)),
    ]
    .spacing(6)
    .into()
}

fn cut<'a>(room: f32) -> Element<'a, Message> {
    let long = "Goldberg Variations, BWV 988: Variatio 25. a 2 Clav. (Adagio)";
    let at = |w: f32, c: Cut| {
        row![
            text(format!("{w}px")).size(11).style(style::faint(0.6)).width(52),
            container(Line::new(long, 13.0, c, |t| theme::of(t).background.base.text))
                .width(w)
                .style(style::outline(4.0))
                .padding([2, 6]),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    };
    column![
        label("Cut::Middle — a name"),
        at((room - 60.0).min(420.0), Cut::Middle),
        at(240.0, Cut::Middle),
        at(140.0, Cut::Middle),
        label("Cut::End — a heading"),
        at(240.0, Cut::End),
        at(140.0, Cut::End),
        label(format!("fit::middle(…, 24) = {}", arkui::fit::middle(long, 24))),
    ]
    .spacing(8)
    .into()
}

fn url_<'a>() -> Element<'a, Message> {
    let name = "Bach/Goldberg #1 · é";
    column![
        pair("encoded(\"Bach/Goldberg #1 · é\")", url::encoded(name)),
        pair("decode(encoded(…))", url::decode(&url::encoded(name))),
        pair("route::join(\"album\", Some(…))", arkui::route::join("album", Some(name))),
    ]
    .spacing(6)
    .into()
}

fn sidebar_(app: &App) -> Element<'_, Message> {
    let places = [
        (glyphs::NOTE, "Songs", Some("Music"), Some(1204)),
        (glyphs::ALBUM, "Albums", Some("Music"), Some(96)),
        (glyphs::ARTIST, "Artists", Some("Music"), Some(41)),
        (glyphs::SHOW, "Podcasts", Some("Shows"), Some(3)),
        (glyphs::PULPIT, "Sermons", Some("Shows"), None),
        (glyphs::PLAYLIST, "Favorites", Some("Playlists"), Some(18)),
    ];
    let items: Vec<sidebar::Item<Message>> = places
        .iter()
        .enumerate()
        .map(|(i, (glyph, name, heading, count))| sidebar::Item {
            glyph,
            label: name.to_string(),
            heading: *heading,
            count: *count,
            press: Message::SideAt(i),
        })
        .collect();
    let bar = container(sidebar::view(&items, app.side, true, sidebar::WIDTH, 8.0, "example-sidebar")).height(300);
    let page = container(text(format!("The page for {}", places.get(app.side).map(|p| p.1).unwrap_or("—"))).size(13)).padding(16);
    match room(app) < sidebar::WIDTH + 160.0 {
        true => column![bar, page].into(),
        false => row![bar, page].into(),
    }
}

fn panel_<'a>() -> Element<'a, Message> {
    let entry = |glyph, name: &str, lit: bool| {
        panel::entry(Some(icon::line(glyph, lit).into()), panel::label(name.to_string(), lit), None)
            .height(panel::ENTRY)
            .style(move |t| style::entry(t, lit, true))
    };
    panel::panel(
        column![
            panel::title("A panel".into()),
            entry(glyphs::PLAY, "Play", true),
            entry(glyphs::NEXT, "Play next", false),
            panel::entry(None, panel::label("No glyph, same column".into(), false), None).height(panel::ENTRY),
        ],
        220.0,
    )
    .into()
}

fn table_(app: &App) -> Element<'_, Message> {
    let (name, artist, album, time) = (
        Length::FillPortion(5),
        Length::FillPortion(3),
        Length::FillPortion(4),
        Length::Fixed(56.0),
    );
    let mut rows = column![
        table::row(
            row![
                table::gutter(),
                table::heading("Title", name),
                table::heading("Artist", artist),
                table::heading("Album", album),
                table::heading("Time", time)
            ]
            .spacing(table::SPACING),
            false,
            false,
            false
        ),
        table::section("Disc 1".into(), table::GUTTER),
    ];
    for (i, (title, by, on, length)) in TRACKS.iter().enumerate() {
        let here = i == app.row;
        let playing = i == 0;
        let gutter: Element<'_, Message> = match playing {
            true => container(icon::accent(glyphs::PLAYING, here)).width(table::GUTTER).into(),
            false => table::gutter(),
        };
        rows = rows.push(
            mouse_area(table::row(
                row![
                    gutter,
                    table::cell(title.to_string(), name, here, playing, false),
                    table::cell(by.to_string(), artist, here, false, true),
                    table::cell(on.to_string(), album, here, false, true),
                    table::cell_cut(length.to_string(), time, Cut::End, here, false, true),
                ]
                .spacing(table::SPACING),
                here,
                true,
                i % 2 == 1,
            ))
            .on_enter(Message::RowAt(i)),
        );
    }
    rows.into()
}

fn cards_(app: &App) -> Element<'_, Message> {
    let cards: Vec<Card<Message>> = [
        ("Goldberg Variations", "Glenn Gould", false),
        ("Suite bergamasque", "Claudio Arrau", false),
        ("Maria Callas", "12 albums", true),
        ("Symphony No. 9", "Wiener Philharmoniker", false),
    ]
    .iter()
    .map(|(title, under, round)| Card {
        seed: title.to_string(),
        title: title.to_string(),
        under: under.to_string(),
        round: *round,
        picture: None,
        open: Message::Ran("open"),
    })
    .collect();
    let line = wrap(
        cards
            .iter()
            .enumerate()
            .map(|(i, c)| cards::view_card(150.0, c, i == app.card, Message::CardAt(i)))
            .collect(),
        150.0,
        20.0,
        room(app),
    );
    column![
        line,
        label("A record is a square and a person a circle. Without a picture, the derived square."),
        cards::header(
            art::square("Goldberg Variations", 120.0, 6.0),
            "ALBUM",
            "Goldberg Variations".into(),
            "Glenn Gould".into(),
            None,
            "1981 · 32 songs · 51 minutes".into(),
            0.0,
        ),
    ]
    .spacing(18)
    .into()
}

fn hero_(app: &App) -> Element<'_, Message> {
    let entries: Vec<hero::Entry<Message>> = [
        ("Bach", "Goldberg Variations", "BWV 988", "32"),
        ("Bach", "The Well-Tempered Clavier", "BWV 846–893", "48"),
        ("Debussy", "Suite bergamasque", "L. 75", "4"),
    ]
    .iter()
    .map(|(over, title, under, count)| hero::Entry {
        over: over.to_string(),
        title: title.to_string(),
        under: under.to_string(),
        count: count.to_string(),
        open: Message::Ran("open"),
    })
    .collect();
    let columns = if room(app) < 560.0 { 1 } else { 2 };
    let listing = hero::listing(&entries, columns, Some(app.card.min(entries.len() - 1)), Message::CardAt);
    column![
        hero::banner(None, "ARTIST", "Johann Sebastian Bach", room(app).min(640.0)),
        hero::heading("Works"),
        listing,
        hero::more("All 1,128 works", false),
    ]
    .spacing(8)
    .into()
}

fn art_<'a>(room: f32) -> Element<'a, Message> {
    let seeds = ["Goldberg Variations", "Clair de lune", "Maria Callas", "Gymnopédies", "Kind of Blue"];
    let squares = wrap(seeds.iter().map(|s| art::square(s, 72.0, 6.0)).collect(), 72.0, 14.0, room);
    let circles = wrap(seeds.iter().map(|s| art::square(s, 72.0, 36.0)).collect(), 72.0, 14.0, room);
    column![
        squares,
        circles,
        label(format!(
            "art::hash(\"Goldberg Variations\") = {:#010x}, the phones' too",
            art::hash("Goldberg Variations")
        )),
    ]
    .spacing(12)
    .into()
}

fn account_<'a>(room: f32) -> Element<'a, Message> {
    let who = |name: &str| Who {
        name: name.to_string(),
        id: "someone".into(),
    };
    let alone: Account<Message> = Account::alone("Alone on this device");
    let faces: Vec<Element<'a, Message>> = vec![
        column![account::face(Some(&who("Ada Lovelace")), None, 48.0), label("Ada Lovelace")]
            .spacing(6)
            .align_x(Alignment::Center)
            .width(110)
            .into(),
        column![account::face(Some(&who("Grace Hopper")), None, 48.0), label("Grace Hopper")]
            .spacing(6)
            .align_x(Alignment::Center)
            .width(110)
            .into(),
        column![account::face(Some(&who("")), None, 48.0), label("no name: the id")]
            .spacing(6)
            .align_x(Alignment::Center)
            .width(110)
            .into(),
        column![account::button(&alone, None, Message::Ran("account")), label("alone, as a button")]
            .spacing(6)
            .align_x(Alignment::Center)
            .width(110)
            .into(),
    ];
    wrap(faces, 110.0, 16.0, room)
}

fn menu_(app: &App) -> Element<'_, Message> {
    let area = mouse_area(
        container(
            text("Right-click anywhere in here for a row's menu. Rest on \"Add to playlist\" for its submenu; j, k, l, h and <Esc> work too.")
                .size(13)
                .style(style::dim),
        )
        .padding(24)
        .width(Length::Fill)
        .height(120)
        .style(style::outline(6.0)),
    )
    .on_right_press(Message::RowMenu)
    .on_press(Message::RowMenu);
    let ran = match app.ran.is_empty() {
        true => "Nothing run yet.".to_string(),
        false => format!("Ran: {}", app.ran),
    };
    column![
        area,
        label(ran),
        label("The theme control at the top right is a menu opened from a control: Anchor::RightEdge.")
    ]
    .spacing(10)
    .into()
}

fn picker_(app: &App) -> Element<'_, Message> {
    let lists = app
        .playlists
        .iter()
        .map(|(n, on)| format!("{n}{}", if *on { " ✓" } else { "" }))
        .collect::<Vec<_>>()
        .join("   ");
    column![
        button(text("Add to playlist…").size(12)).style(style::action).on_press(Message::OpenPicker),
        label("On its own, a picker is centred over a dimmed page (layer::dimmed, layer::centred); off a menu it is that menu's submenu (layer::pinned)."),
        label(lists),
    ]
    .spacing(10)
    .into()
}

fn dialog_<'a>() -> Element<'a, Message> {
    column![
        button(text("Open a dialog").size(12))
            .style(style::action)
            .on_press(Message::Dialog(true)),
        label("It rises as the page dims, eased over dialog::OPENING."),
    ]
    .spacing(10)
    .into()
}

fn keys(app: &App) -> Element<'_, Message> {
    column![
        text("Press keys: 3j, gg, G, ^d, /bach, n, <Tab>, <Esc>.").size(13),
        pair("pending", app.keys.pending().to_string()),
        pair("last action", if app.last.is_empty() { "—".into() } else { app.last.clone() }),
        label("A Move walks this page's sidebar; the rest are only said here."),
    ]
    .spacing(8)
    .into()
}

fn route_(app: &App) -> Element<'_, Message> {
    use arkui::route::Route;
    column![
        pair("this page", app.place.fragment()),
        pair("Place::parse(\"#menu\")", format!("{:?}", Place::parse("#menu"))),
        pair("Place::parse(\"#nowhere\")", format!("{:?}", Place::parse("#nowhere"))),
        label("Walking the sidebar writes the bar; back and forward come back through Router::poll."),
    ]
    .spacing(8)
    .into()
}

fn wake_(app: &App) -> Element<'_, Message> {
    column![
        pair("bell rung", app.rung.to_string()),
        label("The page's hashchange and popstate ring this window's bell (wake::page_changes). Use back and forward in the browser and watch it count."),
    ]
    .spacing(8)
    .into()
}

/// A module with nothing to draw on its own: where to see it at work.
fn said<'a>(_app: &App, module: &str) -> Element<'a, Message> {
    let where_ = match module {
        "scroll" => "Walk the sidebar with j and k past the bottom of the window: scroll::reveal keeps the cursor on screen.",
        "swipe" => "On the desktop build, two fingers sideways over the page go back and forward. In a browser the browser's own back does it.",
        "images" => {
            "Every picture an app shows comes through it; the examples here draw the derived square, which is what a picture shows until it arrives."
        }
        _ => "No example yet.",
    };
    text(where_.to_string()).size(13).style(style::dim).into()
}
