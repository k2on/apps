//! harken's own components (ui/docs/primitives.md, "What Harken Lays
//! Out"): a cover, a tile of one, a shelf and a grid of them, a row of
//! something playable, a record's header, Play and Shuffle, a player's
//! controls, a figure under its label. Each is written once, here, of
//! ArkUI's primitives — stacks, a spacer, words, a glyph, a filled picture
//! over its derived square, a square frame, a clip, a press, a lazy grid —
//! and the generator writes it out as SwiftUI's, Compose's, iced's and the
//! browser's own layout. Nothing of it is hand-written per toolkit, and
//! nothing of it is ArkUI's: only harken lays covers out.

use ark_app::view::*;

/// How large a cover is drawn: one of `ui/theme.toml`'s `[layout]`
/// `art-*` measures, or as wide as it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtSize {
    /// `art-row`: beside a row's words.
    Row,
    /// `art-shelf`: a tile on a shelf.
    Shelf,
    /// `art-header`: beside a page's title.
    Header,
    /// As wide as it is given, square: a grid's tile, the player's cover.
    Fill,
}

impl ArtSize {
    /// Its side; none when it fills.
    pub fn measure(self) -> Option<Measure> {
        match self {
            ArtSize::Row => Some(Measure::ArtRow),
            ArtSize::Shelf => Some(Measure::ArtShelf),
            ArtSize::Header => Some(Measure::ArtHeader),
            ArtSize::Fill => None,
        }
    }
}

/// A cover being built ([`artwork`]).
#[derive(Clone, Debug)]
pub struct Artwork {
    pub url: Val<String>,
    pub seed: Val<String>,
    pub label: Val<String>,
    pub size: ArtSize,
    pub round: bool,
}

/// A cover: the picture at `url` over the square derived from `seed` (shown
/// while the picture comes, or when there is none), at [`ArtSize::Row`]
/// until [`Artwork::size`] says otherwise, `label` said for it.
pub fn artwork(url: impl Into<Val<String>>, seed: impl Into<Val<String>>, label: impl Into<Val<String>>) -> Artwork {
    Artwork {
        url: url.into(),
        seed: seed.into(),
        label: label.into(),
        size: ArtSize::Row,
        round: false,
    }
}

impl Artwork {
    pub fn size(mut self, s: ArtSize) -> Self {
        self.size = s;
        self
    }
    /// Round: a person.
    pub fn round(mut self) -> Self {
        self.round = true;
        self
    }
}

/// The square first — its side a measure, or as tall as it is wide — and
/// the picture laid into it, covering it and cut to it, over the derived
/// square; then clipped to the theme's `corner`, or round.
impl<M> From<Artwork> for View<M> {
    fn from(a: Artwork) -> View<M> {
        let picture = image(a.url, a.label).fill();
        let picture = match a.size.measure() {
            Some(m) => picture.decode(m),
            None => picture,
        };
        let art = zstack((swatch(a.seed), picture));
        let art = match a.size.measure() {
            Some(m) => art.side(m),
            None => art.square(),
        };
        art.corner(if a.round { Corner::Circle } else { Corner::Small }).into()
    }
}

/// A cover over its title and a quiet caption, pressed as one: a shelf's
/// or a grid's tile. Its words are kept to its cover's width, one line
/// each; a person's cover is round.
pub fn artwork_tile<M>(art: Artwork, title: impl Into<Val<String>>, caption: impl Into<Val<String>>, tap: M) -> View<M> {
    let title = title.into();
    let size = art.size;
    let face = vstack((
        View::from(art),
        text(title.clone()).style(TextStyle::Subheadline).lines(1),
        text(caption).style(TextStyle::Caption).ink(Ink::Quiet).lines(1).if_any(),
    ))
    .spacing(Space::Xs)
    .align(Align::Start);
    let face = match size.measure() {
        Some(m) => face.width(m),
        None => face.fill_width_at(Align::Start),
    };
    pressable(title, tap, face).into()
}

/// Tiles in a row that scrolls sideways, at the page's margins: a shelf
/// (Home's shelves, search's albums).
pub fn artwork_shelf<M>(tiles: impl Into<View<M>>) -> View<M> {
    scroll_across(
        hstack(tiles)
            .spacing(Space::M)
            .align(Align::Start)
            .padding(Edges::Horizontal, Space::Page),
    )
    .into()
}

/// Tiles in as many columns as fit at least `art-shelf` wide, at the
/// page's margins: Library's albums, people and shows.
pub fn artwork_grid<M>(tiles: impl Into<View<M>>) -> View<M> {
    grid(Measure::ArtShelf, tiles)
        .spacing(Space::M)
        .line_spacing(Space::L)
        .padding(Edges::Horizontal, Space::Page)
        .into()
}

/// A row of something playable or shown with its cover: the artwork, its
/// title over the line under it, what goes at its end (a length, a glyph
/// button), lit as a row as it is pressed and with a menu of what can be
/// done with it.
pub struct MediaRow<M> {
    pub art: Option<View<M>>,
    pub title: Val<String>,
    pub under: Option<Val<String>>,
    pub trailing: View<M>,
    pub tap: M,
    pub menu: Vec<(Val<String>, &'static str, M)>,
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
        self.menu.push((title.into(), glyph, tap));
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
        let mut row = press_row(r.title, r.tap, face);
        for (title, glyph, tap) in r.menu {
            row = row.menu(title, glyph, tap);
        }
        row.into()
    }
}

/// A record's header: its cover at `art-header` beside its titles, then
/// what it offers (`actions`: a [`play_shuffle`] pair) under both.
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

/// Play and Shuffle side by side, Play prominent.
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

/// A player's controls: back, play or pause (large: `play_glyph` and
/// `play_label` say which), forward, spaced across the width; with
/// [`Transport::shuffle`] and [`Transport::repeat`] at its ends, each in
/// its tint (`accent` while on).
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

/// A quiet label over its figure: diagnostics' numbers.
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
