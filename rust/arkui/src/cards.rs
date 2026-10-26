//! A page of cards — a picture, a name, a line under it — and the header a page
//! about one thing opens with.
//!
//! **How many cards fit is arithmetic, not a measurement.** The keyboard has to
//! know where `l` lands before iced has laid anything out, so [`Shelf::columns`]
//! divides the same width by the same card that [`view`] draws from. A grid
//! drawn four across and walked as though it were five puts the cursor on a
//! card nobody can see, and it reads as the keymap skipping rows at random —
//! which is why the geometry is one value, [`Shelf`], used by both.
//!
//! **The cursor on a card is its title going accent**, not a filled rectangle.
//! A card is mostly picture, and a fill behind one is a border around an image
//! — it reads as a selected file, not as where the next `l` goes.
use iced::widget::{column, container, image, mouse_area, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

use crate::fit::Cut;
use crate::line::Line;
use crate::{art, style, vim, SCROLLBAR};

/// A card grid's geometry: the narrowest a card is drawn, the gap between
/// two, and the page's padding either side.
///
/// **A card is as wide as its row allows**, from `card` up: as many fit as
/// would at `card`, and then they grow to fill the row, the way a record
/// shop's shelf does — so a wider window is bigger covers, not a gutter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shelf {
    pub card: f32,
    pub gap: f32,
    pub padding: f32,
}

impl Shelf {
    /// harken's: cards of 168px and up, 20 apart, on a page padded by 24 —
    /// about 190px across a 1280px window, a catalogue's shelf.
    pub const DEFAULT: Shelf = Shelf {
        card: 168.0,
        gap: 20.0,
        padding: 24.0,
    };

    /// The width a row of cards actually gets inside a pane `pane` wide: less
    /// the page's padding either side — and less the *scrollbar*, which harken
    /// did not subtract. Ten pixels, and at the widths where N cards needed
    /// every one of them the row came out over-full; iced clamps a `Fixed`
    /// child to the space left, so the last card in each row was drawn
    /// narrower than the rest. One card in five at the wrong size reads as a
    /// rendering fault and is an off-by-ten.
    pub fn room(&self, pane: f32) -> f32 {
        (pane - self.padding * 2.0 - SCROLLBAR).max(0.0)
    }

    /// How many cards fit across a pane `pane` wide. At least one.
    pub fn columns(&self, pane: f32) -> usize {
        (((self.room(pane) + self.gap) / (self.card + self.gap)) as usize).max(1)
    }

    /// How wide a card is drawn in a pane `pane` wide: the row shared out
    /// between its columns, whole pixels, and never under `card`.
    pub fn side(&self, pane: f32) -> f32 {
        let n = self.columns(pane) as f32;
        ((self.room(pane) - self.gap * (n - 1.0)) / n).floor().max(self.card)
    }

    /// The keyboard's shape of `cells` cards in a pane `pane` wide.
    pub fn grid(&self, cells: usize, pane: f32) -> vim::Grid {
        vim::Grid {
            cells,
            columns: self.columns(pane),
        }
    }
}

/// One card.
#[derive(Debug, Clone)]
pub struct Card<M> {
    /// What the derived square is drawn from — the name, so the square this
    /// card has is the square it has everywhere else.
    pub seed: String,
    pub title: String,
    /// The line under the title.
    pub under: String,
    /// A person is a circle and a record is a square, which is the one thing
    /// every music app agrees about and the only thing telling two grids apart
    /// at a glance.
    pub round: bool,
    /// The picture, if one has arrived; the derived square otherwise.
    pub picture: Option<image::Handle>,
    /// What opening it does.
    pub open: M,
}

