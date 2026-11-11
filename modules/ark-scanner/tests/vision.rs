//! The scanner's eyes held to drawn frames: a page found where it was
//! drawn, upright or tilted, and none where there is none; corners in order;
//! a quad that turns with its frame; steady only once it has held still;
//! sharp text sharper than blurred; a page made flat lands on its
//! rectangle; and the whole scan over the camera's fake, from frames to a
//! flat JPEG. (Moved from money's Android scanner, Vision.kt's JVM tests.)

use std::sync::{Arc, Mutex};

use ark_camera::{Frame, SQUARE};
use ark_module::Modules;
use ark_scanner::eyes::{self, PageEyes};
use ark_scanner::vision::{self, Luma, Pt, Quad, Steady};
use ark_scanner::{page, scan_page, warp};

/// `java.util.Random`, so the frames are the JVM tests' own, pixel for pixel.
struct JavaRandom(u64);

impl JavaRandom {
    fn new(seed: u64) -> JavaRandom {
        JavaRandom((seed ^ 0x5DEECE66D) & ((1 << 48) - 1))
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(0x5DEECE66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.0 >> (48 - bits)) as i32
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        if bound & -bound == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }
}

/// A frame of a dark, uneven table with `page` drawn on it in paper white,
/// and some text: VisionTests.kt's `frame`.
fn frame(w: usize, h: usize, page: Option<Quad>, text: bool) -> Vec<u8> {
    let mut px = vec![0u8; w * h];
    let mut r = JavaRandom::new(7);
    for y in 0..h {
        for x in 0..w {
            let mut v = 60 + (x * 30 / w) as i32 + (y * 20 / h) as i32 + r.next_int(12);
            if let Some(q) = page.filter(|q| inside(q, x as f32 + 0.5, y as f32 + 0.5)) {
                let _ = q;
                v = 225 + r.next_int(10);
                // Lines of text across the middle of the page.
                if text && (y / 6) % 3 == 0 && (x / 4) % 2 == 0 {
                    v = 40
                }
            }
            px[y * w + x] = v.clamp(0, 255) as u8;
        }
    }
    px
}

fn q(c: [(f32, f32); 4]) -> Quad {
    let p = c.map(|(x, y)| Pt::new(x, y));
    Quad {
        tl: p[0],
        tr: p[1],
        br: p[2],
        bl: p[3],
    }
}

fn inside(q: &Quad, x: f32, y: f32) -> bool {
    let c = q.corners();
    (0..4).all(|i| {
        let (a, b) = (c[i], c[(i + 1) % 4]);
        (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x) >= 0.0
    })
}

fn near(want: &Quad, got: Option<Quad>, within: f32) {
    let got = got.unwrap_or_else(|| panic!("no page found where {want:?} was drawn"));
    assert!(got.moved(want) <= within, "drawn {want:?}, found {got:?}");
}

fn turned(cx: f32, cy: f32, hw: f32, hh: f32, deg: f64) -> Quad {
    let t = deg.to_radians();
    let p = |x: f32, y: f32| {
        Pt::new(
            (cx as f64 + x as f64 * t.cos() - y as f64 * t.sin()) as f32,
            (cy as f64 + x as f64 * t.sin() + y as f64 * t.cos()) as f32,
        )
    };
    Quad {
        tl: p(-hw, -hh),
        tr: p(hw, -hh),
        br: p(hw, hh),
        bl: p(-hw, hh),
    }
}

fn luma(px: &[u8], w: usize, h: usize) -> Luma<'_> {
    Luma::new(w, h, px).unwrap()
}

#[test]
fn a_page_is_found_where_it_was_drawn() {
    let page = q([(120.0, 60.0), (520.0, 80.0), (500.0, 440.0), (140.0, 420.0)]);
    let px = frame(640, 480, Some(page), true);
    near(&page, vision::page(luma(&px, 640, 480)), 12.0);
}

#[test]
fn a_tilted_receipt_is_found() {
    // A long receipt, turned twenty degrees, as a hand holds a phone over it.
    let page = turned(320.0, 240.0, 90.0, 200.0, 20.0);
    let px = frame(640, 480, Some(page), true);
    near(&page, vision::page(luma(&px, 640, 480)), 12.0);
}

