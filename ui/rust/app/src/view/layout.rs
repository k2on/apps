//! The layout primitives under ArkUI's parts (ui/docs/primitives.md):
//! stacks, a spacer, a scroll, text, a glyph and an image, and the few
//! modifiers every one of them takes. The generator writes each out as the
//! toolkit's own layout (SwiftUI's `VStack`, Compose's `Column`, iced's
//! `column`, the browser's flex box), so a purely visual part is laid out
//! once, here, and drawn natively everywhere.
//!
//! A primitive takes no number: every gap and inset is a [`Space`], every
//! corner a [`Corner`], every colour an [`Ink`] or a [`Surface`] and every
//! face a [`TextStyle`], each one of `ui/theme.toml`'s.
//!
//! What carries a platform's behaviour (a list, a section, a sheet, a
//! field, navigation) stays a native part and is never rebuilt of these.

use std::fmt::Debug;

use super::{Part, Val, View};

// * The tokens

/// A gap or an inset: one of `ui/theme.toml`'s `[layout]` steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// `space-xs`: a figure from its label.
    Xs,
    /// `space-s`: a glyph from its words.
    S,
    /// `space-m`: a row's inset above and below its words.
    M,
    /// `space-l`: a card's inset.
    L,
    /// `space-xl`: inside a header card.
    Xl,
    /// `space-xxl`: a first-run page's mark from its words.
    Xxl,
    /// `page-margin`: a page's edges to its cards.
    Page,
    /// `section-gap`: one part from the next.
    Section,
}

impl Space {
    /// As the phones' `Space` spells it: `Space.xs` in Swift and Kotlin.
    pub fn case(self) -> &'static str {
        match self {
            Space::Xs => "xs",
            Space::S => "s",
            Space::M => "m",
            Space::L => "l",
            Space::Xl => "xl",
            Space::Xxl => "xxl",
            Space::Page => "page",
            Space::Section => "section",
        }
    }

    /// As `ui/theme.toml` names it: the browser's `--ark-<name>`.
    pub fn token(self) -> &'static str {
        match self {
            Space::Xs => "space-xs",
            Space::S => "space-s",
            Space::M => "space-m",
            Space::L => "space-l",
            Space::Xl => "space-xl",
            Space::Xxl => "space-xxl",
            Space::Page => "page-margin",
            Space::Section => "section-gap",
        }
    }
}

/// A corner: `card-corner` or `corner`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    Card,
    Small,
}

/// A face words are set in: the system's named styles on the phones (so
/// Dynamic Type and the font scale grow them), `[layout]`'s `type-*` sizes
/// on the desktop and in the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextStyle {
    LargeTitle,
    Title,
    /// A card's title: bold.
    Headline,
    #[default]
    Body,
    /// A line under a title.
    Subheadline,
    /// A note under a part.
    Footnote,
    /// The smallest words.
    Caption,
    /// A card's large figure: bold, rounded on Apple's, its digits in columns.
    Figure,
}

impl TextStyle {
    /// As the phones' `ArkType` spells it: `.headline` in Swift,
    /// `ArkType.Headline` in Kotlin.
    pub fn case(self) -> &'static str {
        match self {
            TextStyle::LargeTitle => "largeTitle",
            TextStyle::Title => "title",
            TextStyle::Headline => "headline",
            TextStyle::Body => "body",
            TextStyle::Subheadline => "subheadline",
            TextStyle::Footnote => "footnote",
            TextStyle::Caption => "caption",
            TextStyle::Figure => "figure",
        }
    }

    /// As `ui/theme.toml` names its desktop size: `type-headline`.
    pub fn token(self) -> &'static str {
        match self {
            TextStyle::LargeTitle => "type-large-title",
            TextStyle::Title => "type-title",
            TextStyle::Headline => "type-headline",
            TextStyle::Body => "type-body",
            TextStyle::Subheadline => "type-subheadline",
            TextStyle::Footnote => "type-footnote",
            TextStyle::Caption => "type-caption",
            TextStyle::Figure => "type-figure",
        }
    }

    /// Set bold on the desktop and in the browser (the phones' styles say
    /// their own weight).
    pub fn bold(self) -> bool {
        matches!(self, TextStyle::LargeTitle | TextStyle::Title | TextStyle::Headline | TextStyle::Figure)
    }
}

/// The colour words or a glyph are drawn in: one of the theme's roles.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Ink {
    /// The words' colour.
    #[default]
    Ink,
    /// Quieter: what is under a title.
    Quiet,
    /// The app's accent, as words read on the page.
    Accent,
    /// A tint by name (`neutral`, `positive`, `caution`, `negative`,
    /// `accent`): `neutral` is the quiet colour.
    Tint(Val<String>),
    /// A figure in a tint by name: `neutral` is the words' colour.
    Figure(Val<String>),
}

