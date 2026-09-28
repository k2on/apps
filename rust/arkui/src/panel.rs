//! The one shape every context window is: a menu, a submenu, a picker.
//!
//! **One component, down to the corner.** [`panel`] is the ground, the border,
//! the radius and the padding; [`entry`] is a row inside one; and
//! [`crate::style::entry`] and [`crate::style::entry_text`] are what it is
//! painted. They were three copies once, and the copies drifted: one corner
//! was 6 and another 8, small enough that nobody would call it a bug and plain
//! enough to see when the two are open beside each other — the submenu read as
//! a different *kind* of thing from the menu it hangs off, which is the one
//! thing a submenu must not do.
//!
//! **Declared, not measured.** A panel is *placed* before iced lays it out —
//! [`crate::menu::Menu::origin_for`], [`crate::menu::Menu::submenu_origin`] and
//! [`hang_above`] all do their arithmetic in `view`'s past — so every height
//! here is a constant given to the container, and the arithmetic is true by
//! construction rather than a guess about what 13pt text inside some padding
//! comes to.
use iced::widget::{container, text, Row};
use iced::{Alignment, Background, Border, Element, Length, Padding, Point};

use crate::icon;
use crate::theme::of;

/// The corner of every panel.
pub const RADIUS: f32 = 6.0;
pub const BORDER: f32 = 1.0;
/// The gap between a panel's border and the rows inside it, on all four sides.
///
/// It is what *contains* the highlight: a lit row is a rounded rectangle
/// floating inside the panel rather than a stripe painted onto its edge, which
/// is what a menu looks like everywhere and what the corner is for. It is also
/// the number [`crate::menu::SUBMENU_OVERLAP`] is, and not by coincidence.
pub const PADDING: f32 = 4.0;
/// The corner of a lit row inside a panel.
pub const ENTRY_RADIUS: f32 = 4.0;
/// How tall one row in a panel is. See the module doc: declared, so every
/// row of every panel is the same height, and a submenu's rows stay level
/// with the menu's from the entry it hangs off down.
pub const ENTRY: f32 = 27.0;
/// A panel's title line, given exactly this height for the same reason.
pub const TITLE: f32 = 24.0;
/// How far in a row's glyph starts.
pub const ENTRY_PAD_X: f32 = 10.0;
/// The gap after the glyph column.
pub const ENTRY_GAP: f32 = 8.0;
/// Roughly how wide one character is at size 13, in pixels.
///
/// The same kind of estimate [`crate::fit::PER_PORTION`] is, and unavailable
/// for the same reason — but it errs the other way, because the cost is the
/// other way: a panel sized from this is its own width, so erring wide is a
/// strip of empty panel and erring narrow is an ellipsis through somebody's
/// name. A proportional face's advance at 13px averages about 6.5 across mixed
/// case; 7 is a shade over, on purpose.
pub const ENTRY_CHAR: f32 = 7.0;
/// Everything in a panel row that is not the label: the panel's padding either
/// side, the row's, the glyph column and the gap after it.
pub const ENTRY_CHROME: f32 = PADDING * 2.0 + ENTRY_PAD_X * 2.0 + icon::SIZE + ENTRY_GAP;
/// …and the same for a title line, which has no glyph column.
pub const TITLE_CHROME: f32 = PADDING * 2.0 + ENTRY_PAD_X * 2.0;
/// A margin, so a panel that only just fits does not sit flush on the glass.
pub const EDGE: f32 = 8.0;

/// How many characters of a label a panel `width` wide affords, given what
/// else is on the row.
pub fn chars(width: f32, chrome: f32) -> usize {
    ((width - chrome) / ENTRY_CHAR).floor().max(0.0) as usize
}

/// How wide a panel has to be for its longest label, within a range: as wide
/// as its widest item and no wider, which is AppKit's rule for a menu. `extra`
/// is anything a label's row carries beyond the glyph column — a chevron.
pub fn width_for<'a>(labels: impl IntoIterator<Item = (&'a str, f32)>, min: f32, max: f32) -> f32 {
    let longest = labels
        .into_iter()
        .map(|(label, extra)| label.chars().count() as f32 * ENTRY_CHAR + extra)
        .fold(0.0_f32, f32::max);
    (ENTRY_CHROME + longest).clamp(min, max)
}

/// The height of a panel with a title line and `rows` entries.
pub fn titled_height(rows: usize) -> f32 {
    PADDING * 2.0 + TITLE + ENTRY * rows as f32
}

