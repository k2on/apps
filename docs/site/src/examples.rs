//! What each component's desktop half shows, built from arkui exactly as an
//! app would build it: pictures drawn by [`crate::render`], and tables of
//! what a function answers, worked out when the site is built.
use arkui::account::{self, Account, Who};
use arkui::cards::{self, Card};
use arkui::context::Context;
use arkui::fit::Cut;
use arkui::line::Line;
use arkui::menu::{self, Anchor, Entry, Menu};
use arkui::picker::{self, Choice, Picker};
use arkui::{art, format, glyphs, hero, icon, layer, panel, sidebar, style, table, theme, url, vim};
use iced::keyboard::{key::Named, Key, Modifiers};
use iced::widget::{button, column, container, row, slider, stack, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Point, Size, Theme};

use crate::render::{Painter, Picture};

/// The examples are drawn, not used: nothing they say is ever heard.
type El<'a> = Element<'a, ()>;

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

/// Rows to draw with: places, long enough to be cut, and with the
/// characters a typeface may lack.
const ROWS: [(&str, &str, &str, &str); 4] = [
    ("Harbour", "North coast", "Coastal places", "3 km"),
    ("Orchard", "The valley", "Farmland", "1 km"),
    (
        "A place with a name far too long for its column, cut through the middle",
        "Somewhere a long way off",
        "Places with long names",
        "12 km",
    ),
    ("Prairie", "The plains", "Open country", "5 km"),
];

/// Names a square or a card is drawn from.
const PLACES: [&str; 6] = ["Harbour", "Orchard", "Meadow", "Summit", "Delta", "Lagoon"];

/// Where a picture is drawn: one theme, one width.
pub struct Canvas<'p> {
    painter: &'p mut Painter,
    theme: &'p Theme,
    /// How wide it is laid out, in logical pixels.
    pub width: f32,
}

impl<'p> Canvas<'p> {
    pub fn new(painter: &'p mut Painter, theme: &'p Theme, width: f32) -> Self {
        Canvas { painter, theme, width }
    }

    /// As tall as it draws.
    fn paint(self, element: El<'_>) -> Picture {
        self.painter.draw(element, self.theme, self.width, None)
    }

    /// A window `height` tall, for what is laid over a page.
    fn window(self, height: f32, element: El<'_>) -> Picture {
        self.painter.draw(element, self.theme, self.width, Some(height))
    }
}

/// One picture of a component.
pub struct Example {
    /// Its file's name, unique across the site.
    pub name: &'static str,
    pub caption: &'static str,
    pub draw: fn(Canvas) -> Picture,
}

const fn ex(name: &'static str, caption: &'static str, draw: fn(Canvas) -> Picture) -> Example {
    Example { name, caption, draw }
}

/// A component's pictures, in the order the page shows them.
pub fn pictures(component: &str) -> &'static [Example] {
    PICTURES.iter().find(|(c, _)| *c == component).map(|(_, e)| *e).unwrap_or(&[])
}

const PICTURES: &[(&str, &[Example])] = &[
    (
        "theme",
        &[ex(
            "theme",
            "The palette's colours, then the derived square's pairs, picked by name.",
            theme_,
        )],
    ),
    (
        "style",
        &[ex(
            "style",
            "Text tones, the two buttons and the seek slider, then rows: the zebra, and the cursor on the second.",
            style_,
        )],
    ),
    (
        "icons",
        &[
            ex("glyphs", "Every glyph, by its name in arkui::glyphs.", glyphs_),
            ex("icon", "One glyph in each of icon's tones.", icon_),
        ],
    ),
    (
        "text",
        &[ex(
            "fit",
            "One name cut to fit: through the middle for a name, at the end for a heading.",
            cut,
        )],
    ),
    ("art", &[ex("art", "Squares and circles, each derived from its name.", art_)]),
    (
        "navigation",
        &[ex(
            "sidebar",
            "A sidebar under headings, with counts, the cursor on its second place.",
            sidebar_,
        )],
    ),
    (
        "lists",
        &[ex(
            "table",
            "A table: its headings, a section, and rows with the cursor on the second.",
            table_,
        )],
    ),
    (
        "cards",
        &[ex("cards", "Cards, square for a thing and round for a person, then a header.", cards_)],
    ),
    ("headers", &[ex("hero", "A banner, a heading, a listing and its last row.", hero_)]),
    (
        "account",
        &[ex(
            "account",
            "Faces from a name, or from an id with none, and the account's button.",
            account_,
        )],
    ),
    (
        "menu",
        &[
            ex(
                "menu",
                "A row's menu, opened at the pointer, its cursor resting on an entry with a submenu.",
                menu_,
            ),
            ex("panel", "A panel: its title, and entries with and without a glyph.", panel_),
        ],
    ),
    ("picker", &[ex("picker", "A picker on its own, centred over the dimmed page.", picker_)]),
    ("dialog", &[ex("dialog", "A dialog risen over the page as it dims.", dialog_)]),
];

