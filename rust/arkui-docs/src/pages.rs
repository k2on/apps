//! The pages: the overview, what harken draws without arkui, and one page
//! per module — what it is, its example, its API, its rules, and where harken
//! uses it. Every line of words comes from [`crate::catalogue`].
use arkui::{hero, style, table};
use iced::widget::{column, container, mouse_area, row, text, Column};
use iced::{Element, Length, Padding};

use crate::catalogue::{self, Entry, ENTRIES, HARKEN_ONLY};
use crate::{examples, App, Message, Place};

/// The page's gutter either side, harken's.
pub const GUTTER: f32 = 24.0;
/// How wide prose runs before it wraps.
const MEASURE: f32 = 760.0;

pub fn view(app: &App) -> Element<'_, Message> {
    let body: Element<'_, Message> = match app.place {
        Place::Overview => overview(),
        Place::Harken => harken(),
        Place::Module(m) => module(app, catalogue::find(m).expect("a place is an entry")),
    };
    container(body)
        .padding(Padding {
            top: 8.0,
            right: GUTTER + arkui::SCROLLBAR,
            bottom: 48.0,
            left: GUTTER,
        })
        .width(Length::Fill)
        .into()
}

fn title<'a>(over: String, name: &str, summary: &str) -> Column<'a, Message> {
    column![
        text(over).size(11).style(style::faint(0.6)),
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

fn none<'a>(said: &str) -> Element<'a, Message> {
    text(said.to_string()).size(13).style(style::faint(0.5)).into()
}

fn overview<'a>() -> Element<'a, Message> {
    let used = ENTRIES.iter().filter(|e| !e.harken.is_empty()).count();
    let intro = prose([
        "The ui every app here shares, drawn by itself: every example on these pages is the arkui component, in the renderer the apps use, not a picture of one.".to_string(),
        "This is the one place the components are described. An agent working on an app reads the page for the part it needs; one that changes a component changes its entry in rust/arkui-docs/src/catalogue.rs and its example in examples.rs in the same commit, and the tests fail until it does. Other sites pull catalogue.json, published beside this page.".to_string(),
        format!("{} modules; harken's desktop draws with {used} of them. j and k walk the sidebar.", ENTRIES.len()),
    ]);
    // The catalogue as a table, a row per module: a row opens its page.
    let name = Length::FillPortion(2);
    let group = Length::FillPortion(2);
    let used_by = Length::FillPortion(5);
    let mut rows = column![table::row(
        row![
            table::heading("Module", name),
            table::heading("Group", group),
            table::heading("In harken", used_by)
        ]
        .spacing(table::SPACING),
        false,
        false,
        false,
    )];
    for (i, e) in ENTRIES.iter().enumerate() {
        let harken = match e.harken.first() {
            Some(first) => first.to_string(),
            None => "—".to_string(),
        };
        rows = rows.push(
            mouse_area(table::row(
                row![
                    table::cell(e.name.into(), name, false, false, false),
                    table::cell(e.group.heading().into(), group, false, false, true),
                    table::cell_cut(harken, used_by, arkui::fit::Cut::End, false, false, e.harken.is_empty()),
                ]
                .spacing(table::SPACING),
                false,
                false,
                i % 2 == 1,
            ))
            .on_press(Message::Go(Place::Module(e.module))),
        );
    }
    column![
        title(
            "arkui".into(),
            "arkui",
            "iced components, the vim keyboard, theming, the glyph table, a picture cache and the address bar — and nothing of any app."
        ),
        intro,
        hero::heading("The catalogue"),
        rows,
    ]
    .spacing(14)
    .into()
}

fn harken<'a>() -> Element<'a, Message> {
    let name = Length::FillPortion(3);
    let platform = Length::FillPortion(1);
    let on = Length::FillPortion(2);
    let at = Length::FillPortion(5);
    let mut rows = column![table::row(
        row![
            table::heading("Element", name),
            table::heading("Where", platform),
            table::heading("Built on", on),
            table::heading("In", at)
        ]
        .spacing(table::SPACING),
        false,
        false,
        false,
    )];
    for (i, h) in HARKEN_ONLY.iter().enumerate() {
        let built = match h.built_on.is_empty() {
            true => "—".to_string(),
            false => h.built_on.join(", "),
        };
        rows = rows.push(table::row(
            column![
                row![
                    table::cell(h.name.into(), name, false, false, false),
                    table::cell(h.platform.into(), platform, false, false, true),
                    table::cell(built, on, false, true, false),
                    table::cell_cut(h.at.into(), at, arkui::fit::Cut::Middle, false, false, true),
                ]
                .spacing(table::SPACING),
                text(h.what).size(12).style(style::dim),
            ]
            .spacing(2),
            false,
            false,
            i % 2 == 1,
        ));
    }
    let used = ENTRIES.iter().filter(|e| !e.harken.is_empty()).count();
    column![
        title(
            "arkui · in harken".into(),
            "In harken",
            "What harken draws that arkui does not have yet — the candidates for lifting in. Each was written for harken first; the ones built on arkui are the easiest to lift."
        ),
        prose([format!(
            "harken's desktop client uses {used} of arkui's {} modules (each module's page says where). The phones' components live in the apps themselves (harken/android/…/ui, harken/ios/Harken) and are listed here with the desktop's.",
            ENTRIES.len()
        )]),
        hero::heading("Not in arkui"),
        rows,
    ]
    .spacing(14)
    .into()
}

fn module<'a>(app: &'a App, e: &'static Entry) -> Element<'a, Message> {
    let example = container(examples::view(app, e.module)).padding(20).width(Length::Fill).style(|t| {
        let mut s = style::outline(8.0)(t);
        s.background = Some(iced::Background::Color(style::surface_color(t)));
        s
    });
    let bullet = |s: &&str| format!("•  {s}");
    column![
        title(format!("arkui::{}", e.module), e.name, e.summary),
        hero::heading("Example"),
        example,
        hero::heading("API"),
        prose(e.api.iter().map(bullet)),
        hero::heading("Rules"),
        match e.rules.is_empty() {
            true => none("No rules beyond its doc comments."),
            false => prose(e.rules.iter().map(bullet)),
        },
        hero::heading("In harken"),
        match e.harken.is_empty() {
            true => none("Not drawn with by harken yet."),
            false => prose(e.harken.iter().map(bullet)),
        },
        hero::heading("On the phones"),
        match e.phones.is_empty() {
            true => none("Nothing for this on the phones."),
            false => prose(e.phones.iter().map(bullet)),
        },
        hero::heading("Source"),
        text(e.source()).size(13).style(style::dim),
    ]
    .spacing(6)
    .into()
}
