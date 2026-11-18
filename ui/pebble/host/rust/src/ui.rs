//! Pebble's toolkit for arkui (ui/docs/pebble.md): what a screen
//! generated from a `#[view]` (ark-ui-gen's `pebble` emitter) is drawn
//! with. The emitter writes, at build time, a `build` that makes the
//! screen's layers once — each a native layer of PebbleOS's through the C
//! shim (`c/ark_pebble.c`): a `TextLayer`, a drawn meter, a `BitmapLayer`
//! of a glyph, a `MenuLayer` for a list, the `ActionBarLayer` — at frames
//! worked out from the window it is given and the theme's tokens
//! ([`crate::tokens`]), and its [`Bindings`]: a table, data, of what each
//! layer shows, a source function of the screen's over the node's rows and
//! cells. [`Ui`] makes a reader of the node's for each binding and is the
//! sink the node's settle tells ([`ark_kernel::Sink`]): a layer whose
//! reads changed is marked dirty, and PebbleOS draws it again, asking for
//! its words as it draws (`ark_pull`), so nothing it shows is held
//! anywhere but the rows and cells it is read from. A glyph, an action's
//! glyph and a list's count are read when their readers are told, and set.
//!
//! Glyphs are arkui's (Lucide's drawings), flattened into line segments
//! by the emitter ([`Glyph`]) and painted here into 1-bit bitmaps at the
//! size a style or the action bar asks, once each, so the app has no
//! resources. Nothing here moves: a first cut animates nothing.

use alloc::vec::Vec;

use ark_kernel::{Node, Read, Reader, Sink, Store};

use crate::sys;
use crate::text::Line;
use crate::tokens::{GLYPH_STROKE, HEADER_HEIGHT, ROW_HEIGHT, TEXT_LIFT};

/// One of Gothic's sizes, regular or bold (`tokens::FONT_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Font {
    pub size: u8,
    pub bold: bool,
}

/// Where a layer is, in the window's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}

/// Where words sit across their frame: leading, centred, trailing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start = 0,
    Center = 1,
    End = 2,
}

/// A button, as PebbleOS numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Back = 0,
    Up = 1,
    Select = 2,
    Down = 3,
}

impl Button {
    pub fn from(id: u8) -> Option<Button> {
        [Button::Back, Button::Up, Button::Select, Button::Down].get(usize::from(id)).copied()
    }
}

/// A glyph by its Lucide name, as the emitter flattened it: subpaths, each
/// its point count, whether it is closed (1) or open (0), then its points
/// as x, y in eighths of the drawing's 24 units.
pub struct Glyph {
    pub name: &'static [u8],
    pub path: &'static [u8],
}

/// How a list draws its rows and its heading.
#[derive(Clone, Copy, Debug)]
pub struct ListStyle {
    pub title: Font,
    pub title_line: i16,
    pub subtitle: Font,
    pub header: Font,
    pub ink: u8,
    pub quiet: u8,
    pub ground: u8,
    pub inset: i16,
}

/// A source's words, written into the line a layer is drawn from.
pub type Words = fn(&dyn Read, &mut Line);
/// A source's fraction, in thousandths.
pub type Fraction = fn(&dyn Read) -> u16;
/// A source's glyph, by its name (one of the screen's).
pub type Name = fn(&dyn Read) -> &'static [u8];
/// How many rows a list's source holds.
pub type Count = fn(&dyn Read) -> usize;
/// The words of a list's row `i`.
pub type RowWords = fn(&dyn Read, usize, &mut Line);
/// Whether an `if`'s first branch is the one shown.
pub type Flag = fn(&dyn Read) -> bool;

/// A list's bindings: its heading, its count and each row's two lines.
pub struct List {
    pub header: Words,
    pub count: Count,
    pub title: RowWords,
    pub line: RowWords,
}

/// What a screen's layers show, by their ids: the emitter writes it from
/// the view's paths, a table of the screen's source functions.
pub struct Bindings {
    pub words: &'static [(u8, Words)],
    pub meters: &'static [(u8, Fraction)],
    pub glyphs: &'static [(u8, Name)],
    pub actions: &'static [(Button, Name)],
    pub lists: &'static [(u8, List)],
    /// Each `if`: its source, and its branches' layers.
    pub shown: &'static [(Flag, &'static [u8], &'static [u8])],
}