/// What a component's functions answer, as a table: the call, then what it
/// gave when the site was built.
pub struct Answers {
    pub caption: &'static str,
    pub rows: Vec<(String, String)>,
}

pub fn answers(component: &str) -> Vec<Answers> {
    match component {
        "text" => vec![Answers {
            caption: "arkui::format",
            rows: vec![
                ("clock(261.0)".into(), format::clock(261.0)),
                ("clock(3725.0)".into(), format::clock(3725.0)),
                ("spell(3_725_000)".into(), format::spell(3_725_000)),
                ("plural(1, \"place\")".into(), format::plural(1, "place")),
                ("plural(12, \"place\")".into(), format::plural(12, "place")),
                ("span(1685, 1750)".into(), format::span(1685, 1750)),
                ("fit::middle(LONG, 24)".into(), arkui::fit::middle(LONG, 24)),
            ],
        }],
        "art" => vec![Answers {
            caption: "arkui::art, the same on both phones",
            rows: PLACES
                .iter()
                .map(|p| (format!("hash(\"{p}\")"), format!("{:#010x}", art::hash(p))))
                .collect(),
        }],
        "routes" => {
            let name = "North/South #1 · é";
            vec![Answers {
                caption: "arkui::url and arkui::route",
                rows: vec![
                    (format!("url::encoded(\"{name}\")"), url::encoded(name)),
                    ("url::decode(url::encoded(…))".into(), url::decode(&url::encoded(name))),
                    (
                        format!("route::join(\"place\", Some(\"{name}\"))"),
                        arkui::route::join("place", Some(name)),
                    ),
                ],
            }]
        }
        "keys" => vec![Answers {
            caption: "What vim::Keys::press finishes for each run of keys",
            rows: KEYS.iter().map(|k| (k.to_string(), said(k))).collect(),
        }],
        _ => vec![],
    }
}

/// A component's notes: what a picture cannot show.
pub fn notes(component: &str) -> &'static [&'static str] {
    match component {
        "theme" => &["Every picture on this site is drawn twice, in iced's light and dark themes with arkui's neutral palette, and the page shows the one its own theme is in."],
        "style" => &["A row under the cursor is marked whether or not its pane has the keys, and lit only while it has."],
        "art" => &["arkui::images is how every picture an app shows arrives; until it has, the derived square is what a picture shows."],
        "menu" => &[
            "A right click opens a row's menu at the pointer (Anchor::Pointer); a control's menu, a dropdown, hangs under the control (Anchor::RightEdge).",
            "Resting on an entry with a submenu shows it beside the entry; j, k, l, h and <Esc> walk the menu and its submenu.",
        ],
        "picker" => &["Off a menu, a picker is that menu's submenu, pinned beside its entry (layer::pinned); on its own it is centred over a dimmed page (layer::dimmed, layer::centred)."],
        "dialog" => &["It rises as the page dims, eased over dialog::OPENING; a click on the dimmed page, its ×, or <Esc> closes it."],
        "pages" => &["On the desktop, two fingers sideways over the page go back and forward (arkui::swipe). In a browser the browser's own back does it."],
        "scroll" => &["scroll::reveal keeps the cursor on screen as j and k walk past the edge of a list."],
        "keys" => &["A count before a motion repeats it; a Move walks whatever grid the pane with the keys has (vim::Grid, vim::Page)."],
        "routes" => &["A place is its address: walking a sidebar writes the bar, and back and forward come back through Router::poll."],
        "waking" => &["A window that runs no loop sleeps until something rings its Bell: a page's hashchange and popstate ring it (wake::page_changes)."],
        _ => &[],
    }
}