/// The chrome every context window shares: the ground, the border, the corner
/// and the padding.
///
/// Returns the container rather than an `Element` so a caller can still say
/// what is its own — a picker's height cap is a fact about thirty rows, not
/// about being a panel.
pub fn panel<'a, M: 'a>(body: impl Into<Element<'a, M>>, width: f32) -> container::Container<'a, M> {
    container(body).width(Length::Fixed(width)).padding(PADDING).style(|theme| {
        let palette = of(theme);
        container::Style {
            background: Some(Background::Color(palette.background.weak.color)),
            border: Border {
                color: palette.background.strong.color,
                width: BORDER,
                radius: RADIUS.into(),
            },
            ..container::Style::default()
        }
    })
}

/// One row inside a panel: a glyph in a column of its own, a label, and
/// whatever hangs off the right.
///
/// **The glyph column is there whether or not there is a glyph, and the row is
/// a fixed height rather than however tall its contents came out.** One rule
/// twice: what a row *is* must not be decided by what happens to be in it. A
/// name with no tick put itself where a ticked one's icon was, so a panel of
/// three was three indents; and a row whose glyph was absent came out shorter
/// than its neighbours, because an empty container has no height.
///
/// Returns the container so a caller can say the one thing that is its own:
/// the fill, [`crate::style::entry`].
pub fn entry<'a, M: 'a>(glyph: Option<Element<'a, M>>, label: Element<'a, M>, trailing: Option<Element<'a, M>>) -> container::Container<'a, M> {
    let mut line = Row::new().spacing(ENTRY_GAP).align_y(Alignment::Center).push(
        container(glyph.unwrap_or_else(|| text("").into()))
            .width(Length::Fixed(icon::SIZE))
            .height(Length::Fixed(icon::SIZE)),
    );
    line = line.push(label);
    if let Some(end) = trailing {
        line = line.push(end);
    }
    container(line)
        .width(Length::Fill)
        .height(Length::Fixed(ENTRY))
        .padding(Padding::from([0.0, ENTRY_PAD_X]))
        .align_y(Alignment::Center)
}

/// A panel's title line, at exactly [`TITLE`]: which thing the panel is about,
/// quietly. A menu opened by a right click can land a row away from where the
/// eye was, and a menu that does not say what it is for is one you close to
/// check.
pub fn title<'a, M: 'a>(label: String) -> container::Container<'a, M> {
    container(text(label).size(11).style(crate::style::dim))
        .height(Length::Fixed(TITLE))
        .align_y(Alignment::Center)
        .padding(Padding::from([0.0, ENTRY_PAD_X]))
}

/// A label in a panel row, in the one colour legible on it.
pub fn label<'a, M: 'a>(body: String, lit: bool) -> Element<'a, M> {
    text(body)
        .size(13)
        .width(Length::Fill)
        .wrapping(text::Wrapping::None)
        .style(move |theme| text::Style {
            color: Some(crate::style::entry_text(theme, lit)),
        })
        .into()
}

/// Where a panel `width` by `height` goes when it hangs *above* something that
/// ends at `right` and starts at `bottom` — a picker opened from a button in a
/// bar at the foot of the window.
///
/// **A control's menu opens at the control.** A panel that turns up wherever
/// the pointer happened to drift reads as detached from the button that opened
/// it, and one placed from the live pointer inside `view` slides about under
/// the hand that opened it. This takes no pointer, which is the stronger of
/// the two guarantees. It stays on the glass: `pin` clips rather than scrolls,
/// so a panel placed off an edge simply loses those rows.
pub fn hang_above(right: f32, bottom: f32, width: f32, height: f32) -> Point {
    Point::new((right - width).max(EDGE), (bottom - height).max(EDGE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picker hung off a button hangs off the button, and nothing about
    /// where the pointer is can move it.
    ///
    /// **Said twice, the second time as literals**: written only against the
    /// constants, both sides of a comparison move together and the assertion
    /// holds whatever they are set to. 628 and 423 are harken's device picker
    /// at 860×600 with two devices and a stop row, worked out by hand —
    /// 860 − 16 − 216, and 600 − 16 − 48 − (8 + 24 + 27 × 3). Falsified by
    /// changing `TITLE` to 27: the literals fail and the first two do not.
    #[test]
    fn a_panel_hangs_off_its_button() {
        let (width, bar_top, right) = (216.0, 600.0 - 16.0 - 48.0, 860.0 - 16.0);
        let at = hang_above(right, bar_top, width, titled_height(3));
        assert_eq!(at.x + width, right, "the panel does not end where the button does");
        assert_eq!(at.y + titled_height(3), bar_top, "the panel does not sit on the bar");
        assert_eq!((at.x, at.y), (628.0, 423.0));
    }

    /// …and it stays on the glass on a window too small to hold it: thirty
    /// rows is a house full of speakers, well past what 600px can show.
    /// Falsified by dropping either `.max` in `hang_above`.
    #[test]
    fn a_panel_never_hangs_off_the_glass() {
        for rows in 1..=30 {
            for window in [120.0, 240.0, 860.0, 4000.0] {
                let at = hang_above(window - 16.0, 600.0 - 16.0 - 48.0, 216.0, titled_height(rows));
                assert!(at.x >= EDGE && at.y >= EDGE, "{rows} rows on a {window}px window put the panel at {at:?}");
            }
        }
    }
}
