//! ArkUI's components built of the primitives (ui/docs/primitives.md):
//! laid out once, here, and drawn by every toolkit through the generator,
//! as no part of their own. Each is generic — no money word, no media word
//! — and used by more than one app.

use super::layout::*;
use super::{card_section, pressable, wide_button, ArtSize, Artwork, HeaderCard, MenuItem, Part, Press, Val, View};

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

// * Media (ui/docs/primitives.md: harken's, any app that plays or shows
//   covers)

/// A cover over its title and a quiet caption, pressed as one: a shelf's
/// or a grid's tile (ui/docs/primitives.md, "artwork tile"). Its words are
/// kept to its cover's width, one line each; a person's cover is round.
pub fn artwork_tile<M>(art: Artwork, title: impl Into<Val<String>>, caption: impl Into<Val<String>>, tap: M) -> View<M> {
    let title = title.into();
    let size = art.size;
    let face = vstack((
        View::from(art),
        text(title.clone()).style(TextStyle::Subheadline).lines(1),
        text(caption).style(TextStyle::Caption).ink(Ink::Quiet).lines(1).if_any(),
    ))
    .spacing(Space::Xs)
    .align(Align::Start)
    .art_width(size);
    pressable(title, tap, face).into()
}

/// Tiles in a row that scrolls sideways, at the page's margins: a shelf
/// (ui/docs/primitives.md, Home's shelves, search's albums).
pub fn artwork_shelf<M>(tiles: impl Into<View<M>>) -> View<M> {
    scroll_across(
        hstack(tiles)
            .spacing(Space::M)
            .align(Align::Start)
            .padding(Edges::Horizontal, Space::Page),
    )
    .into()
}

/// A row of something playable or shown with its cover (ui/docs/primitives.md,
/// "media row"): the artwork, its title over the line under it, what goes at
/// its end (a length, a glyph button, a meter), pressed as a row and with a
/// menu of what can be done with it.
pub struct MediaRow<M> {
    pub art: Option<View<M>>,
    pub title: Val<String>,
    pub under: Option<Val<String>>,
    pub trailing: View<M>,
    pub tap: M,
    pub menu: Vec<MenuItem<M>>,
}

/// A media row of `title`, sending `tap`.
pub fn media_row<M>(title: impl Into<Val<String>>, tap: M) -> MediaRow<M> {
    MediaRow {
        art: None,
        title: title.into(),
        under: None,
        trailing: View::Empty,
        tap,
        menu: vec![],
    }
}

impl<M> MediaRow<M> {
    /// Its cover (or a number, a glyph: anything at its start).
    pub fn art(mut self, a: impl Into<View<M>>) -> Self {
        self.art = Some(a.into());
        self
    }
    /// The line under its title: who it is by, when.
    pub fn under(mut self, s: impl Into<Val<String>>) -> Self {
        self.under = Some(s.into());
        self
    }
    /// What goes at its end.
    pub fn trailing(mut self, v: impl Into<View<M>>) -> Self {
        self.trailing = v.into();
        self
    }
    /// An entry of its menu.
    pub fn menu(mut self, title: impl Into<Val<String>>, glyph: &'static str, tap: M) -> Self {
        self.menu.push(MenuItem {
            title: title.into(),
            glyph: glyph.into(),
            tap,
            destructive: Val::Is(false),
            shown: None,
            keys: None,
        });
        self
    }
}

impl<M> From<MediaRow<M>> for View<M> {
    fn from(r: MediaRow<M>) -> View<M> {
        let mut words = title_stack(r.title.clone()).plain().one_line();
        if let Some(u) = r.under {
            words = words.under(u);
        }
        let face = hstack((r.art.unwrap_or(View::Empty), View::from(words), spacer_min(Space::Xs), r.trailing))
            .spacing(Space::M)
            .padding(Edges::Horizontal, Space::L)
            .padding(Edges::Vertical, Space::S)
            .fill_width_at(Align::Start);
        Part::Press {
            press: Press::Highlight,
            label: r.title,
            tap: r.tap,
            menu: r.menu,
            content: Box::new(face.into()),
        }
        .into()
    }
}

