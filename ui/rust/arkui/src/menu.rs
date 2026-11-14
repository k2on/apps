//! A menu: a title, entries, and where on the window it goes.
//!
//! Every entry is something the window could already do. What a menu adds is
//! asking for it *about something* — a row you are pointing at — which neither
//! a key nor a click on the row itself could say. How a menu, its submenu and
//! a picker behave together — which one has the keyboard, the dwell, `h`, `l`
//! and `<Esc>` — is [`crate::context`]; this file is the menu itself and its
//! geometry.
use iced::widget::{column, mouse_area};
use iced::{Element, Point, Size};

use crate::fit::{middle, tail};
use crate::panel::{self, ENTRY, ENTRY_GAP, PADDING, TITLE, TITLE_CHROME};
use crate::{icon, style, vim};

/// The narrowest a menu is. **A menu is as wide as its longest entry**, within
/// [`MIN_WIDTH`]..=[`MAX_WIDTH`], which is AppKit's rule: an `NSMenu` sizes
/// itself to its widest item and only truncates when it runs out of screen.
/// harken's was a flat 198, so `Go to Goldberg Variations, BWV 988` had to be
/// cut to fit it — through the middle, spending the budget on `Go to` and an
/// ellipsis — and then *wrapped* inside a row whose height is fixed, so the
/// second line drew over its neighbour. The minimum is that old width, so
/// nothing narrows; the maximum is what stops one long title making a menu
/// the width of the window.
pub const MIN_WIDTH: f32 = 198.0;
pub const MAX_WIDTH: f32 = 420.0;

/// How far a submenu laps over the menu it hangs off: **exactly the parent's
/// own padding, so the lit row inside it touches the submenu's edge.**
///
/// That is the whole rule, and the one thing to look at on screen. A menu's
/// highlight is inset by [`PADDING`], so its right-hand edge is the menu's
/// width less the padding — and a submenu placed there meets it. The panels
/// overlap, because a submenu that merely abuts its parent reads as a second
/// panel; what they overlap is the strip of empty panel beside the highlight.
///
/// It was `PADDING * 2` for a while: one panel's worth too far, so the
/// submenu's border landed four pixels inside the parent's lit row and clipped
/// the corner off it. The tell is exactly that — the highlight ending *under*
/// the submenu instead of at it.
pub const SUBMENU_OVERLAP: f32 = PADDING;

/// A submenu is the panel's padding and then its rows, and nothing else: it
/// draws no header, because the menu it hangs off is still up with the same
/// title across its own top. [`crate::picker::view`] draws exactly this.
///
/// **This number and that view have to agree, and nothing checks it.** iced
/// lays out after `view` and [`Menu::submenu_origin`] decides before it, so a
/// header put back without moving this would place a panel shorter than the
/// one drawn — and `pin` clips, so the bottom rows would simply not be there.
pub const SUBMENU_CHROME: f32 = PADDING * 2.0;

/// How long the cursor rests on an entry before its submenu opens, in ticks of
/// a 50ms clock — so 200ms, about what AppKit waits.
///
/// Not zero, and that is the whole reason there is a number. A submenu entry
/// that sits between two others is crossed by every pointer on its way past,
/// and opening on the way past is a panel flashing up under the cursor on a
/// move that was never about it. A beat says "you stopped here" and a sweep
/// does not.
pub const DWELL: u8 = 4;

/// Where a menu opens, which is decided by what asked for it.
///
/// macOS does both, and they are two rules rather than a preference. A
/// contextual menu — right click, Control-click — opens **at the pointer**,
/// with its corner on the click. A menu belonging to a *control*, like the ⋯
/// at the end of a row, opens **at the control**, because the control is a
/// fixed thing on screen and a menu that ignored it would look detached from
/// the button you pressed. Getting either backwards looks like a bug.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor {
    /// A right click: the corner goes on the pointer.
    Pointer,
    /// A control whose right-hand edge is at this x: the menu's right edge
    /// lines up with it, wherever along the row the pointer was. A key that
    /// opens the same menu takes this too — by key there is no pointer.
    RightEdge(f32),
}

/// What an entry does when it is run.
///
/// One field rather than a message and a `bool` beside it: an entry owns the
/// submenu exactly when running it opens one, and a second answer kept beside
/// the first would be one to keep in step with it — the chevron, the dwell and
/// `l` all ask this.
#[derive(Debug, Clone)]
pub enum Act<M> {
    Run(M),
    /// Opens the submenu; `M` is what the app does to open it.
    Submenu(M),
}

/// One thing a menu offers: a drawing, a name, and what it does.
#[derive(Debug, Clone)]
pub struct Entry<M> {
    pub glyph: Option<&'static [u8]>,
    pub label: String,
    pub act: Act<M>,
}