/// What is behind a primitive: one of the theme's containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// A card's container (`container-low`).
    Card,
    /// A container above a card (`container-high`): a chip, a well.
    Raised,
}

impl Surface {
    pub fn case(self) -> &'static str {
        match self {
            Surface::Card => "card",
            Surface::Raised => "raised",
        }
    }
}

/// Where a stack's content sits across its axis, or a filled frame's
/// content: leading and trailing, so a right-to-left language flips it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    Start,
    #[default]
    Center,
    End,
}

/// Which edges a padding is on: leading and trailing, never left and right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edges {
    All,
    Horizontal,
    Vertical,
    Top,
    Bottom,
    Leading,
    Trailing,
}

/// Which way a stack lays out or a scroll scrolls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
    /// One over another (`zstack`).
    Depth,
}

/// What every primitive takes, applied in the order it is written
/// (SwiftUI's order; the generator reverses it for Compose's chain).
#[derive(Clone, Debug, PartialEq)]
pub enum Modifier {
    Padding(Edges, Space),
    /// As wide as it is given, its content at `Align`.
    FillWidth(Align),
    /// As tall as it is given.
    FillHeight,
    Background(Surface),
    /// Clipped to the corner.
    Corner(Corner),
    /// One element to a screen reader, its words in order.
    Combine,
}

// * The primitives

/// A primitive and its modifiers ([`View::Layout`]).
#[derive(Clone, Debug)]
pub struct Layout<M> {
    pub prim: Prim<M>,
    pub mods: Vec<Modifier>,
}

/// One of the primitives.
#[derive(Clone, Debug)]
pub enum Prim<M> {
    /// Its content one after another along `axis`, `spacing` apart (none:
    /// touching), at `align` across it.
    Stack {
        axis: Axis,
        align: Align,
        spacing: Option<Space>,
        content: Box<View<M>>,
    },
    /// What is left along its stack's axis, at least `min`.
    Spacer {
        min: Option<Space>,
    },
    /// Its content, scrolling along `axis`.
    Scroll {
        axis: Axis,
        content: Box<View<M>>,
    },
    Text(Words),
    /// A glyph (a Lucide name; the SF Symbol it maps to on Apple's), as
    /// large as `style`'s words, in `ink`. Decorative: said by the words
    /// beside it.
    Glyph {
        name: Val<String>,
        style: TextStyle,
        ink: Ink,
    },
    /// A picture by its URL, fitted to its width (`photo`'s).
    Image {
        url: Val<String>,
        label: Val<String>,
    },
}

/// Words, in a [`TextStyle`] and an [`Ink`].
#[derive(Clone, Debug)]
pub struct Words {
    pub text: Val<String>,
    pub style: TextStyle,
    pub ink: Ink,
    /// At most this many lines, the last cut short; none: as many as it takes.
    pub lines: Option<u8>,
    /// Its lines centred.
    pub centred: bool,
    /// Nothing at all while it is empty.
    pub if_any: bool,
}

impl<M> Prim<M> {
    /// What it is, in a sentence's words.
    pub fn kind(&self) -> &'static str {
        match self {
            Prim::Stack { .. } => "stack",
            Prim::Spacer { .. } => "spacer",
            Prim::Scroll { .. } => "scroll",
            Prim::Text(_) => "text",
            Prim::Glyph { .. } => "glyph",
            Prim::Image { .. } => "image",
        }
    }
}

impl<M> Layout<M> {
    fn of(prim: Prim<M>) -> Layout<M> {
        Layout { prim, mods: vec![] }
    }

    /// The views it holds.
    pub fn slots(&self) -> Vec<&View<M>> {
        match &self.prim {
            Prim::Stack { content, .. } | Prim::Scroll { content, .. } => vec![&**content],
            _ => vec![],
        }
    }

    /// Whether it is drawn at all: text with `if_any` and no words is not.
    pub fn shown(&self) -> bool {
        match &self.prim {
            Prim::Text(w) => !(w.if_any && matches!(&w.text, Val::Is(t) if t.is_empty())),
            _ => true,
        }
    }

    // A stack's.

    /// Its content `s` apart (a stack's).
    pub fn spacing(mut self, s: Space) -> Self {
        match &mut self.prim {
            Prim::Stack { spacing, .. } => *spacing = Some(s),
            p => panic!("`.spacing` is a stack's, not a {}", p.kind()),
        }
        self
    }

    /// Its content at `a` across its axis (a stack's).
    pub fn align(mut self, a: Align) -> Self {
        match &mut self.prim {
            Prim::Stack { align, .. } => *align = a,
            p => panic!("`.align` is a stack's, not a {}", p.kind()),
        }
        self
    }

    // Words' and a glyph's.