/// What a reader is bound to.
#[derive(Clone, Copy)]
enum Bound {
    Words(u8, Words),
    Meter(u8, Fraction),
    Glyph(u8, Name),
    Action(Button, Name),
    List(u8, &'static List),
    Shown(Flag, &'static [u8], &'static [u8]),
}

/// A glyph painted: which, at what side, how, in what colour, and the
/// shim's bitmap.
struct Painted {
    glyph: u8,
    side: i16,
    solid: bool,
    ink: u8,
    bitmap: u8,
}

/// No glyph: what the shim clears a layer or a button to.
const NONE: u8 = u8::MAX;

/// A screen's layers, by the ids its generated `build` gave them.
pub struct Ui {
    glyphs: &'static [Glyph],
    /// Each glyph layer's side, colour and whether it is solid.
    glyph_style: Vec<(i16, u8, bool)>,
    painted: Vec<Painted>,
    actions: [u8; 4],
    /// The action bar's glyphs' side and colour, once there is one.
    bar: Option<(i16, u8)>,
    room: (i16, i16),
    /// Each binding's reader, in the order made, from the first's.
    bound: Vec<Bound>,
    first: u32,
    /// A reader for what a list's rows read as they are drawn: its count's
    /// reads say when they change.
    scratch: Option<Reader>,
    /// The readers told since they were last read.
    told: Vec<u32>,
}

impl Ui {
    /// A screen over the whole window, its ground `ground`, drawing
    /// `glyphs`.
    pub fn new(glyphs: &'static [Glyph], ground: u8) -> Ui {
        sys::ui_window(ground);
        Ui {
            glyphs,
            glyph_style: Vec::new(),
            painted: Vec::new(),
            actions: [NONE; 4],
            bar: None,
            room: sys::ui_bounds(),
            bound: Vec::new(),
            first: 0,
            scratch: None,
            told: Vec::new(),
        }
    }

    /// Binds the screen's layers to its sources, a reader of `node`'s
    /// each, all read once now.
    pub fn bind<B: ark_kernel::Body + Clone + 'static, S: Store>(&mut self, node: &mut Node<B, S>, b: &'static Bindings) {
        let mut bound: Vec<Bound> = Vec::new();
        bound.extend(b.words.iter().map(|&(id, f)| Bound::Words(id, f)));
        bound.extend(b.meters.iter().map(|&(id, f)| Bound::Meter(id, f)));
        bound.extend(b.glyphs.iter().map(|&(id, f)| Bound::Glyph(id, f)));
        bound.extend(b.actions.iter().map(|&(button, f)| Bound::Action(button, f)));
        bound.extend(b.lists.iter().map(|(id, l)| Bound::List(*id, l)));
        bound.extend(b.shown.iter().map(|&(f, yes, no)| Bound::Shown(f, yes, no)));
        for (i, _) in bound.iter().enumerate() {
            let r = node.reader();
            if i == 0 {
                self.first = r.0;
            }
        }
        self.scratch = Some(node.reader());
        self.told = (0..bound.len() as u32).collect();
        self.bound = bound;
        self.show(node);
    }

