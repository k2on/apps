//! Pebble's toolkit for arkui (ui/docs/pebble.md): what a screen
//! generated from a `#[view]` (ark-ui-gen's `pebble` emitter) is drawn
//! with. The emitter writes, at build time, a `build` that makes the
//! screen's layers once — each a native layer of PebbleOS's through the C
//! shim (`c/ark_pebble.c`): a `TextLayer`, a drawn meter, a `BitmapLayer`
//! of a glyph, a `MenuLayer` for a list, the `ActionBarLayer` — at frames
//! worked out from the window it is given and the theme's tokens
//! ([`crate::tokens`]), and a `draw` that hands each one its value from
//! the record. [`Ui`] keeps what each layer last showed and tells the shim
//! only what changed, so only those layers are marked dirty and redrawn.
//!
//! Glyphs are arkui's (Lucide's drawings), flattened into line segments
//! by the emitter ([`Glyph`]) and painted here into 1-bit bitmaps at the
//! size a style or the action bar asks, once each, so the app has no
//! resources. Nothing here moves: a first cut animates nothing.

use alloc::vec::Vec;

use crate::sys;
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

/// What a layer last showed.
#[derive(Default)]
enum Held {
    #[default]
    Nothing,
    Text(Vec<u8>),
    Fraction(u16),
    Glyph(u8),
    Rows(Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>),
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
    held: Vec<Held>,
    hidden: Vec<bool>,
    /// Each glyph layer's side, colour and whether it is solid.
    glyph_style: Vec<(i16, u8, bool)>,
    painted: Vec<Painted>,
    actions: [u8; 4],
    /// The action bar's glyphs' side and colour, once there is one.
    bar: Option<(i16, u8)>,
    room: (i16, i16),
}

impl Ui {
    /// A screen over the whole window, its ground `ground`, drawing
    /// `glyphs`.
    pub fn new(glyphs: &'static [Glyph], ground: u8) -> Ui {
        sys::ui_window(ground);
        Ui {
            glyphs,
            held: Vec::new(),
            hidden: Vec::new(),
            glyph_style: Vec::new(),
            painted: Vec::new(),
            actions: [NONE; 4],
            bar: None,
            room: sys::ui_bounds(),
        }
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
        if self.held.len() < n {
            self.held.resize_with(n, Held::default);
            self.hidden.resize(n, false);
            self.glyph_style.resize(n, (0, 0, false));
        }
    }

    /// Words, one line of `font` (or as many as the frame holds).
    pub fn text(&mut self, id: u8, f: Frame, font: Font, ink: u8, align: Align) {
        self.slot(id);
        sys::ui_text(id, f, font, ink, align as u8);
    }

    /// A bar of how far, `fill` over a `track` line.
    pub fn meter(&mut self, id: u8, f: Frame, fill: u8, track: u8) {
        self.slot(id);
        sys::ui_meter(id, f, fill, track);
    }

    /// A filled rectangle behind what follows, its corners `corner`.
    pub fn rect(&mut self, id: u8, f: Frame, color: u8, corner: i16) {
        self.slot(id);
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
        self.slot(id);
        sys::ui_list(id, f, &style, ROW_HEIGHT, HEADER_HEIGHT, TEXT_LIFT);
    }

    /// Shows or hides a layer (one branch of an `if`).
    pub fn hide(&mut self, id: u8, hidden: bool) {
        self.slot(id);
        if self.hidden[usize::from(id)] != hidden {
            self.hidden[usize::from(id)] = hidden;
            sys::ui_hide(id, hidden);
        }
    }

    pub fn set_text(&mut self, id: u8, s: &[u8]) {
        self.slot(id);
        let held = &mut self.held[usize::from(id)];
        if !matches!(held, Held::Text(t) if t.as_slice() == s) {
            sys::ui_set_text(id, s);
            *held = Held::Text(s.to_vec());
        }
    }

    /// The meter at `fraction` of the way, 0 to 1.
    pub fn set_meter(&mut self, id: u8, fraction: f64) {
        self.slot(id);
        let permille = (fraction.clamp(0.0, 1.0) * 1000.0) as u16;
        let held = &mut self.held[usize::from(id)];
        if !matches!(held, Held::Fraction(p) if *p == permille) {
            sys::ui_set_meter(id, permille);
            *held = Held::Fraction(permille);
        }
    }

    /// The glyph called `name` (none of the screen's: nothing).
    pub fn set_glyph(&mut self, id: u8, name: &[u8]) {
        self.slot(id);
        let (side, ink, solid) = self.glyph_style[usize::from(id)];
        let bitmap = self.bitmap(name, side, ink, solid);
        let held = &mut self.held[usize::from(id)];
        if !matches!(held, Held::Glyph(b) if *b == bitmap) {
            sys::ui_set_image(id, bitmap);
            *held = Held::Glyph(bitmap);
        }
    }

    /// The list's heading and rows.
    pub fn set_rows<'a>(&mut self, id: u8, header: &[u8], rows: impl Iterator<Item = (&'a [u8], &'a [u8])>) {
        self.slot(id);
        let rows: Vec<(Vec<u8>, Vec<u8>)> = rows.map(|(t, s)| (t.to_vec(), s.to_vec())).collect();
        let held = &mut self.held[usize::from(id)];
        let (old_header, old) = match held {
            Held::Rows(h, r) => (Some(h.as_slice()), r.as_slice()),
            _ => (None, &[][..]),
        };
        if old_header == Some(header) && old == rows.as_slice() {
            return;
        }
        if old_header != Some(header) {
            sys::ui_set_header(id, header);
        }
        for (i, (t, s)) in rows.iter().enumerate() {
            if old.get(i) != Some(&(t.clone(), s.clone())) {
                sys::ui_set_row(id, i as u8, t, s);
            }
        }
        sys::ui_set_rows(id, rows.len() as u8);
        *held = Held::Rows(header.to_vec(), rows);
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
