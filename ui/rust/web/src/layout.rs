//! The layout primitives (`ark_app::view::layout`, ui/docs/primitives.md)
//! in the DOM: a stack a flex box (`.ark-stack`), a spacer what is left of
//! it, a scroll an overflowing box, words a block in a type style's class
//! (`.ark-type-*`, over `[layout]`'s `type-*`), a glyph the Lucide shape at
//! the words' size, and each modifier a class over the theme's variables
//! (`--ark-space-*`, `--ark-corner`, the containers). Leading and trailing
//! are the flex box's start and end, so a right-to-left page flips them.

use ark_app::view::{Align, Axis, Corner, Edges, Ink, Layout, Modifier, Part, Prim, View};
use leptos::html;
use leptos::prelude::*;

use crate::figures::Tint;
use crate::glyph;
use crate::parts::{flatten, part};

/// `l` drawn, or nothing (words with `if_any` and none).
pub(crate) fn layout<M: Clone + 'static>(l: Layout<M>) -> Option<AnyView> {
    if !l.shown() {
        return None;
    }
    let mut class = vec![];
    let mut group = false;
    for m in &l.mods {
        match m {
            Modifier::Padding(e, s) => class.push(format!("ark-pad-{}-{}", edges(*e), s.case())),
            Modifier::FillWidth(a) => class.push(format!("ark-fill-w ark-fill-{}", align(*a))),
            Modifier::FillHeight => class.push("ark-fill-h".into()),
            Modifier::Background(s) => class.push(format!("ark-surface-{}", s.case())),
            Modifier::Corner(Corner::Card) => class.push("ark-corner-card".into()),
            Modifier::Corner(Corner::Small) => class.push("ark-corner-small".into()),
            Modifier::Combine => group = true,
        }
    }
    let role = group.then_some("group");
    let with = |class: &mut Vec<String>, more: String| {
        class.push(more);
        class.join(" ")
    };
    Some(match l.prim {
        Prim::Stack {
            axis,
            align: a,
            spacing,
            content,
        } => {
            let gap = spacing.map(|s| format!(" ark-gap-{}", s.case())).unwrap_or_default();
            let c = with(&mut class, format!("ark-stack ark-{} ark-align-{}{gap}", axis_class(axis), align(a)));
            html::div().class(c).attr("role", role).child(held(*content)).into_any()
        }
        Prim::Spacer { min } => {
            let least = min.map(|s| format!(" ark-min-{}", s.case())).unwrap_or_default();
            html::div().class(with(&mut class, format!("ark-spacer{least}"))).into_any()
        }
        Prim::Scroll { axis, content } => {
            let c = with(&mut class, format!("ark-stack ark-{} ark-scroll", axis_class(axis)));
            html::div().class(c).attr("role", role).child(held(*content)).into_any()
        }
        Prim::Text(w) => {
            let mut c = format!("ark-text ark-type-{} {}", w.style.token().trim_start_matches("type-"), ink(&w.ink));
            if w.lines == Some(1) {
                c.push_str(" ark-one-line");
            }
            if w.centred {
                c.push_str(" ark-centred");
            }
            html::div().class(with(&mut class, c)).attr("role", role).child(w.text.get()).into_any()
        }
        Prim::Glyph { name, style, ink: i } => {
            let c = with(
                &mut class,
                format!("ark-glyphed ark-type-{} {}", style.token().trim_start_matches("type-"), ink(&i)),
            );
            html::span().class(c).attr("aria-hidden", "true").child(glyph(&name.get(), 16)).into_any()
        }
        Prim::Image { url, label } => {
            let photo = part(View::<M>::Part(Part::Photo { url, label, zoom: false }))?;
            match class.is_empty() {
                true => photo,
                false => html::div().class(class.join(" ")).attr("role", role).child(photo).into_any(),
            }
        }
    })
}

fn held<M: Clone + 'static>(content: View<M>) -> Vec<AnyView> {
    flatten(content).into_iter().filter_map(part).collect()
}

fn axis_class(a: Axis) -> &'static str {
    match a {
        Axis::Vertical => "v",
        Axis::Horizontal => "h",
        Axis::Depth => "z",
    }
}

fn align(a: Align) -> &'static str {
    match a {
        Align::Start => "start",
        Align::Center => "center",
        Align::End => "end",
    }
}

fn edges(e: Edges) -> &'static str {
    match e {
        Edges::All => "all",
        Edges::Horizontal => "h",
        Edges::Vertical => "v",
        Edges::Top => "top",
        Edges::Bottom => "bottom",
        Edges::Leading => "start",
        Edges::Trailing => "end",
    }
}

fn ink(i: &Ink) -> String {
    match i {
        Ink::Ink => "ark-ink".into(),
        Ink::Quiet => "ark-ink-quiet".into(),
        Ink::Accent => "ark-ink-accent".into(),
        Ink::Tint(t) => format!("ark-ink-tint {}", Tint::named(&t.get()).class()),
        Ink::Figure(t) => format!("ark-ink-figure {}", Tint::named(&t.get()).class()),
    }
}