    /// Sets what its told readers that are not drawn show (a glyph, an
    /// action's glyph, a list's count) and marks the rest dirty, for
    /// PebbleOS to draw them again: after each settle.
    pub fn show<B: ark_kernel::Body + Clone + 'static, S: Store>(&mut self, node: &mut Node<B, S>) {
        for i in core::mem::take(&mut self.told) {
            let r = Reader(self.first + i);
            match self.bound[i as usize] {
                Bound::Words(id, _) | Bound::Meter(id, _) => sys::ui_dirty(id),
                Bound::Glyph(id, f) => {
                    let mut name: &[u8] = &[];
                    node.record(r, &mut |db| name = f(db));
                    self.set_glyph(id, name);
                }
                Bound::Action(button, f) => {
                    let mut name: &[u8] = &[];
                    node.record(r, &mut |db| name = f(db));
                    self.set_action(button, name);
                }
                Bound::List(id, l) => {
                    let mut n = 0;
                    node.record(r, &mut |db| n = (l.count)(db));
                    sys::ui_set_rows(id, n.min(usize::from(u8::MAX)) as u8);
                }
                Bound::Shown(f, yes, no) => {
                    let mut on = false;
                    node.record(r, &mut |db| on = f(db));
                    for &id in yes {
                        sys::ui_hide(id, !on);
                    }
                    for &id in no {
                        sys::ui_hide(id, on);
                    }
                }
            }
        }
    }

    /// What layer `id` shows as PebbleOS draws it: `part` 0 its words, 1 a
    /// list's heading, 2 + 2i a row's title, 3 + 2i the line under it; its
    /// reads recorded against its reader.
    pub fn pull<B: ark_kernel::Body + Clone + 'static, S: Store>(&self, node: &mut Node<B, S>, id: u8, part: u8, out: &mut Line) {
        for (i, b) in self.bound.iter().enumerate() {
            match *b {
                Bound::Words(at, f) if at == id && part == 0 => return node.record(Reader(self.first + i as u32), &mut |db| f(db, out)),
                Bound::List(at, l) if at == id => {
                    let Some(r) = self.scratch else { return };
                    let row = usize::from(part.saturating_sub(2) / 2);
                    return node.record(r, &mut |db| match part {
                        1 => (l.header)(db, out),
                        p if p % 2 == 0 => (l.title)(db, row, out),
                        _ => (l.line)(db, row, out),
                    });
                }
                _ => {}
            }
        }
    }

    /// A meter's fraction, in thousandths, as PebbleOS draws it.
    pub fn fraction<B: ark_kernel::Body + Clone + 'static, S: Store>(&self, node: &mut Node<B, S>, id: u8) -> u16 {
        for (i, b) in self.bound.iter().enumerate() {
            if let Bound::Meter(at, f) = *b {
                if at == id {
                    let mut v = 0;
                    node.record(Reader(self.first + i as u32), &mut |db| v = f(db));
                    return v;
                }
            }
        }
        0
    }

    /// The width and height the screen's content is laid out in: the
    /// window's, less the action bar's once there is one.
    pub fn room(&self) -> (i16, i16) {
        self.room
    }

    /// The action bar at the trailing edge, on `ground`, its glyphs
    /// `side` a side in `ink`: what is left of the window is the room.
    pub fn action_bar(&mut self, ground: u8, ink: u8, side: i16) {
        let w = sys::ui_action_bar(ground);
        self.room.0 -= w;
        self.bar = Some((side, ink));
    }

    fn slot(&mut self, id: u8) {
        let n = usize::from(id) + 1;
        if self.glyph_style.len() < n {
            self.glyph_style.resize(n, (0, 0, false));
        }
    }

    /// Words, one line of `font` (or as many as the frame holds).
    pub fn text(&mut self, id: u8, f: Frame, font: Font, ink: u8, align: Align) {
        sys::ui_text(id, f, font, ink, align as u8);
    }

    /// A bar of how far, `fill` over a `track` line.
    pub fn meter(&mut self, id: u8, f: Frame, fill: u8, track: u8) {
        sys::ui_meter(id, f, fill, track);
    }

    /// A filled rectangle behind what follows, its corners `corner`.
    pub fn rect(&mut self, id: u8, f: Frame, color: u8, corner: i16) {
        sys::ui_rect(id, f, color, corner);
    }

    /// A glyph `side` a side in `ink`, at the frame's centre.
    pub fn glyph(&mut self, id: u8, f: Frame, side: i16, ink: u8) {
        self.slot(id);
        self.glyph_style[usize::from(id)] = (side, ink, false);
        sys::ui_image(id, f);
    }

    /// A list: a heading over rows of a title and the line under it.
    pub fn list(&mut self, id: u8, f: Frame, style: ListStyle) {
        sys::ui_list(id, f, &style, ROW_HEIGHT, HEADER_HEIGHT, TEXT_LIFT);
    }

    /// Shows or hides a layer (one branch of an `if`).
    pub fn hide(&mut self, id: u8, hidden: bool) {
        sys::ui_hide(id, hidden);
    }

    /// The glyph called `name` (none of the screen's: nothing).
    pub fn set_glyph(&mut self, id: u8, name: &[u8]) {
        self.slot(id);
        let (side, ink, solid) = self.glyph_style[usize::from(id)];
        let bitmap = self.bitmap(name, side, ink, solid);
        sys::ui_set_image(id, bitmap);
    }

    /// The action bar's glyph for `button`, solid as the system's are
    /// (none of the screen's, or no name: none).
    pub fn set_action(&mut self, button: Button, name: &[u8]) {
        let Some((side, ink)) = self.bar else { return };
        let bitmap = self.bitmap(name, side, ink, true);
        if self.actions[button as usize] != bitmap {
            sys::ui_set_action(button as u8, bitmap);
            self.actions[button as usize] = bitmap;
        }
    }

    /// The shim's bitmap of `name` painted so, painted once.
    fn bitmap(&mut self, name: &[u8], side: i16, ink: u8, solid: bool) -> u8 {
        let Some(glyph) = self.glyphs.iter().position(|g| g.name == name) else {
            return NONE;
        };
        let glyph = glyph as u8;
        if let Some(p) = self
            .painted
            .iter()
            .find(|p| p.glyph == glyph && p.side == side && p.ink == ink && p.solid == solid)
        {
            return p.bitmap;
        }
        let bits = paint(self.glyphs[usize::from(glyph)].path, side, solid);
        let bitmap = sys::ui_bitmap(side, &bits, ink);
        if bitmap != NONE {
            self.painted.push(Painted {
                glyph,
                side,
                solid,
                ink,
                bitmap,
            });
        }
        bitmap
    }
}

/// The node's settle tells the screen: each told reader is kept, to be
/// shown ([`Ui::show`]) once the settle is done.
impl Sink for Ui {
    fn dirty(&mut self, r: Reader) {
        if let Some(i) = r.0.checked_sub(self.first).filter(|&i| (i as usize) < self.bound.len()) {
            self.told.push(i);
        }
    }
}