const LONG: &str = "A name far too long for where it goes, which has to lose its end or its middle (2026)";

/// The runs of keys the Keys page asks about.
const KEYS: &[&str] = &[
    "j",
    "3j",
    "k",
    "gg",
    "G",
    "12G",
    "^d",
    "^u",
    "<Enter>",
    "<Space>",
    "/dial<Enter>",
    "n",
    "N",
    "<Tab>",
    "^o",
    "^i",
    "3<Esc>",
];

/// What [`vim::Keys`] finishes after `run`, or what it is still waiting on.
fn said(run: &str) -> String {
    let mut keys = vim::Keys::new();
    let mut last = None;
    for (key, mods) in presses(run) {
        last = keys.press(&key, mods);
    }
    match last {
        Some(action) => format!("{action:?}"),
        None => format!("pending: {}", keys.pending()),
    }
}

/// `run` as presses: a character each, `<Name>` a named key, `^c` with Ctrl.
fn presses(run: &str) -> Vec<(Key, Modifiers)> {
    let mut out = Vec::new();
    let mut rest = run;
    while let Some(c) = rest.chars().next() {
        if let Some(named) = rest.strip_prefix('<').and_then(|r| r.split_once('>')) {
            let key = match named.0 {
                "Enter" => Named::Enter,
                "Space" => Named::Space,
                "Tab" => Named::Tab,
                "Esc" => Named::Escape,
                other => panic!("no such key <{other}>"),
            };
            out.push((Key::Named(key), Modifiers::empty()));
            rest = named.1;
        } else if let Some(ctrl) = rest.strip_prefix('^').and_then(|r| r.chars().next()) {
            out.push((Key::Character(ctrl.to_string().into()), Modifiers::CTRL));
            rest = &rest[1 + ctrl.len_utf8()..];
        } else {
            out.push((Key::Character(c.to_string().into()), Modifiers::empty()));
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// `items`, each `item` wide, in as many rows as `room` needs: an example
/// on a phone's width wraps rather than running off its box.
fn wrap<'a>(items: Vec<El<'a>>, item: f32, gap: f32, room: f32) -> El<'a> {
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

fn label<'a>(s: impl Into<String>) -> El<'a> {
    text(s.into()).size(11).style(style::faint(0.6)).into()
}

fn swatch<'a>(name: &'static str, colour: fn(&Theme) -> Color) -> El<'a> {
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

fn theme_(c: Canvas) -> Picture {
    let room = c.width;
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
        PLACES
            .iter()
            .map(|seed| column![art::square(seed, 48.0, 6.0), label(*seed)].spacing(4).into())
            .collect(),
        48.0,
        12.0,
        room,
    );
    c.paint(column![colours, label("The art pairs, picked by name:"), squares].spacing(14).into())
}

fn style_(c: Canvas) -> Picture {
    let room = c.width;
    let tones = row![
        text("plain").size(13),
        text("dim").size(13).style(style::dim),
        text("faint(0.5)").size(13).style(style::faint(0.5)),
        text("accent").size(13).style(style::accent),
    ]
    .spacing(16);
    let buttons = row![
        button(text("action").size(12)).style(style::action).on_press(()),
        button(text("quiet").size(12)).style(style::quiet).on_press(()),
        slider(0.0..=100.0, 40.0, |_| ()).style(style::seek).width(160),
    ]
    .spacing(16)
    .align_y(Alignment::Center);
    let rows = (0..4).fold(column![], |col, i| {
        let on = i == 1;
        col.push(
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
            .width(room.min(360.0)),
        )
    });
    c.paint(column![tones, buttons, label("Rows: the zebra, and the cursor"), rows].spacing(14).into())
}