/// A record's header (ui/docs/primitives.md, "media header"): its cover at
/// `art-header` beside its titles, then what it offers (`actions`: a
/// [`play_shuffle`] pair) under both.
pub fn media_header<M>(art: Artwork, titles: TitleStack, actions: impl Into<View<M>>) -> View<M> {
    vstack((
        hstack((View::from(art.size(ArtSize::Header)), View::from(titles))).spacing(Space::L),
        actions.into(),
    ))
    .spacing(Space::L)
    .align(Align::Start)
    .padding(Edges::Horizontal, Space::Page)
    .padding(Edges::Vertical, Space::M)
    .fill_width_at(Align::Start)
    .into()
}

/// Play and Shuffle side by side, Play prominent (ui/docs/primitives.md,
/// "play/shuffle pair").
pub fn play_shuffle<M>(play: M, shuffle: M) -> View<M> {
    hstack((
        wide_button("Play", true, true, false, play),
        wide_button("Shuffle", false, true, false, shuffle),
    ))
    .spacing(Space::M)
    .into()
}

/// A glyph pressed alone, said as `label`: a player's control.
pub fn glyph_button<M>(name: impl Into<Val<String>>, label: impl Into<Val<String>>, style: TextStyle, ink: Ink, tap: M) -> View<M> {
    pressable(label, tap, glyph(name).style(style).ink(ink).padding(Edges::All, Space::S)).into()
}

/// A player's controls (ui/docs/primitives.md, "transport controls"):
/// back, play or pause (large: `play_glyph` and `play_label` say which),
/// forward, spaced across the width; with [`Transport::shuffle`] and
/// [`Transport::repeat`] at its ends, each in its tint (`accent` while on).
pub struct Transport<M> {
    pub play_glyph: Val<String>,
    pub play_label: Val<String>,
    pub toggle: M,
    pub back: M,
    pub forward: M,
    pub shuffle: Option<(Val<String>, M)>,
    pub repeat: Option<(Val<String>, Val<String>, M)>,
}

/// A player's back, play or pause and forward.
pub fn transport<M>(play_glyph: impl Into<Val<String>>, play_label: impl Into<Val<String>>, toggle: M, back: M, forward: M) -> Transport<M> {
    Transport {
        play_glyph: play_glyph.into(),
        play_label: play_label.into(),
        toggle,
        back,
        forward,
        shuffle: None,
        repeat: None,
    }
}

impl<M> Transport<M> {
    /// Shuffle at its start, in `tint`.
    pub fn shuffle(mut self, tint: impl Into<Val<String>>, tap: M) -> Self {
        self.shuffle = Some((tint.into(), tap));
        self
    }
    /// Repeat at its end: its glyph (`repeat`, `repeat-1`) in `tint`.
    pub fn repeat(mut self, glyph: impl Into<Val<String>>, tint: impl Into<Val<String>>, tap: M) -> Self {
        self.repeat = Some((glyph.into(), tint.into(), tap));
        self
    }
}

impl<M> From<Transport<M>> for View<M> {
    fn from(t: Transport<M>) -> View<M> {
        let mut row: Vec<View<M>> = vec![];
        if let Some((tint, tap)) = t.shuffle {
            row.push(glyph_button("shuffle", "Shuffle", TextStyle::Body, Ink::Tint(tint), tap));
            row.push(spacer().into());
        }
        row.push(glyph_button("skip-back", "Previous", TextStyle::Title, Ink::Ink, t.back));
        row.push(spacer().into());
        row.push(glyph_button(t.play_glyph, t.play_label, TextStyle::LargeTitle, Ink::Ink, t.toggle));
        row.push(spacer().into());
        row.push(glyph_button("skip-forward", "Next", TextStyle::Title, Ink::Ink, t.forward));
        if let Some((g, tint, tap)) = t.repeat {
            row.push(spacer().into());
            row.push(glyph_button(g, "Repeat", TextStyle::Body, Ink::Tint(tint), tap));
        }
        hstack(row).padding(Edges::Horizontal, Space::L).fill_width().into()
    }
}