/// `path` painted into a square `side` pixels a side, a bit a pixel, rows
/// of whole bytes, the leftmost pixel the highest bit: its strokes
/// [`GLYPH_STROKE`] wide, or, `solid`, its closed shapes filled (their
/// insides, unstroked) and its open ones stroked. A pixel is inked when two
/// of its four samples are.
pub fn paint(path: &[u8], side: i16, solid: bool) -> Vec<u8> {
    let side = i32::from(side.max(1));
    // In quarters of a pixel: a drawing's 24 units are 192 eighths.
    let scale = |v: u8| i32::from(v) * 4 * side / 192;
    let mut segs: Vec<(i32, i32, i32, i32, bool)> = Vec::new();
    let mut at = 0;
    while at + 2 <= path.len() {
        let (n, closed) = (usize::from(path[at]), path[at + 1] == 1);
        let pts = &path[at + 2..(at + 2 + 2 * n).min(path.len())];
        at += 2 + 2 * n;
        let point = |i: usize| (scale(pts[2 * i]), scale(pts[2 * i + 1]));
        let n = pts.len() / 2;
        for i in 1..n {
            let (a, b) = (point(i - 1), point(i));
            segs.push((a.0, a.1, b.0, b.1, closed));
        }
        if closed && n > 1 {
            let (a, b) = (point(n - 1), point(0));
            segs.push((a.0, a.1, b.0, b.1, true));
        }
        if n == 1 {
            let a = point(0);
            segs.push((a.0, a.1, a.0, a.1, false));
        }
    }
    let half = i32::from(GLYPH_STROKE) * 4 * side / 192 / 2;
    let reach = half * half;
    let stride = (side as usize).div_ceil(8);
    let mut bits = alloc::vec![0u8; stride * side as usize];
    for py in 0..side {
        for px in 0..side {
            let mut inked = 0;
            for (sx, sy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                let (x, y) = (4 * px + sx, 4 * py + sy);
                let mut winding = 0;
                let mut near = false;
                for &(ax, ay, bx, by, closed) in &segs {
                    // Solid, a closed shape is its inside alone, as a
                    // filled symbol is, so what is apart stays apart.
                    let filled = solid && closed;
                    near = near || (!filled && distance2(x, y, ax, ay, bx, by) <= reach);
                    if filled && (ay <= y) != (by <= y) {
                        // Which side of the edge the sample is: left of an
                        // edge going up winds once, of one going down back.
                        let cross = (bx - ax) * (y - ay) - (x - ax) * (by - ay);
                        if by > ay && cross > 0 {
                            winding += 1;
                        } else if by <= ay && cross < 0 {
                            winding -= 1;
                        }
                    }
                }
                if near || winding != 0 {
                    inked += 1;
                }
            }
            if inked >= 2 {
                bits[py as usize * stride + px as usize / 8] |= 0x80 >> (px % 8);
            }
        }
    }
    bits
}

/// The square of the distance from (x, y) to the segment a–b.
fn distance2(x: i32, y: i32, ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    let (dx, dy) = (bx - ax, by - ay);
    let (px, py) = (x - ax, y - ay);
    let len2 = dx * dx + dy * dy;
    let t = px * dx + py * dy;
    if len2 == 0 || t <= 0 {
        return px * px + py * py;
    }
    if t >= len2 {
        let (qx, qy) = (x - bx, y - by);
        return qx * qx + qy * qy;
    }
    let cross = px * dy - py * dx;
    cross * cross / len2
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square of a closed path, and a line across: painted at 8 pixels.
    #[test]
    fn a_closed_shape_is_filled_and_a_line_stroked() {
        // The square from (4, 4) to (20, 20) of the 24, closed.
        let square = [4u8, 1, 32, 32, 160, 32, 160, 160, 32, 160];
        let solid = paint(&square, 8, true);
        let rows: Vec<u8> = solid.clone();
        // Its edges are at 1.3 and 6.7 pixels: filled, columns 1 to 6 of 0
        // to 7 (each half in), and rows 1 to 6.
        assert_eq!(rows[3], 0b0111_1110, "{rows:?}");
        assert_eq!((rows[0], rows[7]), (0, 0), "{rows:?}");
        let outline = paint(&square, 8, false);
        // Its middle is not, unfilled.
        assert_eq!(outline[3] & 0b0001_1000, 0, "{outline:?}");
        // A line from (0, 12) to (24, 12): the middle row, end to end.
        let line = [2u8, 0, 0, 96, 192, 96];
        let bits = paint(&line, 8, true);
        assert_eq!(bits[3] | bits[4], 0xff, "{bits:?}");
        assert_eq!(bits[0], 0);
    }
}
