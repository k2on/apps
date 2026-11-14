//! Long-form writing in iced (`ark_app::writing`), as the browser draws it
//! (`arkui_web::writing`): headings, paragraphs with their links, emphasis
//! and code, lists, highlighted code in Fira Mono, callouts, quotes,
//! tables and rules, in a column of a reader's measure. Colours are the
//! theme's, asked for as each part is drawn, so the same writing is right
//! in light and dark. Nothing in it moves.
//!
//! A link sends `on_link(href)` when one is given (`markdown_with`); a
//! view's `writing` part draws its links underlined and still.

use ark_app::writing::{self, Block, Callout, Inline, Token};
use iced::font::{Style as Slant, Weight};
use iced::widget::text::Span;
use iced::widget::{column, container, rich_text, row, span, text, Column};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Theme};

use crate::fonts::MONO;
use crate::style;

/// The widest a column of writing is laid out, in logical pixels.
pub const MEASURE: f32 = 680.0;
/// The body's size; headings are drawn from it.
pub const BODY: f32 = 15.0;

/// `markdown` read and drawn, its links still.
pub fn markdown<'a, M: Clone + 'a>(markdown: &str) -> Element<'a, M> {
    blocks(&writing::parse(markdown), None)
}

/// `markdown` read and drawn, a link sending `on_link` of where it goes.
pub fn markdown_with<'a, M: Clone + 'a>(markdown: &str, on_link: fn(String) -> M) -> Element<'a, M> {
    blocks(&writing::parse(markdown), Some(on_link))
}

/// `blocks` in a column of writing's measure.
pub fn blocks<'a, M: Clone + 'a>(blocks: &[Block], on_link: Option<fn(String) -> M>) -> Element<'a, M> {
    container(each(blocks, on_link)).max_width(MEASURE).into()
}

fn each<'a, M: Clone + 'a>(blocks: &[Block], on_link: Option<fn(String) -> M>) -> Column<'a, M> {
    Column::with_children(blocks.iter().map(|b| block(b, on_link))).spacing(12)
}

/// One block.
pub fn block<'a, M: Clone + 'a>(b: &Block, on_link: Option<fn(String) -> M>) -> Element<'a, M> {
    match b {
        Block::Heading { level, text, .. } => {
            let size = match level {
                1 => 28.0,
                2 => 21.0,
                3 => 17.0,
                _ => BODY,
            };
            let bold = Font {
                weight: Weight::Bold,
                ..Font::DEFAULT
            };
            container(words(text, size, Some(bold), on_link))
                .padding(Padding::ZERO.top(if *level == 1 { 0.0 } else { 10.0 }))
                .into()
        }
        Block::Paragraph(text) => words(text, BODY, None, on_link),
        Block::List { ordered, start, items } => Column::with_children(items.iter().enumerate().map(|(i, item)| {
            let mark = match ordered {
                true => format!("{}.", *start as usize + i),
                false => "•".to_string(),
            };
            row![
                container(text(mark).size(BODY).style(style::dim)).width(22),
                each(item, on_link).spacing(4).width(Length::Fill)
            ]
            .into()
        }))
        .spacing(4)
        .into(),
        Block::Code { lang, text } => code(lang, text),
        Block::Callout { kind, blocks } => {
            let kind = *kind;
            container(
                column![
                    text(kind.title())
                        .size(BODY)
                        .font(Font {
                            weight: Weight::Bold,
                            ..Font::DEFAULT
                        })
                        .style(move |t| text::Style {
                            color: Some(callout(t, kind)),
                            ..Default::default()
                        }),
                    each(blocks, on_link).spacing(8)
                ]
                .spacing(4),
            )
            .padding(Padding::from([10, 14]))
            .width(Length::Fill)
            .style(move |t| container::Style {
                background: Some(Background::Color(callout(t, kind).scale_alpha(0.08))),
                border: Border {
                    color: callout(t, kind).scale_alpha(0.5),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .into()
        }
        Block::Quote(blocks) => row![
            container(text("")).width(3).height(Length::Fill).style(|t| container::Style {
                background: Some(Background::Color(style::edge_color(t))),
                ..Default::default()
            }),
            each(blocks, on_link).spacing(8)
        ]
        .spacing(12)
        .height(Length::Shrink)
        .into(),
        Block::Table { head, rows } => {
            let cell = |c: &Vec<Inline>, head: bool| -> Element<'a, M> {
                let font = head.then_some(Font {
                    weight: Weight::Semibold,
                    ..Font::DEFAULT
                });
                container(words(c, 14.0, font, on_link))
                    .width(Length::Fill)
                    .padding(Padding::from([6, 12]).left(0))
                    .into()
            };
            let line = |cells: &Vec<Vec<Inline>>, head: bool| -> Element<'a, M> { row(cells.iter().map(|c| cell(c, head))).into() };
            let mut c = column![line(head, true)];
            for r in rows {
                c = c.push(iced::widget::rule::horizontal(1).style(style::edge)).push(line(r, false));
            }
            c.into()
        }
        Block::Image { alt, .. } => text(alt.clone()).size(13).style(style::dim).into(),
        Block::Rule => iced::widget::rule::horizontal(1).style(style::edge).into(),
    }
}

/// What a callout's edge and title are drawn in.
fn callout(t: &Theme, kind: Callout) -> Color {
    let p = crate::theme::of(t);
    match kind {
        Callout::Note => p.primary.base.color,
        Callout::Tip => p.success.base.color,
        Callout::Important => crate::theme::chart(t, 2),
        Callout::Warning => p.warning.base.color,
        Callout::Caution => p.danger.base.color,
    }
}

/// Words as rich text: emphasis slanted, strong bold, code in Fira Mono
/// on a quiet ground, a link underlined.
fn words<'a, M: Clone + 'a>(words: &[Inline], size: f32, font: Option<Font>, on_link: Option<fn(String) -> M>) -> Element<'a, M> {
    let mut spans: Vec<Span<'a, String>> = vec![];
    flat(words, font.unwrap_or(Font::DEFAULT), None, &mut spans);
    let r = rich_text(spans).size(size).line_height(crate::text::LINE_HEIGHT);
    match on_link {
        Some(f) => r.on_link_click(f).into(),
        None => r.into(),
    }
}

