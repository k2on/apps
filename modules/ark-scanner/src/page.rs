//! The photo's page made flat and upright: where the eyes last saw it,
//! scaled from the analysis frame to the photo, only the rectangle round it
//! decoded at full size, flattened, and turned upright; the whole photo
//! turned when no page was seen. Then a JPEG again.

use image::{imageops, DynamicImage, ImageFormat, RgbImage};

use crate::eyes::Seen;
use crate::warp;

/// The longest side the whole photo is kept at, a page not found in it: a
/// little past the 1920 a server reads a receipt at.
pub const WHOLE: u32 = 2600;
/// The longest side the rectangle round a page is kept at before it is halved.
pub const PAGE: u32 = 4000;

/// `jpeg` (as the sensor took it) made the page alone, flat and upright, as
/// a JPEG; none when it does not decode.
pub fn flat(jpeg: &[u8], seen: Option<Seen>, turns: u8) -> Option<Vec<u8>> {
    let photo = image::load_from_memory_with_format(jpeg, ImageFormat::Jpeg).ok()?.to_rgb8();
    let (pw, ph) = photo.dimensions();
    let quad = seen
        .filter(|s| (pw as f32 / ph as f32 - s.w as f32 / s.h as f32).abs() < 0.05)
        .map(|s| s.quad.scaled(pw as f32 / s.w as f32));
    let page = match quad {
        Some(q) => alone(&photo, &q)?,
        None => whole(photo),
    };
    let upright = match turns % 4 {
        1 => imageops::rotate90(&page),
        2 => imageops::rotate180(&page),
        3 => imageops::rotate270(&page),
        _ => page,
    };
    let mut out = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(&DynamicImage::ImageRgb8(upright))
        .ok()?;
    Some(out.into_inner())
}

/// The whole photo, halved until it is at most [`WHOLE`] on its longest side.
fn whole(photo: RgbImage) -> RgbImage {
    let (w, h) = photo.dimensions();
    let mut sample = 1;
    while w.max(h) / sample > WHOLE {
        sample *= 2;
    }
    match sample {
        1 => photo,
        s => imageops::resize(&photo, w / s, h / s, imageops::FilterType::Triangle),
    }
}

/// The page alone, flat, at the photo's own resolution: only the rectangle
/// round `q` is taken, halved only past [`PAGE`]. A receipt is a strip of the
/// photo, and halving the whole photo first leaves its small print unread.
fn alone(photo: &RgbImage, q: &crate::vision::Quad) -> Option<RgbImage> {
    let (pw, ph) = photo.dimensions();
    let xs = q.corners().map(|p| p.x);
    let ys = q.corners().map(|p| p.y);
    let min = |v: [f32; 4]| v.iter().copied().fold(f32::MAX, f32::min);
    let max = |v: [f32; 4]| v.iter().copied().fold(f32::MIN, f32::max);
    let left = (min(xs).floor() as i64).clamp(0, pw as i64 - 1) as u32;
    let top = (min(ys).floor() as i64).clamp(0, ph as i64 - 1) as u32;
    let right = (max(xs).ceil() as i64).clamp(left as i64 + 1, pw as i64) as u32;
    let bottom = (max(ys).ceil() as i64).clamp(top as i64 + 1, ph as i64) as u32;
    let (rw, rh) = (right - left, bottom - top);
    let mut sample = 1;
    while rw.max(rh) / sample > PAGE {
        sample *= 2;
    }
    let mut part = imageops::crop_imm(photo, left, top, rw, rh).to_image();
    if sample > 1 {
        part = imageops::resize(&part, rw / sample, rh / sample, imageops::FilterType::Triangle);
    }
    let local = q.shifted(-(left as f32), -(top as f32)).scaled(part.width() as f32 / rw as f32);
    let (fw, fh) = local.flat_size();
    let px = warp::flatten(part.as_raw(), part.width(), part.height(), &local, fw, fh)?;
    RgbImage::from_raw(fw, fh, px)
}