    /// Set in `s` (words' and a glyph's).
    pub fn style(mut self, s: TextStyle) -> Self {
        match &mut self.prim {
            Prim::Text(w) => w.style = s,
            Prim::Glyph { style, .. } => *style = s,
            p => panic!("`.style` is words' or a glyph's, not a {}", p.kind()),
        }
        self
    }

    /// Drawn in `i` (words' and a glyph's).
    pub fn ink(mut self, i: Ink) -> Self {
        match &mut self.prim {
            Prim::Text(w) => w.ink = i,
            Prim::Glyph { ink, .. } => *ink = i,
            p => panic!("`.ink` is words' or a glyph's, not a {}", p.kind()),
        }
        self
    }

    /// At most `n` lines (words').
    pub fn lines(mut self, n: u8) -> Self {
        self.words("lines").lines = Some(n);
        self
    }

    /// Its lines centred (words').
    pub fn centred(mut self) -> Self {
        self.words("centred").centred = true;
        self
    }

    /// Nothing at all while its words are empty (words').
    pub fn if_any(mut self) -> Self {
        self.words("if_any").if_any = true;
        self
    }

    fn words(&mut self, what: &str) -> &mut Words {
        match &mut self.prim {
            Prim::Text(w) => w,
            p => panic!("`.{what}` is words', not a {}", p.kind()),
        }
    }
}

/// The modifiers every primitive takes (ui/docs/primitives.md).
pub trait Modify: Sized {
    fn modify(self, m: Modifier) -> Self;

    /// Inset on `edges` by `s`.
    fn padding(self, edges: Edges, s: Space) -> Self {
        self.modify(Modifier::Padding(edges, s))
    }
    /// As wide as it is given, its content centred.
    fn fill_width(self) -> Self {
        self.modify(Modifier::FillWidth(Align::Center))
    }
    /// As wide as it is given, its content at `a`.
    fn fill_width_at(self, a: Align) -> Self {
        self.modify(Modifier::FillWidth(a))
    }
    /// As tall as it is given.
    fn fill_height(self) -> Self {
        self.modify(Modifier::FillHeight)
    }
    /// `s` behind it.
    fn background(self, s: Surface) -> Self {
        self.modify(Modifier::Background(s))
    }
    /// Clipped to `c`.
    fn corner(self, c: Corner) -> Self {
        self.modify(Modifier::Corner(c))
    }
    /// One element to a screen reader.
    fn combine(self) -> Self {
        self.modify(Modifier::Combine)
    }
}

impl<M> Modify for Layout<M> {
    fn modify(mut self, m: Modifier) -> Self {
        self.mods.push(m);
        self
    }
}

impl<M> From<Layout<M>> for View<M> {
    fn from(l: Layout<M>) -> View<M> {
        View::Layout(Box::new(l))
    }
}

fn stack<M>(axis: Axis, content: impl Into<View<M>>) -> Layout<M> {
    Layout::of(Prim::Stack {
        axis,
        align: Align::Center,
        spacing: None,
        content: Box::new(content.into()),
    })
}

/// Its content one under another (SwiftUI's `VStack`, Compose's `Column`).
pub fn vstack<M>(content: impl Into<View<M>>) -> Layout<M> {
    stack(Axis::Vertical, content)
}

/// Its content side by side (`HStack`, `Row`).
pub fn hstack<M>(content: impl Into<View<M>>) -> Layout<M> {
    stack(Axis::Horizontal, content)
}

/// Its content one over another, the first at the back (`ZStack`, `Box`).
pub fn zstack<M>(content: impl Into<View<M>>) -> Layout<M> {
    stack(Axis::Depth, content)
}

/// What is left along its stack's axis.
pub fn spacer<M>() -> Layout<M> {
    Layout::of(Prim::Spacer { min: None })
}

/// What is left along its stack's axis, at least `min`.
pub fn spacer_min<M>(min: Space) -> Layout<M> {
    Layout::of(Prim::Spacer { min: Some(min) })
}

/// Its content, scrolling down.
pub fn scroll<M>(content: impl Into<View<M>>) -> Layout<M> {
    Layout::of(Prim::Scroll {
        axis: Axis::Vertical,
        content: Box::new(content.into()),
    })
}

/// Its content, scrolling sideways (a shelf).
pub fn scroll_across<M>(content: impl Into<View<M>>) -> Layout<M> {
    Layout::of(Prim::Scroll {
        axis: Axis::Horizontal,
        content: Box::new(content.into()),
    })
}

/// Words, in the body's style and the words' colour until told otherwise.
pub fn text<M>(words: impl Into<Val<String>>) -> Layout<M> {
    Layout::of(Prim::Text(Words {
        text: words.into(),
        style: TextStyle::Body,
        ink: Ink::Ink,
        lines: None,
        centred: false,
        if_any: false,
    }))
}