/// A page of cards across a pane `pane` wide, with the cursor on `cursor`.
///
/// Laid out as rows of [`Shelf::columns`] rather than by a wrapping widget,
/// because the keyboard has to agree about where `l` lands: pass the same
/// pane width the keyboard's grid was built from. `on_hover(i)` is
/// sent when the pointer reaches card `i` — one highlight, however you moved
/// it — `scroll` is the scrollable's id, for [`crate::scroll::reveal`], and
/// `on_scroll` hears where it was scrolled to.
#[allow(clippy::too_many_arguments)]
pub fn view<'a, M: Clone + 'a>(
    shelf: Shelf,
    cards: &[Card<M>],
    pane: f32,
    cursor: Option<usize>,
    empty: &'a str,
    on_hover: impl Fn(usize) -> M,
    scroll: &'static str,
    on_scroll: impl Fn(iced::widget::scrollable::Viewport) -> M + 'a,
) -> Element<'a, M> {
    if cards.is_empty() {
        return empty_page(empty, shelf.padding);
    }
    let columns = shelf.columns(pane);
    let side = shelf.side(pane);
    let mut page = column![].spacing(shelf.gap);
    for (r, chunk) in cards.chunks(columns).enumerate() {
        // Top-aligned: a title that wrapped to two lines makes its card taller,
        // and centring would then float the short ones.
        let mut line = row![].spacing(shelf.gap).align_y(Alignment::Start);
        for (c, card) in chunk.iter().enumerate() {
            let at = r * columns + c;
            line = line.push(view_card(side, card, Some(at) == cursor, on_hover(at)));
        }
        page = page.push(line);
    }
    // Clear of the scrollbar on the right, which `room` has already left.
    let padding = Padding {
        right: shelf.padding + SCROLLBAR,
        ..Padding::new(shelf.padding)
    };
    container(
        scrollable(container(page).padding(padding))
            .id(scroll)
            .on_scroll(on_scroll)
            .style(style::bars),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// One card `side` wide ([`Shelf::side`]): the picture, the name, and what
/// is under it.
pub fn view_card<'a, M: Clone + 'a>(side: f32, card: &Card<M>, on_cursor: bool, hover: M) -> Element<'a, M> {
    let corner = if card.round { side / 2.0 } else { 6.0 };
    mouse_area(
        column![
            art::picture(&card.seed, card.picture.as_ref(), side, corner),
            // Bounded, and allowed to wrap. Unbounded with `Wrapping::None` the
            // layout node is as wide as the text, so a long title ran straight
            // through the card beside it.
            text(crate::fit::drawable(&card.title).into_owned())
                .size(13)
                .width(Length::Fixed(side))
                .style(move |theme| text::Style {
                    color: Some(match on_cursor {
                        true => crate::theme::of(theme).primary.base.color,
                        false => crate::theme::of(theme).background.base.text,
                    }),
                }),
            text(crate::fit::drawable(&card.under).into_owned())
                .size(12)
                .width(Length::Fixed(side))
                .style(style::faint(0.5)),
        ]
        .spacing(4)
        .width(Length::Fixed(side)),
    )
    .on_enter(hover)
    .on_press(card.open.clone())
    .into()
}

