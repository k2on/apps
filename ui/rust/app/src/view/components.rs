//! ArkUI's components built of the primitives (ui/docs/primitives.md):
//! laid out once, here, and drawn by every toolkit through the generator,
//! as no part of their own. Each is generic — no money word, no media word
//! — and used by more than one app.

use super::layout::*;
use super::{card_section, HeaderCard, Val, View};

/// A block of titles (ui/docs/primitives.md, "title stack"): a kicker over a
/// title over a line under it over a caption, each only while it has
/// words. Harken's players, record headers and rows; money's empty states.
#[derive(Clone, Debug)]
pub struct TitleStack {
    pub over: Option<Val<String>>,
    pub title: Val<String>,
    pub under: Option<Val<String>>,
    pub under_ink: Ink,
    pub caption: Option<Val<String>>,
    /// Centred, each line's words too; else at the leading edge.
    pub centred: bool,
    /// A page's: its title a title, the line under it the body's.
    pub large: bool,
}

/// A title stack of `title` alone.
pub fn title_stack(title: impl Into<Val<String>>) -> TitleStack {
    TitleStack {
        over: None,
        title: title.into(),
        under: None,
        under_ink: Ink::Quiet,
        caption: None,
        centred: false,
        large: false,
    }
}

impl TitleStack {
    /// A kicker over the title: what it is ("Album", "Last Played").
    pub fn over(mut self, s: impl Into<Val<String>>) -> Self {
        self.over = Some(s.into());
        self
    }
    /// A line under the title: who it is by, a sentence.
    pub fn under(mut self, s: impl Into<Val<String>>) -> Self {
        self.under = Some(s.into());
        self
    }
    /// The line under it in `i` (an accent's by-line) rather than quiet.
    pub fn under_ink(mut self, i: Ink) -> Self {
        self.under_ink = i;
        self
    }
    /// The smallest words, last.
    pub fn caption(mut self, s: impl Into<Val<String>>) -> Self {
        self.caption = Some(s.into());
        self
    }
    pub fn centred(mut self) -> Self {
        self.centred = true;
        self
    }
    pub fn large(mut self) -> Self {
        self.large = true;
        self
    }
}

impl<M> From<TitleStack> for View<M> {
    fn from(t: TitleStack) -> View<M> {
        let centred = t.centred;
        let line = |words: Val<String>, style: TextStyle, ink: Ink| -> View<M> {
            let w = text(words).style(style).ink(ink).if_any();
            if centred { w.centred() } else { w }.into()
        };
        let (title, under) = match t.large {
            true => (TextStyle::Title, TextStyle::Body),
            false => (TextStyle::Headline, TextStyle::Subheadline),
        };
        let mut lines = vec![];
        if let Some(o) = t.over {
            lines.push(line(o, TextStyle::Caption, Ink::Quiet));
        }
        lines.push(line(t.title, title, Ink::Ink));
        if let Some(u) = t.under {
            lines.push(line(u, under, t.under_ink));
        }
        if let Some(c) = t.caption {
            lines.push(line(c, TextStyle::Caption, Ink::Quiet));
        }
        vstack(lines)
            .spacing(Space::Xs)
            .align(if centred { Align::Center } else { Align::Start })
            .into()
    }
}

/// What an empty list says (ui/docs/primitives.md, "empty state"): a quiet
/// glyph, what it is waiting for, and what to do about it, centred — the
/// iPhone's `ContentUnavailableView`, Android's `Empty`. Harken's library,
/// search and queue; money's register, transactions and insights.
pub fn empty_state<M>(glyph_name: impl Into<Val<String>>, title: impl Into<Val<String>>, sentence: impl Into<Val<String>>) -> View<M> {
    vstack((
        glyph(glyph_name).style(TextStyle::LargeTitle).ink(Ink::Quiet),
        title_stack(title).under(sentence).large().centred(),
    ))
    .spacing(Space::L)
    .padding(Edges::Horizontal, Space::Xl)
    .padding(Edges::Vertical, Space::Xxl)
    .fill_width()
    .combine()
    .into()
}

/// ArkUI's header card ([`HeaderCard`]), of the primitives: a card whose
/// first row is its face — the glyph on its tile, the title, the figure in
/// a status badge or large in its tint, the line in the tint, the caption
/// quiet, all centred and read as one — and whose rows after it are its
/// actions.
pub(super) fn header_card<M>(h: HeaderCard<M>) -> View<M> {
    let words = |v: Val<String>, style: TextStyle, ink: Ink| -> View<M> { text(v).style(style).ink(ink).centred().if_any().into() };
    let mut face: Vec<View<M>> = vec![];
    if let Some(g) = h.glyph {
        face.push(tile(g, h.tint.clone()));
    }
    face.push(words(h.title, TextStyle::Headline, Ink::Ink));
    face.push(match h.badge {
        true => status_badge(h.figure, h.tint.clone()),
        false => text(h.figure).style(TextStyle::Figure).ink(Ink::Figure(h.tint.clone())).centred().into(),
    });
    if let Some(l) = h.line {
        face.push(words(l, TextStyle::Subheadline, Ink::Tint(h.tint.clone())));
    }
    if let Some(c) = h.caption {
        face.push(words(c, TextStyle::Footnote, Ink::Quiet));
    }
    let face = vstack(face)
        .spacing(Space::Xs)
        .padding(Edges::Horizontal, Space::L)
        .padding(Edges::Vertical, Space::Xl)
        .fill_width()
        .combine();
    card_section((face, *h.actions)).into()
}