fn glyphs_(c: Canvas) -> Picture {
    let room = c.width;
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
    c.paint(grid.into())
}

fn icon_(c: Canvas) -> Picture {
    let room = c.width;
    let tone = |name: &'static str, drawn: iced::widget::Svg<'static>| column![drawn, label(name)].spacing(6).align_x(Alignment::Center).width(110);
    let tones: Vec<El<'static>> = vec![
        tone("plain", icon::plain(glyphs::PLAY, false)).into(),
        tone("plain, dim", icon::plain(glyphs::PLAY, true)).into(),
        tone("accent", icon::accent(glyphs::PLAY, false)).into(),
        tone("line", icon::line(glyphs::PLAY, false)).into(),
        tone("faint", icon::faint(glyphs::PLAY, false)).into(),
        tone("tick", icon::tick(false)).into(),
        tone("more", icon::more(false)).into(),
        tone("chevron", icon::chevron(false)).into(),
    ];
    c.paint(wrap(tones, 110.0, 4.0, room))
}

fn cut(c: Canvas) -> Picture {
    let room = c.width;
    let at = |w: f32, cut: Cut| {
        row![
            text(format!("{w}px")).size(11).style(style::faint(0.6)).width(52),
            container(Line::new(LONG, 13.0, cut, |t| theme::of(t).background.base.text))
                .width(w)
                .style(style::outline(4.0))
                .padding([2, 6]),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    };
    c.paint(
        column![
            label("Cut::Middle — a name"),
            at((room - 80.0).min(420.0), Cut::Middle),
            at(240.0, Cut::Middle),
            at(140.0, Cut::Middle),
            label("Cut::End — a heading"),
            at(240.0, Cut::End),
            at(140.0, Cut::End),
        ]
        .spacing(8)
        .into(),
    )
}

fn sidebar_(c: Canvas) -> Picture {
    let room = c.width;
    let places = [
        (glyphs::HOME, "Home", Some("Places"), None),
        (glyphs::LIBRARY, "Collections", Some("Places"), Some(12)),
        (glyphs::SEARCH, "Search", Some("Places"), None),
        (glyphs::TICK, "Done", Some("Lists"), Some(3)),
        (glyphs::MORE, "Later", Some("Lists"), Some(8)),
        (glyphs::ARTIST, "People", Some("Lists"), Some(41)),
    ];
    let items: Vec<sidebar::Item<()>> = places
        .iter()
        .map(|(glyph, name, heading, count)| sidebar::Item {
            glyph,
            label: name.to_string(),
            heading: *heading,
            count: *count,
            press: (),
        })
        .collect();
    let bar = container(sidebar::view(&items, 1, true, sidebar::WIDTH, 8.0, "example-sidebar")).height(300);
    let page = container(text("The page for Collections").size(13)).padding(16);
    c.paint(match room < sidebar::WIDTH + 160.0 {
        true => column![bar, page].into(),
        false => row![bar, page].into(),
    })
}

fn panel_(c: Canvas) -> Picture {
    let entry = |glyph, name: &str, lit: bool| {
        panel::entry(Some(icon::line(glyph, lit).into()), panel::label(name.to_string(), lit), None)
            .height(panel::ENTRY)
            .style(move |t| style::entry(t, lit, true))
    };
    c.paint(
        panel::panel(
            column![
                panel::title("A panel".into()),
                entry(glyphs::PLAY, "Open", true),
                entry(glyphs::NEXT, "Open beside", false),
                panel::entry(None, panel::label("No glyph, same column".into(), false), None).height(panel::ENTRY),
            ],
            220.0,
        )
        .into(),
    )
}

/// A table of [`ROWS`], the cursor on `cursor`.
fn rows<'a>(cursor: Option<usize>, narrow: bool) -> El<'a> {
    let (name, place, collection, time) = (
        Length::FillPortion(5),
        Length::FillPortion(3),
        Length::FillPortion(4),
        Length::Fixed(56.0),
    );
    let mut head = row![table::gutter(), table::heading("Name", name), table::heading("Where", place)].spacing(table::SPACING);
    if !narrow {
        head = head.push(table::heading("Collection", collection));
    }
    head = head.push(table::heading("Length", time));
    let mut out = column![table::row(head, false, false, false), table::section("Section one".into(), table::GUTTER)];
    for (i, (title, by, on, length)) in ROWS.iter().enumerate() {
        let here = Some(i) == cursor;
        let first = i == 0;
        let gutter: El<'a> = match first {
            true => container(icon::accent(glyphs::TICK, here)).width(table::GUTTER).into(),
            false => table::gutter(),
        };
        let mut line = row![
            gutter,
            table::cell(title.to_string(), name, here, first, false),
            table::cell(by.to_string(), place, here, false, true),
        ]
        .spacing(table::SPACING);
        if !narrow {
            line = line.push(table::cell(on.to_string(), collection, here, false, true));
        }
        line = line.push(table::cell_cut(length.to_string(), time, Cut::End, here, false, true));
        out = out.push(table::row(line, here, true, i % 2 == 1));
    }
    out.into()
}