#[test]
fn a_frame_with_no_page_has_none() {
    let px = frame(640, 480, None, true);
    assert_eq!(vision::page(luma(&px, 640, 480)), None);
    let sliver = q([(10.0, 10.0), (40.0, 10.0), (40.0, 30.0), (10.0, 30.0)]);
    let px = frame(640, 480, Some(sliver), true);
    assert_eq!(vision::page(luma(&px, 640, 480)), None, "a sliver is no page");
}

#[test]
fn corners_are_ordered_clockwise_from_the_top_left() {
    let o = Quad::ordered([Pt::new(10.0, 90.0), Pt::new(90.0, 10.0), Pt::new(10.0, 10.0), Pt::new(90.0, 90.0)]);
    assert_eq!(o, q([(10.0, 10.0), (90.0, 10.0), (90.0, 90.0), (10.0, 90.0)]));
    assert!(o.convex());
    assert!(!q([(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)]).convex());
}

#[test]
fn a_quad_turns_with_its_frame() {
    // A frame 100 by 50; a quarter turn clockwise makes it 50 by 100.
    let a = q([(10.0, 5.0), (60.0, 5.0), (60.0, 25.0), (10.0, 25.0)]);
    assert_eq!(a.rotated(1, 100, 50), q([(25.0, 10.0), (45.0, 10.0), (45.0, 60.0), (25.0, 60.0)]));
    assert_eq!(a.rotated(4, 100, 50), a);
}

#[test]
fn a_page_is_steady_only_once_it_has_held_still() {
    let mut s = Steady::new(1000, 0.02);
    let a = q([(100.0, 100.0), (500.0, 100.0), (500.0, 400.0), (100.0, 400.0)]);
    let shaken = a.scaled(1.2);
    assert!(!s.tell(Some(a), 0, 800.0));
    assert!(!s.tell(Some(a), 500, 800.0));
    assert!(s.tell(Some(a), 1000, 800.0), "a second still");
    assert!(!s.tell(Some(shaken), 1100, 800.0), "moved");
    assert!(!s.tell(Some(shaken), 1900, 800.0));
    assert!(s.tell(Some(shaken), 2100, 800.0));
    assert!(!s.tell(None, 2200, 800.0), "lost");
    assert!(!s.tell(Some(shaken), 2300, 800.0));
}

#[test]
fn sharp_text_is_sharper_than_blurred() {
    let page = q([(80.0, 40.0), (560.0, 40.0), (560.0, 440.0), (80.0, 440.0)]);
    let sharp = frame(640, 480, Some(page), true);
    let mut soft: Vec<i32> = sharp.iter().map(|&v| v as i32).collect();
    for _ in 0..4 {
        soft = vision::blur(&soft, 640, 480);
    }
    let blurred: Vec<u8> = soft.iter().map(|&v| v as u8).collect();
    let (a, b) = (
        vision::sharpness(luma(&sharp, 640, 480), 2),
        vision::sharpness(luma(&blurred, 640, 480), 2),
    );
    assert!(a > 4.0 * b, "sharp {a} over blurred {b}");
    assert!(a >= eyes::SHARP, "sharp text is taken: {a}");
}

#[test]
fn a_page_made_flat_lands_on_its_rectangle() {
    let a = q([(20.0, 10.0), (80.0, 20.0), (70.0, 90.0), (10.0, 80.0)]);
    let m = warp::homography(&a, 60, 80).unwrap();
    for (rect, corner) in [(0.0, 0.0), (60.0, 0.0), (60.0, 80.0), (0.0, 80.0)].into_iter().zip(a.corners()) {
        let (u, v) = warp::map(&m, rect.0, rect.1);
        assert!((u - corner.x as f64).abs() < 1e-6 && (v - corner.y as f64).abs() < 1e-6);
    }
    // A square of white in a black frame, flattened, is white throughout.
    let w = 100u32;
    let src: Vec<u8> = (0..w * w)
        .flat_map(|i| match inside(&a, (i % w) as f32 + 0.5, (i / w) as f32 + 0.5) {
            true => [255, 255, 255],
            false => [0, 0, 0],
        })
        .collect();
    let flat = warp::flatten(&src, w, w, &a, 30, 40).unwrap();
    let white = flat.chunks(3).filter(|p| p == &[255, 255, 255]).count();
    assert!(white as f32 > 0.95 * (30 * 40) as f32, "{white} of {} white", 30 * 40);
    assert_eq!(warp::homography(&Quad::default(), 10, 10), None, "corners in a line make no rectangle");
}