/// A quiet label over its figure (ui/docs/primitives.md, "figure with
/// label"): diagnostics' numbers, a record's small print, a summary's facts.
pub fn figure_label<M>(label: impl Into<Val<String>>, figure: impl Into<Val<String>>) -> View<M> {
    vstack((
        text(label).style(TextStyle::Caption).ink(Ink::Quiet),
        text(figure).style(TextStyle::Headline).lines(1),
    ))
    .spacing(Space::Xs)
    .align(Align::Start)
    .combine()
    .into()
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Assign,
        Open,
        Toggle,
        Back,
        Forward,
        Next,
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

    #[test]
    fn a_media_row_is_a_lit_row_of_its_cover_words_and_end_with_its_menu() {
        let v: View<Msg> = media_row("Aria", Msg::Open)
            .art(artwork("http://x/cover", "Aria", "Aria"))
            .under("Glenn Gould")
            .trailing(text("3:05"))
            .menu("Play Next", "list-start", Msg::Next)
            .into();
        let View::Part(Part::Press {
            press,
            label,
            tap,
            menu,
            content,
        }) = &v
        else {
            panic!("a media row is pressed as one: {v:?}")
        };
        assert_eq!((*press, label.get(), tap), (Press::Highlight, "Aria".to_string(), &Msg::Open));
        assert_eq!(menu.len(), 1);
        let View::Layout(face) = &**content else { panic!() };
        let Prim::Stack { content, .. } = &face.prim else { panic!() };
        let View::List(face) = &**content else { panic!() };
        assert!(matches!(&face[0], View::Part(Part::Artwork { size: ArtSize::Row, .. })));
        let mut said = vec![];
        words(&v, &mut said);
        let texts: Vec<_> = said.iter().map(|(t, _, s)| (t.as_str(), *s)).collect();
        assert_eq!(
            texts,
            [
                ("Aria", TextStyle::Body),
                ("Glenn Gould", TextStyle::Subheadline),
                ("3:05", TextStyle::Body)
            ]
        );
    }

    #[test]
    fn an_artwork_tile_gives_and_keeps_its_words_to_its_cover() {
        let v: View<Msg> = artwork_tile(
            artwork("", "Abbey Road", "Abbey Road").size(ArtSize::Shelf),
            "Abbey Road",
            "The Beatles",
            Msg::Open,
        );
        let View::Part(Part::Press { press, content, .. }) = &v else { panic!() };
        assert_eq!(*press, Press::Give);
        let View::Layout(face) = &**content else { panic!() };
        assert!(face.mods.contains(&Modifier::ArtWidth(ArtSize::Shelf)));
    }

    #[test]
    fn transport_is_back_play_forward_each_said() {
        let v: View<Msg> = transport("pause", "Pause", Msg::Toggle, Msg::Back, Msg::Forward).into();
        let mut presses = vec![];
        fn walk(v: &View<Msg>, out: &mut Vec<(String, Msg)>) {
            match v {
                View::List(vs) => vs.iter().for_each(|v| walk(v, out)),
                View::Layout(l) => l.slots().into_iter().for_each(|s| walk(s, out)),
                View::Part(Part::Press { label, tap, .. }) => out.push((label.get(), tap.clone())),
                _ => {}
            }
        }
        walk(&v, &mut presses);
        assert_eq!(
            presses,
            [
                ("Previous".into(), Msg::Back),
                ("Pause".into(), Msg::Toggle),
                ("Next".into(), Msg::Forward)
            ]
        );
    }

    #[test]
    fn a_pressed_row_is_named_by_its_label_and_a_tile_by_what_it_sends() {
        let row: Part<Msg> = Part::Press {
            press: Press::Highlight,
            label: "Dining Out".into(),
            tap: Msg::Open,
            menu: vec![],
            content: Box::new(View::Empty),
        };
        let tile: Part<Msg> = Part::Press {
            press: Press::Give,
            label: "Play".into(),
            tap: Msg::Toggle,
            menu: vec![],
            content: Box::new(View::Empty),
        };
        let id = |p: &Part<Msg>| match crate::shots::recorded("home", p).unwrap().as_slice() {
            [crate::shots::Piece::Text(t)] => t.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(id(&row), "home.row:dining-out");
        assert_eq!(id(&tile), "home.toggle");
    }
}