fn flat<'a>(words: &[Inline], font: Font, link: Option<&str>, out: &mut Vec<Span<'a, String>>) {
    for w in words {
        match w {
            Inline::Text(t) => out.push(span(t.clone()).font(font).link_maybe(link.map(String::from)).underline(link.is_some())),
            Inline::Emphasis(i) => flat(
                i,
                Font {
                    style: Slant::Italic,
                    ..font
                },
                link,
                out,
            ),
            Inline::Strong(i) => flat(
                i,
                Font {
                    weight: Weight::Bold,
                    ..font
                },
                link,
                out,
            ),
            Inline::Code(c) => out.push(
                span(c.clone())
                    .font(MONO)
                    .size(BODY * 0.88)
                    .background(Color::from_rgba(0.5, 0.5, 0.5, 0.16))
                    .border(Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    })
                    .padding(Padding::from([0, 3]))
                    .link_maybe(link.map(String::from)),
            ),
            Inline::Link { href, text } => flat(text, font, Some(href), out),
            Inline::Break => out.push(span("\n")),
        }
    }
}

/// `code` in `lang`, highlighted (`ark_app::writing::highlight`), a line a
/// row, on the surface's ground.
pub fn code<'a, M: 'a>(lang: &str, code: &str) -> Element<'a, M> {
    let mut lines: Vec<Vec<(Token, String)>> = vec![vec![]];
    for (t, s) in writing::highlight(lang, code) {
        for (n, piece) in s.split('\n').enumerate() {
            if n > 0 {
                lines.push(vec![]);
            }
            if !piece.is_empty() {
                lines.last_mut().expect("a line").push((t, piece.to_string()));
            }
        }
    }
    let body = Column::with_children(lines.into_iter().map(|l| {
        row(l.into_iter().map(|(t, s)| {
            text(s)
                .font(match t {
                    Token::Keyword => Font {
                        weight: Weight::Bold,
                        ..MONO
                    },
                    Token::Comment => Font {
                        style: Slant::Italic,
                        ..MONO
                    },
                    _ => MONO,
                })
                .size(13)
                .wrapping(text::Wrapping::None)
                .style(move |th| text::Style {
                    color: tint(th, t),
                    ..Default::default()
                })
                .into()
        }))
        .height(20)
        .into()
    }));
    container(iced::widget::scrollable(body).direction(iced::widget::scrollable::Direction::Horizontal(Default::default())))
        .padding(Padding::from([12, 14]))
        .width(Length::Fill)
        .style(|t| container::Style {
            background: Some(Background::Color(style::surface_color(t))),
            border: Border {
                color: style::edge_color(t),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// A token's colour, of the theme's roles, as the browser's
/// `ark-tok-<word>` (writing.css).
fn tint(t: &Theme, token: Token) -> Option<Color> {
    let p = crate::theme::of(t);
    match token {
        Token::Plain => None,
        Token::Keyword => Some(p.primary.base.color),
        Token::Type => Some(crate::theme::chart(t, 1)),
        Token::Function => Some(crate::theme::chart(t, 0)),
        Token::String => Some(p.success.base.color),
        Token::Number => Some(crate::theme::chart(t, 3)),
        Token::Comment => style::dim(t).color,
        Token::Attribute => Some(p.warning.base.color),
    }
}