impl<M> Entry<M> {
    pub fn run(glyph: &'static [u8], label: impl Into<String>, message: M) -> Self {
        Entry {
            glyph: Some(glyph),
            label: label.into(),
            act: Act::Run(message),
        }
    }

    pub fn submenu(glyph: &'static [u8], label: impl Into<String>, message: M) -> Self {
        Entry {
            glyph: Some(glyph),
            label: label.into(),
            act: Act::Submenu(message),
        }
    }

    pub fn message(&self) -> &M {
        match &self.act {
            Act::Run(m) | Act::Submenu(m) => m,
        }
    }

    pub fn opens_submenu(&self) -> bool {
        matches!(self.act, Act::Submenu(_))
    }
}

/// A menu that is up.
///
/// Its entries are built once, when it opens, and are the one definition of
/// what is in it: the view draws them and `<Enter>` runs them, so the two
/// cannot come to disagree about what the third entry is.
#[derive(Debug, Clone)]
pub struct Menu<M> {
    /// What it is about, drawn across its top.
    pub title: String,
    pub entries: Vec<Entry<M>>,
    /// Where it is pinned, in the window. Stored rather than recomputed from
    /// the pointer, so it stays where it was asked for.
    pub origin: Point,
    /// How wide it came out, from its own longest entry. Stored because three
    /// things need it and must agree — the view draws the panel, the origin
    /// places it, and the submenu hangs off its right-hand edge.
    pub width: f32,
    /// Which entry the cursor is on. A menu only the pointer can reach is
    /// missing from half a keyboard window's controls.
    pub at: usize,
    /// Ticks the cursor has rested on `at`.
    pub(crate) dwell: u8,
    /// Whether the submenu `at` owns has already been offered this rest.
    ///
    /// `<Esc>` out of a submenu leaves the cursor on the entry it came from,
    /// and without this the very next tick would open it again — an overlay
    /// you cannot close. It clears when the cursor moves, so leaving the entry
    /// and coming back offers it a second time, which is what somebody who
    /// closed it by accident will do.
    pub(crate) offered: bool,
}

impl<M> Menu<M> {
    /// A menu about `title`, asked for with the pointer at `cursor`: sized from
    /// its entries, then placed — in that order, because how many entries it
    /// has decides how tall it is and their longest decides how wide, and both
    /// decide which way it has room to open.
    pub fn open(title: impl Into<String>, entries: Vec<Entry<M>>, cursor: Point, window: Size, anchor: Anchor) -> Self {
        let width = Self::width_for(&entries);
        let origin = Self::origin_for(cursor, window, entries.len(), anchor, width);
        Menu {
            title: title.into(),
            entries,
            origin,
            width,
            at: 0,
            dwell: 0,
            offered: false,
        }
    }

    /// As wide as its longest entry, within [`MIN_WIDTH`]..=[`MAX_WIDTH`].
    ///
    /// Counted against [`panel::ENTRY_CHAR`], which errs wide. The entry that
    /// owns a submenu carries a chevron at the end of its row, so it needs that
    /// column too.
    pub fn width_for(entries: &[Entry<M>]) -> f32 {
        panel::width_for(
            entries.iter().map(|e| {
                let extra = match e.opens_submenu() {
                    true => icon::SIZE + ENTRY_GAP,
                    false => 0.0,
                };
                (e.label.as_str(), extra)
            }),
            MIN_WIDTH,
            MAX_WIDTH,
        )
    }

    /// How many characters of a label its width affords.
    pub fn label_chars(&self) -> usize {
        panel::chars(self.width, panel::ENTRY_CHROME)
    }

    /// …and of the title, which sits in a line with no glyph column.
    pub fn title_chars(&self) -> usize {
        panel::chars(self.width, TITLE_CHROME)
    }

    /// The entry the cursor is on.
    pub fn current(&self) -> Option<&Entry<M>> {
        self.entries.get(self.at)
    }

    /// Its shape, for the keyboard: one column, so `h` and `l` are refused and
    /// the context gets to say what is on the other side of that edge.
    pub fn grid(&self) -> vim::Grid {
        vim::Grid::column(self.entries.len())
    }

    /// How tall a menu of `entries` comes out: the panel's padding, its title
    /// line, and a row per entry — the panel's own numbers.
    pub fn height(entries: usize) -> f32 {
        panel::titled_height(entries)
    }

    /// Where to pin a menu asked for at `cursor`.
    ///
    /// **How far down is always the pointer's row; how far across is the
    /// [`Anchor`]'s.** And it opens away from whichever edge it is against:
    /// `pin` clips rather than scrolls, so a menu asked for near the bottom of
    /// the window would otherwise simply not have its last entries.
    pub fn origin_for(cursor: Point, window: Size, entries: usize, anchor: Anchor, width: f32) -> Point {
        let x = match anchor {
            Anchor::Pointer => cursor.x,
            Anchor::RightEdge(right) => right_aligned(right, width),
        };
        fit(Point::new(x, cursor.y), window, width, Self::height(entries))
    }