/// A glyph by its Lucide name, as large as the body's words.
pub fn glyph<M>(name: impl Into<Val<String>>) -> Layout<M> {
    Layout::of(Prim::Glyph {
        name: name.into(),
        style: TextStyle::Body,
        ink: Ink::Ink,
    })
}

/// A picture by its URL (`file:`, `data:` or `https:`), `label` said for it.
pub fn image<M>(url: impl Into<Val<String>>, label: impl Into<Val<String>>) -> Layout<M> {
    Layout::of(Prim::Image {
        url: url.into(),
        label: label.into(),
    })
}

// * Leaves the primitives hold

/// A glyph on a rounded tile of its tint (ArkUI's `Tile`).
pub fn tile<M>(glyph: impl Into<Val<String>>, tint: impl Into<Val<String>>) -> View<M> {
    Part::Tile {
        glyph: glyph.into(),
        tint: tint.into(),
    }
    .into()
}

/// A figure in a capsule of its tint (ArkUI's `StatusBadge`).
pub fn status_badge<M>(figure: impl Into<Val<String>>, tint: impl Into<Val<String>>) -> View<M> {
    Part::StatusBadge {
        figure: figure.into(),
        tint: tint.into(),
    }
    .into()
}

/// A bar of how far (`fraction`, 0 to 1) in a tint, `neutral` the
/// positive's (ArkUI's `MeterBar`: money's records have a `Meter` of their own).
pub fn meter<M>(fraction: impl Into<Val<f64>>, tint: impl Into<Val<String>>) -> View<M> {
    Part::Meter {
        fraction: fraction.into(),
        tint: tint.into(),
    }
    .into()
}

// * Mapping

impl<M: 'static> Layout<M> {
    pub(super) fn map_rc<N: 'static>(self, f: &std::rc::Rc<dyn Fn(M) -> N>) -> Layout<N> {
        let b = |v: Box<View<M>>| Box::new(v.map_rc(f));
        Layout {
            prim: match self.prim {
                Prim::Stack {
                    axis,
                    align,
                    spacing,
                    content,
                } => Prim::Stack {
                    axis,
                    align,
                    spacing,
                    content: b(content),
                },
                Prim::Spacer { min } => Prim::Spacer { min },
                Prim::Scroll { axis, content } => Prim::Scroll { axis, content: b(content) },
                Prim::Text(w) => Prim::Text(w),
                Prim::Glyph { name, style, ink } => Prim::Glyph { name, style, ink },
                Prim::Image { url, label } => Prim::Image { url, label },
            },
            mods: self.mods,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Tap,
    }

    #[test]
    fn a_stack_holds_its_content_and_its_tokens() {
        let v: View<Msg> = vstack((text("Title").style(TextStyle::Headline), text("under").ink(Ink::Quiet)))
            .spacing(Space::Xs)
            .align(Align::Start)
            .padding(Edges::Horizontal, Space::L)
            .fill_width_at(Align::Start)
            .into();
        let View::Layout(l) = v else { panic!("a stack is a layout") };
        let Prim::Stack {
            axis,
            align,
            spacing,
            content,
            ..
        } = &l.prim
        else {
            panic!("a vstack is a stack")
        };
        assert_eq!((*axis, *align, *spacing), (Axis::Vertical, Align::Start, Some(Space::Xs)));
        assert_eq!(
            l.mods,
            vec![Modifier::Padding(Edges::Horizontal, Space::L), Modifier::FillWidth(Align::Start)]
        );
        let View::List(words) = &**content else {
            panic!("its content is a list")
        };
        assert_eq!(words.len(), 2);
    }

    #[test]
    #[should_panic(expected = "`.spacing` is a stack's")]
    fn a_method_of_another_primitive_is_refused() {
        let _: Layout<Msg> = text("x").spacing(Space::M);
    }

    #[test]
    fn empty_words_are_not_drawn_while_if_any() {
        assert!(!text::<Msg>("").if_any().shown());
        assert!(text::<Msg>("").shown());
        assert!(text::<Msg>("Hi").if_any().shown());
        // Recorded, it is the emitters' to ask.
        assert!(text::<Msg>(Val::At("p.title".into())).if_any().shown());
    }

    #[test]
    fn a_mapped_layout_keeps_its_messages_inside() {
        let v: View<Msg> = vstack(list_row("Go").on_tap(Msg::Tap)).into();
        let v = v.map(|m| format!("{m:?}"));
        let View::Layout(l) = v else { panic!() };
        let Prim::Stack { content, .. } = l.prim else { panic!() };
        let View::Part(Part::Row(r)) = *content else { panic!() };
        assert_eq!(r.tap.as_deref(), Some("Tap"));
    }
}