/// Narrow enough that a table drops a column.
fn narrow(width: f32) -> bool {
    width < 480.0
}

fn table_(c: Canvas) -> Picture {
    let narrow = narrow(c.width);
    c.paint(rows(Some(1), narrow))
}

fn cards_(c: Canvas) -> Picture {
    let room = c.width;
    let cards: Vec<Card<()>> = [
        ("Harbour", "North coast", false),
        ("Orchard", "The valley", false),
        ("Ada Lovelace", "Someone", true),
        ("Summit", "The highlands", false),
    ]
    .iter()
    .map(|(title, under, round)| Card {
        seed: title.to_string(),
        title: title.to_string(),
        under: under.to_string(),
        round: *round,
        picture: None,
        open: (),
    })
    .collect();
    let side = if room < 400.0 { 140.0 } else { 150.0 };
    let line = wrap(
        cards
            .iter()
            .enumerate()
            .map(|(i, card)| cards::view_card(side, card, i == 0, ()))
            .collect(),
        side,
        20.0,
        room,
    );
    c.paint(
        column![
            line,
            cards::header(
                art::square("Coastal places", 120.0, 6.0),
                "COLLECTION",
                "Coastal places".into(),
                "North coast".into(),
                None,
                "2026 · 12 places · 40 km".into(),
                0.0,
            ),
        ]
        .spacing(18)
        .into(),
    )
}

fn hero_(c: Canvas) -> Picture {
    let room = c.width;
    let entries: Vec<hero::Entry<()>> = [
        ("Coast", "Harbour", "The north", "12"),
        ("Coast", "Lagoon", "The south", "8"),
        ("Hills", "Summit", "The highlands", "4"),
    ]
    .iter()
    .map(|(over, title, under, count)| hero::Entry {
        over: over.to_string(),
        title: title.to_string(),
        under: under.to_string(),
        count: count.to_string(),
        open: (),
    })
    .collect();
    let columns = if room < 560.0 { 1 } else { 2 };
    let listing = hero::listing(&entries, columns, Some(0), |_| ());
    c.paint(
        column![
            hero::banner(None, "COLLECTION", "Coastal places", room.min(640.0)),
            hero::heading("Places"),
            listing,
            hero::more("All 128 places", false),
        ]
        .spacing(8)
        .into(),
    )
}

fn art_(c: Canvas) -> Picture {
    let room = c.width;
    let seeds = ["Harbour", "Orchard", "Ada Lovelace", "Meadow", "Summit"];
    let squares = wrap(seeds.iter().map(|s| art::square(s, 72.0, 6.0)).collect(), 72.0, 14.0, room);
    let circles = wrap(seeds.iter().map(|s| art::square(s, 72.0, 36.0)).collect(), 72.0, 14.0, room);
    c.paint(column![squares, circles].spacing(12).into())
}