    /// Where the cursor's entry starts, down the window.
    pub fn entry_top(&self) -> f32 {
        self.origin.y + PADDING + TITLE + ENTRY * self.at as f32
    }

    /// Where a submenu of `rows` rows and `width` goes: beside the entry it
    /// hangs off.
    ///
    /// **It slides to fit; it does not flip.** [`fit`] is right for a menu,
    /// which hangs off a *point*: with no room below, opening upward from that
    /// point is still a menu about that point. A submenu hangs off a *row*, and
    /// flipping puts it somewhere with nothing to do with the row. harken's
    /// once passed `fit` the panel's *maximum* height, so on any window under
    /// about 520px the flip fired whatever the submenu's real height was and a
    /// two-row panel jumped above the menu — and flipped across, a wide
    /// submenu against a menu at the right-hand edge landed almost entirely on
    /// top of its parent.
    ///
    /// So: to the right of the parent when there is room and to its left when
    /// there is not, never over it; and level with the entry, slid up only as
    /// far as staying on the glass takes. "Level" means the *rows* are level:
    /// both panels inset their rows by [`PADDING`], so the panel starts a
    /// padding above the entry, and from there down every row of both is
    /// [`ENTRY`] tall.
    pub fn submenu_origin(&self, rows: usize, width: f32, max_height: f32, window: Size) -> Point {
        let right = self.origin.x + self.width - SUBMENU_OVERLAP;
        let x = match right + width + panel::EDGE > window.width {
            true => (self.origin.x - width + SUBMENU_OVERLAP).max(panel::EDGE),
            false => right,
        };
        let height = submenu_height(rows, max_height);
        let y = (self.entry_top() - PADDING)
            .min((window.height - height - panel::EDGE).max(panel::EDGE))
            .max(panel::EDGE);
        Point::new(x, y)
    }
}

/// How tall a submenu of `rows` comes out, capped where the panel caps itself,
/// so thirty rows scroll inside a panel of a known size rather than growing one
/// nothing can place.
pub fn submenu_height(rows: usize, max_height: f32) -> f32 {
    (SUBMENU_CHROME + ENTRY * rows as f32).min(max_height)
}

/// Put a panel of `w` by `h` at `at`, or back the other way on either axis
/// when it would not fit. The rule a menu follows; a submenu does not.
pub fn fit(at: Point, window: Size, w: f32, h: f32) -> Point {
    let edge = panel::EDGE;
    let x = match at.x + w + edge > window.width {
        true => (at.x - w).max(edge),
        false => at.x,
    };
    let y = match at.y + h + edge > window.height {
        true => (at.y - h).max(edge),
        false => at.y,
    };
    Point::new(x, y)
}

/// The x a panel `width` wide starts at when its right edge is at `right` —
/// on the glass, whatever the window: `pin` clips, so a menu placed off the
/// left edge simply loses that half.
pub fn right_aligned(right: f32, width: f32) -> f32 {
    (right - width).max(panel::EDGE)
}

/// The menu, drawn: a title, then an entry per row, each landing the cursor
/// when pointed at and running when released.
///
/// `focused` is false while its submenu has the keyboard, and then the entry
/// under the cursor dims rather than filling with the accent — the way a
/// native menu leaves the parent entry marked rather than lit. Two accent rows
/// would be two answers to "where does the next key go".
///
/// Pointing at an entry and clicking one are the same two messages a key is:
/// `at(i)` lands the cursor where the keyboard would have walked it, and
/// `activate` runs whatever it is on — so whichever you used last, the other
/// carries on from there.
pub fn view<'a, M: Clone + 'a>(menu: &'a Menu<M>, focused: bool, at: impl Fn(usize) -> M, activate: M) -> Element<'a, M> {
    let budget = menu.label_chars();
    let rows = menu.entries.iter().enumerate().fold(column![].spacing(0), |col, (i, entry)| {
        let on_cursor = menu.at == i;
        let lit = on_cursor && focused;
        let end = entry.opens_submenu().then(|| Element::from(icon::chevron(lit)));
        let line = panel::entry(
            entry.glyph.map(|g| Element::from(icon::line(g, lit))),
            panel::label(tail(&entry.label, budget), lit),
            end,
        );
        col.push(
            mouse_area(line.style(move |theme| style::entry(theme, on_cursor, focused)))
                .on_enter(at(i))
                .on_press(at(i))
                .on_release(activate.clone()),
        )
    });
    panel::panel(
        column![panel::title(middle(&menu.title, menu.title_chars())), rows].spacing(0),
        menu.width,
    )
    .into()
}