/// What a page says when it has nothing to show.
pub fn empty_page<'a, M: 'a>(message: &'a str, padding: f32) -> Element<'a, M> {
    container(text(message).size(13).style(style::faint(0.5))).padding(padding).into()
}

/// The header a page *about* something opens with: the picture, what kind of
/// thing it is, its name, one line under it, and the numbers true of the whole
/// of it.
///
/// A record is not only a list of what is on it, and a page that opens on the
/// first row of a table says nothing about the record. `under` is the one
/// thing on such a page that is also a *place* (an album's artist), so it is
/// drawn in the accent and `open_under` is where a click on it goes.
///
/// **It is laid out inside the page, however narrow the page is** — beside a
/// docked panel, `Goldberg Variations, BWV 988` at 26px ran straight on into
/// the panel. The name wraps, because a title is the one thing on the page
/// drawn large enough to read on two lines, and cut at 26px it would lose
/// the half of itself nobody else on the page says. The numbers wrap too,
/// since every one of them is a fact and a licence is among them. `kind` and
/// `under` are cut ([`crate::line::Line`]) — at the end, because `under` is
/// a person's name, and that reads from its start.
pub fn header<'a, M: Clone + 'a>(
    picture: Element<'a, M>,
    kind: &'a str,
    name: String,
    under: String,
    open_under: Option<M>,
    facts: String,
    padding: f32,
) -> Element<'a, M> {
    container(
        row![
            picture,
            column![
                Line::new(kind, 10.0, Cut::End, faint),
                text(crate::fit::drawable(&name).into_owned())
                    .size(28)
                    .font(crate::hero::BOLD)
                    .wrapping(text::Wrapping::Word),
                match open_under {
                    Some(open) => Element::from(mouse_area(under_line(under)).on_press(open)),
                    None => under_line(under).into(),
                },
                text(crate::fit::drawable(&facts).into_owned())
                    .size(11)
                    .wrapping(text::Wrapping::Word)
                    .style(style::faint(0.5)),
            ]
            .spacing(4)
            .width(Length::Fill),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: padding,
        right: padding,
        bottom: padding,
        left: 0.0,
    })
    .width(Length::Fill)
    .into()
}

/// The faint grey a header's small print is drawn in.
fn faint(theme: &iced::Theme) -> iced::Color {
    style::faint(0.5)(theme).color.unwrap_or_default()
}

/// A header's `under`, in the accent, cut at its end.
fn under_line<'a>(under: String) -> Line<'a> {
    Line::new(under, 13.0, Cut::End, |theme| style::accent(theme).color.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What harken's pane was: the window less a 200px sidebar.
    const SIDEBAR: f32 = 200.0;

    /// Every width a window can be, and the row it produces has to fit.
    ///
    /// Falsified by removing `SCROLLBAR` from `room`: it fails at 964 and at
    /// every width where a row is exactly full.
    #[test]
    fn cards_never_overflow_their_row() {
        let shelf = Shelf::DEFAULT;
        for width in 320..=4000 {
            let pane = width as f32 - SIDEBAR;
            let n = shelf.columns(pane) as f32;
            let needed = n * shelf.card + (n - 1.0) * shelf.gap;
            let room = pane - shelf.padding * 2.0 - SCROLLBAR;
            assert!(
                n == 1.0 || needed <= room,
                "at {width}px the page draws {n} cards needing {needed}px in {room}px — \
                 the last one is clamped and comes out a different size from the rest"
            );
        }
    }

    /// …and not needlessly stingy either: whenever another card would fit, it
    /// is drawn. Without this half, "subtract more" passes the test above and
    /// wastes a column. Falsified by subtracting the scrollbar twice in `room`.
    #[test]
    fn a_card_that_fits_is_drawn() {
        let shelf = Shelf::DEFAULT;
        for width in 320..=4000 {
            let pane = width as f32 - SIDEBAR;
            let n = shelf.columns(pane) as f32;
            let one_more = (n + 1.0) * shelf.card + n * shelf.gap;
            let room = pane - shelf.padding * 2.0 - SCROLLBAR;
            assert!(one_more > room, "at {width}px there is room for {} cards and only {n} are drawn", n + 1.0);
        }
    }

    /// The cards grow to fill their row and no further: never under `card`,
    /// never past the room, and no more than a pixel a card short of it.
    #[test]
    fn the_cards_fill_their_row() {
        let shelf = Shelf::DEFAULT;
        for width in 320..=4000 {
            let pane = width as f32 - SIDEBAR;
            let (n, side) = (shelf.columns(pane) as f32, shelf.side(pane));
            let drawn = n * side + (n - 1.0) * shelf.gap;
            let room = shelf.room(pane);
            assert!(side >= shelf.card, "at {width}px a card is {side}px");
            if n > 1.0 {
                assert!(drawn <= room && room - drawn < n, "at {width}px {n} cards draw {drawn}px in {room}px");
            }
        }
    }
}