fn account_(c: Canvas) -> Picture {
    let room = c.width;
    let who = |name: &str| Who {
        name: name.to_string(),
        id: "someone".into(),
    };
    let alone: Account<()> = Account::alone("Alone on this device");
    let face = |shown: El<'static>, name: &'static str| -> El<'static> {
        column![shown, label(name)].spacing(6).align_x(Alignment::Center).width(110).into()
    };
    let faces = vec![
        face(account::face(Some(&who("Ada Lovelace")), None, 48.0), "Ada Lovelace"),
        face(account::face(Some(&who("Grace Hopper")), None, 48.0), "Grace Hopper"),
        face(account::face(Some(&who("")), None, 48.0), "no name: the id"),
        face(account::button(&alone, None, ()), "alone, as a button"),
    ];
    c.paint(wrap(faces, 110.0, 16.0, room))
}

/// How tall a picture of an overlay is: a window's worth of page under it.
const WINDOW: f32 = 300.0;

fn menu_(c: Canvas) -> Picture {
    let window = Size::new(c.width, WINDOW);
    let entries = vec![
        Entry::run(glyphs::CHEVRON, "Open", ()),
        Entry::run(glyphs::NEXT, "Open beside", ()),
        Entry::run(glyphs::HOME, "Go to Coastal places, a collection with a long name", ()),
        Entry::submenu(glyphs::ADD_TO, "Add to collection", ()),
        Entry::run(glyphs::MINUS, "Remove", ()),
    ];
    let mut ctx: Context<(), usize> = Context::new();
    // Near the left, so the submenu has room beside the menu.
    let pointer = Point::new(24.0, 56.0);
    ctx.open_menu(Menu::open("Harbour", entries, pointer, window, Anchor::Pointer));
    ctx.land(3);
    let choices = ["Favourites", "Trips", "Later"]
        .iter()
        .enumerate()
        .map(|(i, name)| Choice {
            value: i,
            name: name.to_string(),
            on: i == 0,
        })
        .collect();
    ctx.open_picker(Picker::new("Add to collection", choices, Some("New collection…".into())), window);
    let m = ctx.menu.as_ref().expect("the menu is up");
    let p = ctx.picker.as_ref().expect("the submenu is up");
    let mut layers = stack![rows(Some(0), narrow(c.width)), layer::pinned(menu::view(m, true, |_| (), ()), m.origin)];
    if let Some(at) = p.origin {
        layers = layers.push(layer::pinned(picker::view(p, false, messages()), at));
    }
    c.window(WINDOW, layers.width(Length::Fill).height(Length::Fill).into())
}

fn messages<'a>() -> picker::Messages<'a, ()> {
    picker::Messages {
        at: Box::new(|_| ()),
        activate: (),
        name: Box::new(|_| ()),
        create: (),
    }
}

fn picker_(c: Canvas) -> Picture {
    let choices = ["Favourites", "Trips", "Later"]
        .iter()
        .enumerate()
        .map(|(i, name)| Choice {
            value: i,
            name: name.to_string(),
            on: i != 1,
        })
        .collect();
    let mut p = Picker::new("Add to collection", choices, Some("New collection…".into()));
    p.land(1);
    let drawn = picker::view(&p, true, messages());
    let narrow = narrow(c.width);
    c.window(
        WINDOW,
        stack![rows(None, narrow), layer::dimmed((), ()), layer::centred(drawn)]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

fn dialog_(c: Canvas) -> Picture {
    let close = button(icon::tinted(glyphs::CLOSE, 14.0, |p| p.background.base.text))
        .style(style::quiet)
        .padding(6)
        .on_press(());
    let body =
        column![text("A dialog rises over the page as the page dims. A click on the dimmed page, the ×, or <Esc> closes it.").size(13)].spacing(8);
    let width = (c.width - 32.0).min(420.0);
    let narrow = narrow(c.width);
    c.window(
        WINDOW,
        stack![
            rows(None, narrow),
            arkui::dialog::backdrop((), (), 1.0),
            arkui::dialog::view("A dialog".into(), close.into(), body.into(), width, 240.0, 1.0),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
    )
}
