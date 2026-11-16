//! The components several apps share, built of the primitives
//! (ui/docs/primitives.md): laid out once, here, and drawn by every toolkit
//! through the generator, as no part of their own. Each is generic — no
//! money word, no media word — and used by more than one app; one app's own
//! components are that app's (harken's covers and rows are
//! harken-app's `parts`).

use super::layout::*;
use super::{Val, View};

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
    /// A row's: its title the body's, not bold.
    pub plain: bool,
    /// Each line one line at most, cut short.
    pub one_line: bool,
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
        plain: false,
        one_line: false,
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
    /// A row's: the title in the body's face.
    pub fn plain(mut self) -> Self {
        self.plain = true;
        self
    }
    /// Each line on one line, cut short.
    pub fn one_line(mut self) -> Self {
        self.one_line = true;
        self
    }
}

impl<M> From<TitleStack> for View<M> {
    fn from(t: TitleStack) -> View<M> {
        let (centred, one_line) = (t.centred, t.one_line);
        let line = |words: Val<String>, style: TextStyle, ink: Ink| -> View<M> {
            let w = text(words).style(style).ink(ink).if_any();
            let w = if one_line { w.lines(1) } else { w };
            if centred { w.centred() } else { w }.into()
        };
        let (title, under) = match (t.large, t.plain) {
            (true, _) => (TextStyle::Title, TextStyle::Body),
            (false, true) => (TextStyle::Body, TextStyle::Subheadline),
            (false, false) => (TextStyle::Headline, TextStyle::Subheadline),
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
        // A row's lines touch, as a list row's title and subtitle do.
        let stack = vstack(lines).align(if centred { Align::Center } else { Align::Start });
        match t.plain {
            true => stack.into(),
            false => stack.spacing(Space::Xs).into(),
        }
    }
}

/// What an empty list says (ui/docs/primitives.md, "empty state"): a quiet
/// glyph and what it is waiting for, centred — the iPhone's
/// `ContentUnavailableView`, Android's `Empty`. Harken's library, search
/// and queue; money's register, transactions and insights.
pub fn empty_state<M>(glyph_name: impl Into<Val<String>>, title: impl Into<Val<String>>) -> View<M> {
    vstack((
        glyph(glyph_name).style(TextStyle::LargeTitle).ink(Ink::Quiet),
        title_stack(title).large().centred(),
    ))
    .spacing(Space::L)
    .padding(Edges::Horizontal, Space::Xl)
    .padding(Edges::Vertical, Space::Xxl)
    .fill_width()
    .combine()
    .into()
}
