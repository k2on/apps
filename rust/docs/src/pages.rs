//! The pages: the overview, and one page per component — how to use it on the desktop, and on a phone, Android or
//! iOS. Every line of words comes from [`crate::components`] and
//! [`crate::catalogue`].
use arkui::{hero, style, table};
use iced::widget::{button, column, container, mouse_area, row, text, Column, Space};
use iced::{Alignment, Element, Length, Padding};

use crate::catalogue::{self, ENTRIES};
use crate::components::{Component, Mobile, COMPONENTS};
use crate::screens::{self, Phone};
use crate::{examples, App, Message, Place};

/// The page's gutter either side: a shelf's padding, so a list, a header
/// and a row of cards start at one line; a phone's 16.
pub fn gutter(app: &App) -> f32 {
    if app.narrow() {
        16.0
    } else {
        24.0
    }
}
/// How wide prose runs before it wraps.
const MEASURE: f32 = 760.0;

pub fn view(app: &App) -> Element<'_, Message> {
    let body: Element<'_, Message> = match app.place {
        Place::Overview => overview(app),
        Place::Component(id) => component(app, crate::components::find(id).expect("a place is a component")),
    };
    container(body)
        .padding(Padding {
            top: 8.0,
            right: gutter(app) + if app.narrow() { 0.0 } else { arkui::SCROLLBAR },
            bottom: 48.0,
            left: gutter(app),
        })
        .width(Length::Fill)
        .into()
}

fn title<'a>(name: &str, summary: &str) -> Column<'a, Message> {
    column![
        text(name.to_string()).size(28).font(hero::BOLD),
        container(text(summary.to_string()).size(14)).max_width(MEASURE),
    ]
    .spacing(6)
}

fn prose<'a>(lines: impl IntoIterator<Item = String>) -> Element<'a, Message> {
    lines
        .into_iter()
        .fold(column![].spacing(6), |col, line| {
            col.push(container(text(line).size(13)).max_width(MEASURE))
        })
        .into()
}

fn bullets<'a>(lines: &[&str]) -> Element<'a, Message> {
    prose(lines.iter().map(|s| format!("•  {s}")))
}

fn none<'a>(said: impl Into<String>) -> Element<'a, Message> {
    text(said.into()).size(13).style(style::faint(0.5)).into()
}

fn small<'a>(said: impl Into<String>) -> Element<'a, Message> {
    text(said.into()).size(11).style(style::faint(0.6)).into()
}

/// A sub-heading inside a half: smaller than the half's own.
fn label<'a>(said: &str) -> Element<'a, Message> {
    container(text(said.to_string()).size(14).font(hero::BOLD))
        .padding(Padding {
            top: 14.0,
            right: 0.0,
            bottom: 2.0,
            left: 0.0,
        })
        .into()
}

fn yes(on: bool) -> String {
    match on {
        true => "yes".into(),
        false => "—".into(),
    }
}

fn overview<'a>(app: &App) -> Element<'a, Message> {
    // A phone's width has room for the name and where it is on the desktop.
    let narrow = app.narrow();
    let captured: usize = COMPONENTS.iter().map(|c| c.shots.len()).sum();
    let intro = prose([
        "The ui every app here shares, by component. Each page says how to use it on the desktop, where every example is the arkui component itself, drawn by the renderer the apps use, and on a phone, Android or iOS, with the latest capture from a real emulator and simulator in a frame of that device.".to_string(),
        "This is the one place the components are described. An agent working on an app reads the page for the part it needs; one that changes a component changes its entry in rust/docs/src/components.rs (and the module's in catalogue.rs, its example in examples.rs) in the same commit, and the tests fail until it does. Other sites pull catalogue.json, published beside this page.".to_string(),
        format!(
            "{} components over {} arkui modules, and {captured} captures from the phones. j and k walk the sidebar.",
            COMPONENTS.len(),
            ENTRIES.len()
        ),
    ]);
    let (name, section, desktop, phone) = (
        Length::FillPortion(2),
        Length::FillPortion(2),
        Length::FillPortion(4),
        Length::FillPortion(1),
    );
    let mut head = row![table::heading("Component", name)].spacing(table::SPACING);
    if !narrow {
        head = head.push(table::heading("Section", section));
    }
    head = head.push(table::heading("Desktop", desktop));
    if !narrow {
        head = head.push(table::heading("Android", phone)).push(table::heading("iOS", phone));
    }
    let mut rows = column![table::row(head, false, false, false,)];
    for (i, c) in COMPONENTS.iter().enumerate() {
        let on_desktop = match c.desktop.is_empty() {
            false => c.desktop.iter().map(|m| format!("arkui::{m}")).collect::<Vec<_>>().join(", "),
            true => "—".into(),
        };
        rows = rows.push(
            mouse_area(table::row(
                {
                    let mut line = row![table::cell(c.name.into(), name, false, false, false)].spacing(table::SPACING);
                    if !narrow {
                        line = line.push(table::cell(c.section.heading().into(), section, false, false, true));
                    }
                    line = line.push(table::cell_cut(
                        on_desktop,
                        desktop,
                        arkui::fit::Cut::End,
                        false,
                        false,
                        c.desktop.is_empty(),
                    ));
                    if !narrow {
                        line = line
                            .push(table::cell(yes(c.android.is_some()), phone, false, false, c.android.is_none()))
                            .push(table::cell(yes(c.ios.is_some()), phone, false, false, c.ios.is_none()));
                    }
                    line
                },
                false,
                false,
                i % 2 == 1,
            ))
            .on_press(Message::Go(Place::Component(c.id))),
        );
    }
    column![
        title(
            "arkui",
            "iced components, the vim keyboard, theming, the glyph table, a picture cache and the address bar — and nothing of any app."
        ),
        intro,
        hero::heading("Components"),
        rows,
    ]
    .spacing(14)
    .into()
}

