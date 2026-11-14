//! Long-form writing in the DOM (`ark_app::writing`): a help page, a
//! release note, the docs — headings with anchors, paragraphs with their
//! links, emphasis and code, lists, highlighted code, callouts, quotes,
//! tables and pictures, as the HTML that says each (`h2`, `p`, `pre`,
//! `aside`, …), in [`WRITING_CSS`] over the theme's variables. As iced's
//! (`arkui::writing`), it is the same blocks drawn: written as HTML it is
//! whole without script, and nothing in it moves.

use ark_app::writing::{self, Block, Inline};
use leptos::html;
use leptos::prelude::*;

use crate::glyph;

/// The writing's own styles, over the theme's variables: put after
/// `ARKUI_CSS` by an app that shows writing.
pub const WRITING_CSS: &str = include_str!("../writing.css");

/// `markdown` read (`ark_app::writing::parse`) and drawn.
pub fn markdown(markdown: &str) -> AnyView {
    blocks(&writing::parse(markdown))
}

/// `blocks` drawn, one after another, in a column of writing's measure.
pub fn blocks(blocks: &[Block]) -> AnyView {
    html::div().class("ark-writing").child(each(blocks)).into_any()
}

fn each(blocks: &[Block]) -> Vec<AnyView> {
    blocks.iter().map(block).collect()
}

/// One block.
pub fn block(b: &Block) -> AnyView {
    match b {
        Block::Heading { level, id, text } => {
            let words = inlines(text);
            let id = id.clone();
            match level {
                1 => html::h1().id(id).child(words).into_any(),
                2 => html::h2().id(id).child(words).into_any(),
                3 => html::h3().id(id).child(words).into_any(),
                4 => html::h4().id(id).child(words).into_any(),
                5 => html::h5().id(id).child(words).into_any(),
                _ => html::h6().id(id).child(words).into_any(),
            }
        }
        Block::Paragraph(text) => html::p().child(inlines(text)).into_any(),
        Block::List { ordered, start, items } => {
            let items: Vec<AnyView> = items.iter().map(|i| html::li().child(item(i)).into_any()).collect();
            match ordered {
                true => html::ol().attr("start", (*start != 1).then(|| start.to_string())).child(items).into_any(),
                false => html::ul().child(items).into_any(),
            }
        }
        Block::Code { lang, text } => code(lang, text),
        Block::Callout { kind, blocks } => html::aside()
            .class("ark-callout")
            .attr("data-kind", kind.word())
            .attr("aria-label", kind.title())
            .child((
                html::p()
                    .class("ark-callout-title")
                    .child((glyph(kind.glyph(), 16), html::span().child(kind.title()))),
                each(blocks),
            ))
            .into_any(),
        Block::Quote(blocks) => html::blockquote().child(each(blocks)).into_any(),
        Block::Table { head, rows } => html::div()
            .class("ark-writing-table")
            .child(
                html::table().child((
                    html::thead().child(html::tr().child(head.iter().map(|c| html::th().child(inlines(c))).collect::<Vec<_>>())),
                    html::tbody().child(
                        rows.iter()
                            .map(|r| html::tr().child(r.iter().map(|c| html::td().child(inlines(c))).collect::<Vec<_>>()))
                            .collect::<Vec<_>>(),
                    ),
                )),
            )
            .into_any(),
        Block::Image { alt, src, title } => html::figure()
            .child((
                html::img()
                    .attr("src", src.clone())
                    .attr("alt", alt.clone())
                    .attr("loading", "lazy")
                    .attr("decoding", "async"),
                (!title.is_empty()).then(|| html::figcaption().child(title.clone())),
            ))
            .into_any(),
        Block::Rule => html::hr().into_any(),
    }
}

/// A list item: a lone paragraph is its words, as a tight list's are.
fn item(blocks: &[Block]) -> Vec<AnyView> {
    match blocks {
        [Block::Paragraph(text), rest @ ..] => {
            let mut out = inlines(text);
            out.extend(each(rest));
            out
        }
        _ => each(blocks),
    }
}

/// Words, with their emphasis, code and links.
pub fn inlines(words: &[Inline]) -> Vec<AnyView> {
    words
        .iter()
        .map(|w| match w {
            Inline::Text(t) => t.clone().into_any(),
            Inline::Emphasis(i) => html::em().child(inlines(i)).into_any(),
            Inline::Strong(i) => html::strong().child(inlines(i)).into_any(),
            Inline::Code(c) => html::code().child(c.clone()).into_any(),
            Inline::Link { href, text } => {
                let away = href.starts_with("http://") || href.starts_with("https://");
                html::a()
                    .attr("href", href.clone())
                    .attr("rel", away.then_some("noopener"))
                    .child(inlines(text))
                    .into_any()
            }
            Inline::Break => html::br().into_any(),
        })
        .collect()
}

/// `text` in `lang`, highlighted (`ark_app::writing::highlight`): each
/// piece a span of its token's class (`ark-tok-keyword`), the plain bare.
pub fn code(lang: &str, text: &str) -> AnyView {
    let pieces: Vec<AnyView> = writing::highlight(lang, text)
        .into_iter()
        .map(|(t, s)| match t {
            writing::Token::Plain => s.into_any(),
            t => html::span().class(format!("ark-tok-{}", t.word())).child(s).into_any(),
        })
        .collect();
    html::pre()
        .class("ark-code")
        .attr("data-lang", (!lang.is_empty()).then(|| lang.to_string()))
        .child(html::code().child(pieces))
        .into_any()
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use leptos::prelude::RenderHtml;

    /// Falsified by drawing a callout as a quote, or code unhighlighted.
    #[test]
    fn writing_is_the_html_that_says_it() {
        let html =
            super::markdown("## Two Words\n\nSee [`menu`](../menu/) *now*.\n\n> [!WARNING]\n> Careful.\n\n```rust\nfn a() {}\n```\n").to_html();
        for want in [
            "<h2 id=\"two-words\">",
            "<a href=\"../menu/\"><code>menu</code>",
            "<em>now",
            "data-kind=\"warning\" aria-label=\"Warning\" class=\"ark-callout\"",
            "<span class=\"ark-tok-keyword\">fn</span>",
        ] {
            assert!(html.contains(want), "{want} is not in {html}");
        }
    }
}

/// The way to the page before and the page after, at a page's foot, each
/// its title and where it is (`href`): links, so they work without script
/// and an app that takes its links over walks them.
pub fn pager(previous: Option<(&str, &str)>, next: Option<(&str, &str)>) -> AnyView {
    let one = |(title, href): (&str, &str), class: &'static str, said: &'static str, g: &'static str| {
        let mark = glyph(g, 12);
        let words = html::span().child(said);
        html::a()
            .class(class)
            .attr("href", href.to_string())
            .attr("rel", if class == "ark-pager-next" { "next" } else { "prev" })
            .child((
                match class {
                    "ark-pager-next" => html::span().class("ark-pager-said").child((words, mark)).into_any(),
                    _ => html::span().class("ark-pager-said").child((mark, words)).into_any(),
                },
                html::span().class("ark-pager-title").child(title.to_string()),
            ))
            .into_any()
    };
    html::nav()
        .class("ark-pager")
        .attr("aria-label", "Pages")
        .child((
            previous.map(|p| one(p, "ark-pager-previous", "Previous", "chevron-left")),
            next.map(|n| one(n, "ark-pager-next", "Next", "chevron-right")),
        ))
        .into_any()
}
