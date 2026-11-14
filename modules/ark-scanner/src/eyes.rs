//! The scanner's eyes: what ark-camera's live page shows each frame to
//! ([`ark_camera::See`]). They find the page (the corner model where it is
//! built in and the frame has colour, the bright region otherwise), judge
//! whether the frame is sharp and the page has held still, and say so: an
//! outline for the page to draw, a sentence, and — once, when the page has
//! held still and sharp for about a second — take the picture.

use std::sync::Mutex;

use ark_camera::{Frame, Look, See};

use crate::vision::{self, Luma, Pt, Quad, Steady};

/// The least sharpness ([`vision::sharpness`] of an analysis frame) a page
/// is taken at.
pub const SHARP: f64 = 60.0;

/// What the person is told.
pub mod words {
    pub const POINT: &str = "Point the camera at the page";
    pub const BLURRY: &str = "Hold steady: it's blurry";
    pub const STILL: &str = "Hold still";
}

/// Where the page was last seen: in a frame's pixels as the sensor saw it,
/// the frame `w` by `h`, `turns` quarters from upright.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    pub quad: Quad,
    pub w: u32,
    pub h: u32,
    pub turns: u8,
}

#[derive(Default)]
struct Watching {
    steady: Steady,
    seen: Option<Seen>,
    taken: bool,
}

/// The eyes of one scan.
#[derive(Default)]
pub struct PageEyes {
    watching: Mutex<Watching>,
}

impl PageEyes {
    pub fn new() -> PageEyes {
        PageEyes::default()
    }

    /// Where the page was last seen, for the photo to be made flat by.
    pub fn seen(&self) -> Option<Seen> {
        self.watching.lock().unwrap_or_else(|e| e.into_inner()).seen
    }
}

/// The page in `frame`, in its pixels.
pub fn find(frame: &Frame, luma: Luma) -> Option<Quad> {
    #[cfg(feature = "model")]
    if !frame.rgb.is_empty() {
        let (w, h) = (frame.width as f32, frame.height as f32);
        return crate::model::corners(&frame.rgb).map(|c| Quad::ordered(std::array::from_fn(|i| Pt::new(c[2 * i] * w, c[2 * i + 1] * h))));
    }
    let _ = frame;
    vision::page(luma)
}

impl See for PageEyes {
    fn look(&self, frame: &Frame) -> Look {
        let Some(luma) = Luma::new(frame.width as usize, frame.height as usize, &frame.luma) else {
            return Look {
                words: words::POINT.into(),
                ..Look::default()
            };
        };
        let quad = find(frame, luma);
        let sharp = vision::sharpness(luma, 2) >= SHARP;
        let mut w = self.watching.lock().unwrap_or_else(|e| e.into_inner());
        let diagonal = (frame.width as f32).hypot(frame.height as f32);
        let held = w.steady.tell(quad, frame.at_ms, diagonal) && sharp;
        let Some(quad) = quad else {
            return Look {
                words: words::POINT.into(),
                ..Look::default()
            };
        };
        w.seen = Some(Seen {
            quad,
            w: frame.width,
            h: frame.height,
            turns: frame.turns,
        });
        let take = held && !w.taken;
        w.taken |= take;
        Look {
            outline: outline(&quad, frame.width, frame.height, frame.turns),
            held,
            words: if sharp { words::STILL } else { words::BLURRY }.into(),
            take,
        }
    }
}

/// `quad` stood upright with its frame, each corner 0 to 1 across it.
pub fn outline(quad: &Quad, w: u32, h: u32, turns: u8) -> Vec<f32> {
    let up = quad.rotated(turns, w, h);
    let (uw, uh) = match turns % 2 {
        1 => (h as f32, w as f32),
        _ => (w as f32, h as f32),
    };
    up.corners().iter().flat_map(|p: &Pt| [p.x / uw, p.y / uh]).collect()
}