/// The eyes on a stream of frames: no page said, then the page outlined
/// upright, and the picture taken once it has held a second, and only
/// once. Falsified by taking the picture on the first steady frame twice.
#[test]
fn the_eyes_take_the_picture_once_the_page_holds_still() {
    let e = PageEyes::new();
    let none = frame(640, 480, None, true);
    let look = ark_camera::See::look(
        &e,
        &Frame {
            width: 640,
            height: 480,
            luma: none,
            turns: 1,
            ..Frame::default()
        },
    );
    assert_eq!((look.words.as_str(), look.outline.len(), look.take), (eyes::words::POINT, 0, false));
    let page = q([(120.0, 60.0), (520.0, 80.0), (500.0, 440.0), (140.0, 420.0)]);
    let px = frame(640, 480, Some(page), true);
    let mut takes = vec![];
    for at_ms in (0..=1600).step_by(100) {
        let l = ark_camera::See::look(
            &e,
            &Frame {
                width: 640,
                height: 480,
                luma: px.clone(),
                turns: 1,
                at_ms,
                ..Frame::default()
            },
        );
        assert_eq!(l.outline.len(), 8);
        assert_eq!(l.words, eyes::words::STILL);
        takes.push(l.take);
    }
    assert_eq!(takes.iter().filter(|t| **t).count(), 1, "taken once: {takes:?}");
    assert!(takes[..10].iter().all(|t| !t), "not before a second has passed");
    // Upright (a quarter turn): the frame 480 wide and 640 high, the
    // page's bottom left (140, 420) its top left, at (480 - 420, 140).
    let o = eyes::outline(&page, 640, 480, 1);
    assert!(o.iter().all(|c| (0.0..=1.0).contains(c)));
    assert!((o[0] - 60.0 / 480.0).abs() < 1e-5 && (o[1] - 140.0 / 640.0).abs() < 1e-5, "{o:?}");
}

/// The whole scan over the camera's fake, as a phone runs it: the
/// scanner asks the camera to watch, the eyes see the frames, the photo
/// comes back, and the flat page is a JPEG of the page's proportions
/// stood upright. Falsified by flattening the photo without the quad
/// scaled from the analysis frame: the page is the whole photo.
#[test]
fn a_scan_over_the_camera_is_the_page_made_flat() {
    let camera = ark_camera::Fake::new();
    let mut modules = Modules::new();
    modules.register::<dyn ark_scanner::Scanner>(ark_scanner::over_camera(Arc::new(camera.clone())));
    let got = Arc::new(Mutex::new(vec![]));
    let g = got.clone();
    modules.run(&scan_page(), move |a| g.lock().unwrap().push(a));
    assert!(camera.watching());
    // The analysis frame 320 by 240, the page a landscape strip in it.
    let page = q([(60.0, 70.0), (260.0, 70.0), (260.0, 170.0), (60.0, 170.0)]);
    let px = frame(320, 240, Some(page), true);
    for at_ms in [0, 600, 1200] {
        camera.show(Frame {
            width: 320,
            height: 240,
            luma: px.clone(),
            turns: 1,
            at_ms,
            ..Frame::default()
        });
    }
    // The photo, twice the frame, as the sensor took it.
    let big = frame(640, 480, Some(page.scaled(2.0)), true);
    let rgb: Vec<u8> = big.iter().flat_map(|&v| [v, v, v]).collect();
    let photo = image::RgbImage::from_raw(640, 480, rgb).unwrap();
    let mut jpeg = std::io::Cursor::new(Vec::new());
    photo.write_to(&mut jpeg, image::ImageFormat::Jpeg).unwrap();
    camera.watched(Some(jpeg.into_inner()));
    let answers = got.lock().unwrap();
    let flat = answers[0].as_ref().unwrap().as_ref().unwrap();
    let img = image::load_from_memory(flat).unwrap();
    // The page 400 by 200 in the photo, stood upright: about 200 by 400.
    assert!(
        (img.width() as i32 - 200).abs() <= 8 && (img.height() as i32 - 400).abs() <= 8,
        "{}x{}",
        img.width(),
        img.height()
    );
    // Backed out is none; a photo that does not decode is a failure.
    assert_eq!(page::flat(b"not a jpeg", None, 0), None);
}

#[test]
fn a_frame_too_short_for_its_size_is_nothing_seen() {
    let l = ark_camera::See::look(
        &PageEyes::new(),
        &Frame {
            width: 640,
            height: 480,
            luma: vec![0; 10],
            ..Frame::default()
        },
    );
    assert!(l.outline.is_empty() && !l.take);
    let _ = SQUARE;
}