/// How far toward a goal (`fraction`, 0 to 1), in `tint` (`neutral` the
/// positive's), `note` under it: of the primitives, the bar at a row's
/// inset and the note quiet under it.
pub fn goal_progress<M>(fraction: impl Into<Val<f64>>, tint: impl Into<Val<String>>, note: impl Into<Val<String>>) -> View<M> {
    vstack((meter(fraction, tint), text(note).style(TextStyle::Footnote).ink(Ink::Quiet)))
        .spacing(Space::Xs)
        .align(Align::Start)
        .padding(Edges::Horizontal, Space::L)
        .padding(Edges::Vertical, Space::M)
        .fill_width_at(Align::Start)
        .into()
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Assign,
    }

    /// The texts a view says, in order, and whether each is quiet.
    fn words<M>(v: &View<M>, out: &mut Vec<(String, Ink, TextStyle)>) {
        match v {
            View::List(vs) => vs.iter().for_each(|v| words(v, out)),
            View::Layout(l) => {
                if let Prim::Text(w) = &l.prim {
                    if l.shown() {
                        out.push((w.text.get(), w.ink.clone(), w.style));
                    }
                }
                l.slots().into_iter().for_each(|s| words(s, out));
            }
            View::Part(p) => p.call().slots.into_iter().for_each(|(_, s)| words(s, out)),
            _ => {}
        }
    }

    #[test]
    fn a_header_card_is_a_card_of_its_face_then_its_actions() {
        let v: View<Msg> = header_card("Groceries", "$40.00", "negative")
            .line("Overspent by $21.02")
            .caption("")
            .actions(list_row("Assign").on_tap(Msg::Assign))
            .into();
        let View::Part(Part::Section(s)) = &v else {
            panic!("a header card is a card: {v:?}")
        };
        let View::List(rows) = &*s.rows else { panic!() };
        assert!(matches!(&rows[0], View::Layout(l) if l.mods.contains(&Modifier::Combine)));
        assert!(matches!(&rows[1], View::Part(Part::Row(_))));
        let mut said = vec![];
        words(&v, &mut said);
        let texts: Vec<_> = said.iter().map(|(t, ..)| t.as_str()).collect();
        // The empty caption is not drawn; the action's row is a part's.
        assert_eq!(texts, ["Groceries", "$40.00", "Overspent by $21.02"]);
        assert_eq!(said[1].1, Ink::Figure(Val::Is("negative".into())));
        assert_eq!(said[1].2, TextStyle::Figure);
        assert_eq!(said[2].1, Ink::Tint(Val::Is("negative".into())));
    }

    #[test]
    fn a_badged_header_card_holds_its_figure_in_a_badge() {
        let v: View<Msg> = header_card("Rent", "$0.00", "positive").badge().glyph("house").into();
        let View::Part(Part::Section(s)) = &v else { panic!() };
        let View::List(rows) = &*s.rows else { panic!() };
        let View::Layout(face) = &rows[0] else { panic!() };
        let Prim::Stack { content, .. } = &face.prim else { panic!() };
        let View::List(face) = &**content else { panic!() };
        assert!(matches!(&face[0], View::Part(Part::Tile { .. })));
        assert!(matches!(&face[2], View::Part(Part::StatusBadge { .. })));
    }

    #[test]
    fn an_empty_state_says_its_title_and_sentence_quietly() {
        let v: View<Msg> = empty_state("list", "No Transactions", "Add one, or connect a bank.");
        let mut said = vec![];
        words(&v, &mut said);
        assert_eq!(
            said,
            [
                ("No Transactions".into(), Ink::Ink, TextStyle::Title),
                ("Add one, or connect a bank.".into(), Ink::Quiet, TextStyle::Body),
            ]
        );
    }

    #[test]
    fn a_title_stack_leaves_out_what_it_was_not_given() {
        let v: View<Msg> = title_stack("Abbey Road").over("Album").under("The Beatles").under_ink(Ink::Accent).into();
        let mut said = vec![];
        words(&v, &mut said);
        let texts: Vec<_> = said.iter().map(|(t, i, s)| (t.as_str(), i.clone(), *s)).collect();
        assert_eq!(
            texts,
            [
                ("Album", Ink::Quiet, TextStyle::Caption),
                ("Abbey Road", Ink::Ink, TextStyle::Headline),
                ("The Beatles", Ink::Accent, TextStyle::Subheadline),
            ]
        );
    }
}