fn component<'a>(app: &'a App, c: &'static Component) -> Element<'a, Message> {
    column![
        title(c.name, c.summary),
        hero::heading("Desktop"),
        desktop(app, c),
        mobile_heading(app),
        mobile(app, c)
    ]
    .spacing(6)
    .into()
}

/// The desktop half: the example, then each module's API and rules.
fn desktop<'a>(app: &'a App, c: &'static Component) -> Element<'a, Message> {
    if c.desktop.is_empty() {
        return none(format!("arkui has no {} yet.", c.name.to_lowercase()));
    }
    let example = container(examples::for_component(app, c.id)).padding(20).width(Length::Fill).style(|t| {
        let mut s = style::outline(8.0)(t);
        s.background = Some(iced::Background::Color(style::surface_color(t)));
        s
    });
    let mut out = column![
        small(c.desktop.iter().map(|m| format!("arkui::{m}")).collect::<Vec<_>>().join("  ·  ")),
        example
    ]
    .spacing(10);
    for m in c.desktop {
        let e = catalogue::find(m).expect("a component's module is in the catalogue");
        let mut part = column![
            label(&format!("arkui::{}", e.module)),
            container(text(e.summary).size(13).style(style::dim)).max_width(MEASURE),
            bullets(e.api)
        ]
        .spacing(6);
        if !e.rules.is_empty() {
            part = part.push(small("Rules")).push(bullets(e.rules));
        }
        part = part.push(small(e.source()));
        out = out.push(part);
    }
    out.into()
}

/// "Mobile", and the switch between the two phones beside it.
fn mobile_heading(app: &App) -> Element<'_, Message> {
    let tab = |phone: Phone, name: &'static str| {
        button(text(name).size(12))
            .padding([4, 12])
            .style(if app.phone == phone { style::action } else { style::quiet })
            .on_press(Message::Phone(phone))
    };
    let switch = row![tab(Phone::Android, "Android"), tab(Phone::Ios, "iOS")].spacing(6);
    // Beside the heading where there is room, under it on a phone.
    match app.narrow() {
        true => column![hero::heading("Mobile"), switch].spacing(4).into(),
        false => row![hero::heading("Mobile"), Space::new().width(16), switch]
            .align_y(Alignment::End)
            .into(),
    }
}

fn mobile<'a>(app: &'a App, c: &'static Component) -> Element<'a, Message> {
    let (half, name): (Option<&Mobile>, &str) = match app.phone {
        Phone::Android => (c.android.as_ref(), "Android"),
        Phone::Ios => (c.ios.as_ref(), "iOS"),
    };
    let Some(half) = half else {
        return none(format!("Nothing shared for this on {name} yet."));
    };
    let words = column![label("Where"), bullets(half.at), label("How"), bullets(half.how)]
        .spacing(6)
        .width(Length::Fill);
    let shots: Vec<&'static crate::components::Shot> = c.shots.iter().filter(|s| s.phones.contains(&app.phone.key())).collect();
    if shots.is_empty() {
        return words.into();
    }
    // As many phones to a row as fit: one on a phone.
    let per = (((app.content() + 24.0) / (screens::PHONE + 44.0)) as usize).max(1);
    let mut phones = column![].spacing(24);
    for line in shots.chunks(per) {
        phones = phones.push(line.iter().fold(row![].spacing(24), |r, shot| r.push(captured(app, shot))));
    }
    column![phones, words].spacing(16).into()
}

/// One capture in its phone, and what it is under it.
fn captured<'a>(app: &'a App, shot: &'static crate::components::Shot) -> Element<'a, Message> {
    let found = app.fetched.find(app.phone, shot.id);
    let device = found.map(|(p, _)| p.device.clone()).unwrap_or_else(|| screens::default_device(app.phone));
    let (picture, missing) = match (&app.fetched, found) {
        (screens::Fetched::Asking, _) => (None, "Asking for the latest capture…".to_string()),
        (screens::Fetched::Missing(_), _) => (None, "No captures published yet: the screens workflow has not run.".to_string()),
        (screens::Fetched::Got(_), None) => (None, format!("Not captured yet on {}.", device.name)),
        (screens::Fetched::Got(m), Some((_, got))) => {
            let urls = screens::urls(m, app.phone, got, app.dark());
            let cache = app.pictures.of(&got.kind);
            let ready = urls.iter().filter(|u| cache.handle(u).is_some()).count();
            let picture = match got.kind.as_str() {
                // A recording plays once every frame is in, so it never stutters on one still on its way.
                "recording" if ready == urls.len() => cache.handle(&urls[(app.played as usize) % urls.len()]),
                "recording" => None,
                _ => cache.handle(&urls[0]),
            };
            (picture, format!("Loading… {ready} of {}", urls.len()))
        }
    };
    let from = match app.fetched.manifest() {
        Some(m) if found.is_some() => format!("{} · {} · {}", device.name, &m.commit[..m.commit.len().min(7)], m.made),
        _ => device.name.clone(),
    };
    column![
        screens::frame(&device, screens::screen(&device, picture, missing)),
        text(format!("{} · {}", screens::kind_name(shot.kind), shot.what))
            .size(12)
            .width(screens::PHONE + 20.0),
        small(from),
    ]
    .spacing(6)
    .into()
}
