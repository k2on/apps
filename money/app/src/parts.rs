//! money's own components (ui/docs/primitives.md, "Inventory"): a
//! category's or a transaction's header card and a goal's progress, Rust
//! compositions of ArkUI's primitives and its card, which the generator
//! writes out as each toolkit's own layout. Only money lays these out, so
//! they are money's, not ArkUI's.

use ark_app::view::*;

/// A card at the top of an inspector: its face, then its actions
/// ([`header_card`]).
#[derive(Clone, Debug)]
pub struct HeaderCard<M> {
    pub title: Val<String>,
    pub figure: Val<String>,
    /// `neutral`, `positive`, `caution`, `negative` or `accent`.
    pub tint: Val<String>,
    pub glyph: Option<Val<String>>,
    /// The figure in a status badge; else large and bold.
    pub badge: bool,
    pub line: Option<Val<String>>,
    pub caption: Option<Val<String>>,
    pub actions: Box<View<M>>,
}

/// A card at the top of an inspector or a detail: `title` (none: the empty
/// text, the glyph alone), `figure` in `tint` ([`HeaderCard`]).
pub fn header_card<M>(title: impl Into<Val<String>>, figure: impl Into<Val<String>>, tint: impl Into<Val<String>>) -> HeaderCard<M> {
    HeaderCard {
        title: title.into(),
        figure: figure.into(),
        tint: tint.into(),
        glyph: None,
        badge: false,
        line: None,
        caption: None,
        actions: Box::new(View::Empty),
    }
}

impl<M> HeaderCard<M> {
    pub fn glyph(mut self, g: impl Into<Val<String>>) -> Self {
        self.glyph = Some(g.into());
        self
    }
    /// The figure in a status badge (a category's available).
    pub fn badge(mut self) -> Self {
        self.badge = true;
        self
    }
    /// What needs attention, in the tint: "Overspent by $21.02".
    pub fn line(mut self, l: impl Into<Val<String>>) -> Self {
        self.line = Some(l.into());
        self
    }
    /// A quiet word under it: the group, the account.
    pub fn caption(mut self, c: impl Into<Val<String>>) -> Self {
        self.caption = Some(c.into());
        self
    }
    /// Its actions, rows under a divider in the card.
    pub fn actions(mut self, a: impl Into<View<M>>) -> Self {
        self.actions = Box::new(a.into());
        self
    }
}

/// A card whose first row is its face — the glyph on its tile, the title,
/// the figure in a status badge or large in its tint, the line in the tint,
/// the caption quiet, all centred and read as one — and whose rows after it
/// are its actions.
impl<M> From<HeaderCard<M>> for View<M> {
    fn from(h: HeaderCard<M>) -> View<M> {
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
}

/// How far toward a goal (`fraction`, 0 to 1), in `tint` (`neutral` the
/// positive's), `note` under it: the bar at a row's inset and the note
/// quiet under it.
pub fn goal_progress<M>(fraction: impl Into<Val<f64>>, tint: impl Into<Val<String>>, note: impl Into<Val<String>>) -> View<M> {
    vstack((meter(fraction, tint), text(note).style(TextStyle::Footnote).ink(Ink::Quiet)))
        .spacing(Space::Xs)
        .align(Align::Start)
        .padding(Edges::Horizontal, Space::L)
        .padding(Edges::Vertical, Space::M)
        .fill_width_at(Align::Start)
        .into()
}

/// A page's headline: what it is (quiet), its figure large in its tint,
/// a caption, and its one action at its end ([`Headline::action`]), on a
/// card — Budget's Ready to Assign with Auto-Assign.
pub struct Headline<M> {
    pub label: Val<String>,
    pub figure: Val<String>,
    pub tint: Val<String>,
    pub caption: Val<String>,
    pub action: Option<(&'static str, &'static str, M)>,
}

/// A headline of `label`, `figure` in `tint`, `caption` under it.
pub fn headline<M>(
    label: impl Into<Val<String>>,
    figure: impl Into<Val<String>>,
    tint: impl Into<Val<String>>,
    caption: impl Into<Val<String>>,
) -> Headline<M> {
    Headline {
        label: label.into(),
        figure: figure.into(),
        tint: tint.into(),
        caption: caption.into(),
        action: None,
    }
}

impl<M> Headline<M> {
    /// Its one action at its end: `title` with `glyph`, sending `tap`.
    pub fn action(mut self, title: &'static str, glyph: &'static str, tap: M) -> Self {
        self.action = Some((title, glyph, tap));
        self
    }
}

impl<M> From<Headline<M>> for View<M> {
    fn from(h: Headline<M>) -> View<M> {
        let words = vstack((
            text(h.label).style(TextStyle::Subheadline).ink(Ink::Quiet),
            text(h.figure).style(TextStyle::Figure).ink(Ink::Figure(h.tint)).lines(1),
            text(h.caption).style(TextStyle::Footnote).ink(Ink::Quiet).if_any(),
        ))
        .align(Align::Start)
        .combine();
        let action: View<M> = match h.action {
            Some((title, glyph, tap)) => capsule_button(title, true, tap).glyph(glyph).into(),
            None => View::Empty,
        };
        card_section(
            hstack((View::from(words), spacer_min(Space::S), action))
                .spacing(Space::L)
                .padding(Edges::Horizontal, Space::L)
                .padding(Edges::Vertical, Space::M)
                .fill_width_at(Align::Start),
        )
        .into()
    }
}
