//! Pictures by URL: fetched once, shrunk once, remembered.
//!
//! The problem has two halves: doing the fetch once, and handing the renderer
//! something the size of the thing on screen.
//!
//! **The second half is the one that was wrong, and it stuttered.**
//! `image::Handle::from_bytes` does not decode — it hands iced the encoded
//! bytes and the decode happens in whichever frame first *draws* them. So a
//! 960×1262 portrait was a 1.2-megapixel JPEG decoded inside a frame, and then
//! 4.8 MB of RGBA in the texture atlas for something 132 pixels wide; iced
//! evicts what a frame did not use, so leaving a page and coming back decoded
//! them all again — a hitch on *navigation*, both ways. So the decode happens
//! here, where the fetch already is, and what reaches the renderer is
//! `Handle::from_rgba` at most [`BOUND`] on its longest side.
//!
//! **Two caches, because the two targets have different ones to offer.** On a
//! desktop it is a directory under `XDG_CACHE_HOME`, so a picture survives a
//! restart. In a browser the disk belongs to the browser, so it is the Cache
//! API — keyed by URL, which is exactly the shape of this question. Both sit
//! behind one [`Images::want`] / [`Images::handle`] pair.
//!
//! What both cache is the *encoded* original — the fetch, not the decode — so
//! the cache could be re-shrunk if the bound ever moved, and it is a tenth of
//! the disk. What is not cached beyond the session is the `Handle`: it holds
//! decoded pixels, and the map here is the session's while the disk is the
//! machine's.
use std::collections::HashMap;

use iced::widget::image;
use iced::Task;

/// The longest side a picture is kept at, in pixels: a shade under three times
/// the largest square harken draws (132), which covers every display anybody
/// has and leaves the atlas holding at most 590 KB a picture.
///
/// A *bound*, not a size: aspect is kept, so `ContentFit::Cover` still does
/// the cropping in `view`, where that decision belongs. Anything already
/// smaller is left exactly as it is.
pub const BOUND: u32 = 384;

/// The bound for a picture drawn across a page: a banner's width at twice
/// the pixels of a wide window.
pub const WIDE: u32 = 2400;

/// Where one picture has got to.
enum State {
    /// Asked for, not answered yet. Held so a second frame does not ask again.
    Loading,
    Ready(image::Handle),
    /// Asked for and refused. Remembered rather than retried: a 404 is not
    /// going to become a 200 because the window was redrawn. Why is dropped on
    /// purpose — a picture is decoration, the derived square is already right,
    /// and a status line saying "could not fetch" forty times would bury the
    /// notes that matter.
    Missing,
}

/// Every picture this session has asked about.
pub struct Images {
    by_url: HashMap<String, State>,
    /// The cache's name: a directory under `XDG_CACHE_HOME` on the desktop, a
    /// Cache API store in a browser. An app's own, so two apps never share one.
    name: String,
    bound: u32,
}

impl Images {
    /// A cache called `name` — `harken/covers`, say — at [`BOUND`].
    pub fn new(name: impl Into<String>) -> Self {
        Images {
            by_url: HashMap::new(),
            name: name.into(),
            bound: BOUND,
        }
    }

    /// The handle to draw, if there is one yet.
    ///
    /// `None` covers "not asked", "still coming" and "there is no such image"
    /// alike, because the answer to each is the same: draw the derived square.
    /// A spinner where a picture will be is worse than the square that is
    /// already right.
    pub fn handle(&self, url: &str) -> Option<&image::Handle> {
        match self.by_url.get(url) {
            Some(State::Ready(handle)) => Some(handle),
            _ => None,
        }
    }

    /// A cache called `name` whose pictures are kept at most `bound` on their
    /// longest side.
    pub fn bounded(name: impl Into<String>, bound: u32) -> Self {
        Images { bound, ..Images::new(name) }
    }

    /// Start fetching this one, unless it is already known — or empty, which is
    /// "nobody set one" and not a URL to go and fail on.
    ///
    /// A `Task` rather than doing the work, because `update` is where side
    /// effects happen. Idempotent, which is what makes asking for *every*
    /// picture on a page affordable: scrolling does not go through `update`,
    /// so a picture that only starts loading once visible is never there when
    /// you look at it.
    pub fn want(&mut self, url: String) -> Option<Task<Loaded>> {
        if url.is_empty() || self.by_url.contains_key(&url) {
            return None;
        }
        self.by_url.insert(url.clone(), State::Loading);
        Some(imp::fetch(url, self.name.clone(), self.bound))
    }

    /// What a finished fetch did. `from_rgba`, because the pixels are already
    /// decoded and already the size they will be drawn at.
    pub fn loaded(&mut self, done: Loaded) {
        let state = match done.image {
            Ok(rgba) => State::Ready(image::Handle::from_rgba(rgba.width, rgba.height, rgba.pixels)),
            Err(_) => State::Missing,
        };
        self.by_url.insert(done.url, state);
    }
}

/// How many pixels a face ([`round`]) is kept at for each point it is
/// drawn across: a HiDPI screen's two, which a 1× screen draws by
/// averaging and a 1.5× one by filtering down. Sharper than that is past
/// what a 28-point button shows.
pub const DENSITY: f32 = 2.0;

/// The most faces [`round`] remembers before it starts again: a page of
/// people, at the few sizes they are drawn, many times over.
const ROUND_KEPT: usize = 1024;

/// Every face [`round`] has made, by picture and pixel size.
static ROUNDED: std::sync::Mutex<Option<HashMap<(iced::advanced::image::Id, u32), image::Handle>>> = std::sync::Mutex::new(None);

/// `picture` as a face `side` points across, made round in its pixels: the
/// middle square cut out (what `ContentFit::Cover` shows), resampled to
/// `side` × [`DENSITY`] pixels, and everything outside the circle in it
/// made transparent, the edge anti-aliased.
///
/// **In the pixels, because the renderer cannot be trusted with it.** An
/// image's `border_radius` is wgpu's alone: tiny-skia, which iced draws with
/// when wgpu opens no device (`crate::renderer`), draws the picture whole,
/// so a face was a square on exactly the machines that also had no GPU. A
/// circle that is in the picture is a circle on every renderer; the
/// widget's `border_radius` stays, and on wgpu agrees with it.
///
/// Made once for each picture and size and remembered, so the frames after
/// the first hand over the same handle: the atlas keeps one texture and a
/// page of faces costs its resampling once. A handle that is not decoded
/// pixels — encoded bytes, a path — comes back as it is.
pub fn round(picture: &image::Handle, side: f32) -> image::Handle {
    let image::Handle::Rgba { width, height, pixels, .. } = picture else {
        return picture.clone();
    };
    let out = ((side * DENSITY).round() as u32).max(1);
    let key = (picture.id(), out);
    let mut kept = ROUNDED.lock().unwrap_or_else(|e| e.into_inner());
    let kept = kept.get_or_insert_with(HashMap::new);
    if let Some(done) = kept.get(&key) {
        return done.clone();
    }
    if kept.len() >= ROUND_KEPT {
        kept.clear();
    }
    let done = image::Handle::from_rgba(out, out, circle(pixels, *width, *height, out));
    kept.insert(key, done.clone());
    done
}

/// The middle square of an RGBA picture `width` × `height`, resampled to
/// `out` × `out` and masked to the circle inscribed in it: four bytes a
/// pixel, unpremultiplied, as `Handle::from_rgba` takes them.
///
/// Each pixel out is the average of up to 8 × 8 bilinear samples across
/// the part of the picture it covers — a box filter, so a 384-pixel
/// portrait shrunk to a 56-pixel button keeps its face rather than every
/// seventh pixel of it. Averaged premultiplied, so a transparent pixel's
/// colour does not bleed in. The mask is the circle's coverage of each
/// pixel, from the distance of its centre to the rim: 1 inside, 0 outside,
/// and the pixel the rim crosses in between.
pub fn circle(pixels: &[u8], width: u32, height: u32, out: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut rgba = vec![0u8; out as usize * out as usize * 4];
    if w == 0 || h == 0 || pixels.len() < w * h * 4 {
        return rgba;
    }
    let side = w.min(h) as f32;
    let (x0, y0) = ((w as f32 - side) / 2.0, (h as f32 - side) / 2.0);
    let step = side / out as f32;
    let taps = (step.ceil() as usize).clamp(1, 8);
    // One sample, premultiplied, at a point in the picture's pixels.
    let at = |x: f32, y: f32| -> [f32; 4] {
        let x = (x - 0.5).clamp(0.0, (w - 1) as f32);
        let y = (y - 0.5).clamp(0.0, (h - 1) as f32);
        let (xi, yi) = (x.floor() as usize, y.floor() as usize);
        let (xj, yj) = ((xi + 1).min(w - 1), (yi + 1).min(h - 1));
        let (fx, fy) = (x - xi as f32, y - yi as f32);
        let px = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            let a = pixels[i + 3] as f32 / 255.0;
            [pixels[i] as f32 * a, pixels[i + 1] as f32 * a, pixels[i + 2] as f32 * a, a]
        };
        let (p00, p10, p01, p11) = (px(xi, yi), px(xj, yi), px(xi, yj), px(xj, yj));
        let mut s = [0.0; 4];
        for c in 0..4 {
            let top = p00[c] + (p10[c] - p00[c]) * fx;
            let bottom = p01[c] + (p11[c] - p01[c]) * fx;
            s[c] = top + (bottom - top) * fy;
        }
        s
    };
    let r = out as f32 / 2.0;
    for j in 0..out as usize {
        for i in 0..out as usize {
            let d = ((i as f32 + 0.5 - r).powi(2) + (j as f32 + 0.5 - r).powi(2)).sqrt();
            let cover = (r - d + 0.5).clamp(0.0, 1.0);
            if cover == 0.0 {
                continue;
            }
            let mut sum = [0.0f32; 4];
            for ty in 0..taps {
                for tx in 0..taps {
                    let x = x0 + (i as f32 + (tx as f32 + 0.5) / taps as f32) * step;
                    let y = y0 + (j as f32 + (ty as f32 + 0.5) / taps as f32) * step;
                    let s = at(x, y);
                    for c in 0..4 {
                        sum[c] += s[c];
                    }
                }
            }
            let n = (taps * taps) as f32;
            let a = sum[3] / n;
            let o = (j * out as usize + i) * 4;
            if a > 0.0 {
                for c in 0..3 {
                    rgba[o + c] = (sum[c] / n / a).round().clamp(0.0, 255.0) as u8;
                }
            }
            rgba[o + 3] = (a * cover * 255.0).round() as u8;
        }
    }
    rgba
}

/// A decoded picture, at most the bound on its longest side.
#[derive(Clone)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    /// Four bytes a pixel, row major.
    pub pixels: Vec<u8>,
}

// Written out rather than derived: an app's messages are `Debug`, and a derived
// one would put half a megabyte of pixels in anything that prints one.
impl std::fmt::Debug for Rgba {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rgba({}\u{d7}{}, {} B)", self.width, self.height, self.pixels.len())
    }
}

/// One finished fetch, on its way back into `update`.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub url: String,
    pub image: Result<Rgba, String>,
}

/// FNV-1a over the bytes of a URL, as sixteen hex digits: the cache's filename.
/// Sixty-four bits, not [`crate::art::hash`]'s thirty-two — a collision there
/// is the wrong shade of a gradient and here it is the wrong picture.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn key(url: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.as_bytes() {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::{key, Loaded, Rgba};
    use iced::Task;

    /// Where a picture lives between runs: a *cache*, so where a machine sweeps
    /// caches.
    fn dir(name: &str) -> Option<std::path::PathBuf> {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
        Some(base.join(name))
    }

    /// The pixels, from the disk if the bytes are there and the network if
    /// not, decoded and shrunk before they come back.
    ///
    /// Blocking, on a thread of its own, with a oneshot carrying the answer
    /// back — so this needs no runtime, and the desktop's `smol` executor is
    /// not stalled behind an HTTP call. The decode rides on that thread for the
    /// reason it is not in the renderer: taking twenty milliseconds there
    /// costs nobody anything.
    pub fn fetch(url: String, name: String, bound: u32) -> Task<Loaded> {
        let (tx, rx) = iced::futures::channel::oneshot::channel();
        let back = url.clone();
        std::thread::spawn(move || {
            let _ = tx.send(blocking(&back, &name).and_then(|bytes| shrink(&bytes, bound)));
        });
        Task::perform(
            async move { rx.await.unwrap_or_else(|_| Err("the fetch was dropped".to_string())) },
            move |image| Loaded { url: url.clone(), image },
        )
    }

    fn blocking(url: &str, name: &str) -> Result<Vec<u8>, String> {
        let path = dir(name).map(|d| d.join(key(url)));
        if let Some(bytes) = path.as_ref().and_then(|p| std::fs::read(p).ok()) {
            return Ok(bytes);
        }
        let response = ureq::get(url).call().map_err(|e| format!("could not fetch {url}: {e}"))?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes).map_err(|e| format!("could not read {url}: {e}"))?;
        // Written after it is whole, and a failed write is not a failed fetch:
        // a machine with no writable cache should still show pictures, it
        // should just fetch them again next time.
        if let Some(path) = &path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, &bytes);
        }
        Ok(bytes)
    }

    /// Decode, and shrink to fit `bound` if it does not already.
    ///
    /// `resize` fits inside the box given and keeps the aspect ratio.
    /// `Triangle` is a weighted downscale — `Nearest` at these ratios drops
    /// most of the pixels it is averaging over, and a portrait comes out
    /// speckled.
    pub fn shrink(bytes: &[u8], bound: u32) -> Result<Rgba, String> {
        let decoded = ::image::load_from_memory(bytes).map_err(|e| format!("could not decode: {e}"))?;
        let decoded = match decoded.width().max(decoded.height()) > bound {
            true => decoded.resize(bound, bound, ::image::imageops::FilterType::Triangle),
            false => decoded,
        };
        let rgba = decoded.into_rgba8();
        Ok(Rgba {
            width: rgba.width(),
            height: rgba.height(),
            pixels: rgba.into_raw(),
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::{Loaded, Rgba};
    use iced::Task;
    use wasm_bindgen::prelude::*;

    // A snippet rather than `web-sys`: `CacheStorage` is behind
    // `web_sys_unstable_apis`, a `RUSTFLAGS` every build of the crate would
    // have to agree on. This travels inside the module instead.
    //
    // `caches` needs a secure context, so it is absent on plain `http://` that
    // is not localhost — a fall-through to a plain fetch, not a failure.
    //
    // The shrink is `createImageBitmap`, the browser's own decoder, off the
    // main thread. Decoded from the fetched *blob*, never from the URL: a
    // canvas that has drawn a cross-origin image is tainted and `getImageData`
    // throws, while a blob the page already holds is readable whatever its
    // origin.
    #[wasm_bindgen(inline_js = r#"
export async function picture(url, name, bound) {
  let store = null;
  try { store = await caches.open(name); } catch (e) { store = null; }
  let response = store ? await store.match(url) : null;
  if (!response) {
    response = await fetch(url, { mode: 'cors' });
    if (!response.ok) throw new Error('HTTP ' + response.status);
    // Put the clone, read the original: a body can be consumed exactly once.
    if (store) { try { await store.put(url, response.clone()); } catch (e) {} }
  }
  const source = await createImageBitmap(await response.blob());
  const long = Math.max(source.width, source.height);
  const scale = long > bound ? bound / long : 1;
  const w = Math.max(1, Math.round(source.width * scale));
  const h = Math.max(1, Math.round(source.height * scale));
  const canvas = typeof OffscreenCanvas === 'function'
    ? new OffscreenCanvas(w, h)
    : Object.assign(document.createElement('canvas'), { width: w, height: h });
  const context = canvas.getContext('2d', { willReadFrequently: true });
  context.drawImage(source, 0, 0, w, h);
  source.close();
  return { width: w, height: h, data: context.getImageData(0, 0, w, h).data };
}
"#)]
    extern "C" {
        #[wasm_bindgen(catch)]
        async fn picture(url: &str, name: &str, bound: u32) -> Result<JsValue, JsValue>;
    }

    fn field(value: &JsValue, name: &str) -> Result<JsValue, String> {
        js_sys::Reflect::get(value, &JsValue::from_str(name)).map_err(|_| format!("the picture had no {name}"))
    }

    pub fn fetch(url: String, name: String, bound: u32) -> Task<Loaded> {
        let back = url.clone();
        Task::perform(
            async move {
                let value = picture(&back, &name, bound)
                    .await
                    .map_err(|e| format!("could not fetch {back}: {}", e.as_string().unwrap_or_else(|| "failed".into())))?;
                let width = field(&value, "width")?.as_f64().unwrap_or_default() as u32;
                let height = field(&value, "height")?.as_f64().unwrap_or_default() as u32;
                let pixels = js_sys::Uint8Array::new(&field(&value, "data")?).to_vec();
                Ok(Rgba { width, height, pixels })
            },
            move |image| Loaded { url: url.clone(), image },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{key, Images, BOUND};

    /// A cache filename has to be a function of the URL and nothing else, or
    /// two runs disagree about where a picture is and the cache never hits.
    /// Falsified by mixing the time into `key`.
    #[test]
    fn the_key_is_the_url() {
        assert_eq!(key("https://example.com/a.jpg"), key("https://example.com/a.jpg"));
        assert_ne!(key("https://example.com/a.jpg"), key("https://example.com/b.jpg"));
        assert_eq!(key("").len(), 16);
    }

    /// A second ask for a picture already in flight is a no-op, or a grid on
    /// screen issues one request per card per frame. Falsified by dropping the
    /// `contains_key` check.
    #[test]
    fn asking_twice_fetches_once() {
        let mut images = Images::new("arkui-test");
        assert!(images.want("http://127.0.0.1:9/a.jpg".into()).is_some());
        assert!(images.want("http://127.0.0.1:9/a.jpg".into()).is_none());
    }

    /// An empty URL is "nobody set one", not a URL to go and fail on.
    #[test]
    fn nothing_is_not_a_picture() {
        assert!(Images::new("arkui-test").want(String::new()).is_none());
    }

    /// A portrait the size harken's demo seeds must not reach the renderer at
    /// the size it was fetched. 960×1262 was going into the atlas as 4.8 MB to
    /// be drawn 132 pixels wide. Falsified by deleting the `resize`.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_portrait_is_shrunk_to_what_is_drawn() {
        let mut big = ::image::RgbImage::new(960, 1262);
        // Not a flat fill: a solid image encodes to almost nothing and would
        // prove the decoder ran on no more than the header.
        for (x, y, pixel) in big.enumerate_pixels_mut() {
            *pixel = ::image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 239) as u8]);
        }
        let mut png = std::io::Cursor::new(Vec::new());
        ::image::DynamicImage::ImageRgb8(big)
            .write_to(&mut png, ::image::ImageFormat::Png)
            .expect("the fixture encodes");

        let out = super::imp::shrink(png.get_ref(), BOUND).expect("a PNG decodes");
        assert!(
            out.width.max(out.height) <= BOUND,
            "a picture reaches the renderer at {}\u{d7}{}, {:.1} MB of atlas",
            out.width,
            out.height,
            (out.width as f64 * out.height as f64 * 4.0) / 1e6,
        );
        // Aspect kept: 960/1262 at a 384 bound is 292×384.
        assert_eq!((out.width, out.height), (292, 384));
        assert_eq!(out.pixels.len(), 292 * 384 * 4);
    }

    /// A face's corners are transparent and its middle is the picture,
    /// opaque, with the rim between anti-aliased — on any renderer, because
    /// it is in the pixels. Falsified by a hard mask (no pixel between 0
    /// and 255 on the rim) or by none (opaque corners).
    #[test]
    fn a_face_is_a_circle_in_its_pixels() {
        // Not square, so the crop is exercised: 120 × 80, red, opaque.
        let (w, h) = (120u32, 80u32);
        let pixels: Vec<u8> = (0..w * h).flat_map(|_| [200u8, 30, 30, 255]).collect();
        let out = 56u32;
        let face = super::circle(&pixels, w, h, out);
        assert_eq!(face.len(), (out * out * 4) as usize);
        let alpha = |x: u32, y: u32| face[((y * out + x) * 4 + 3) as usize];
        for (x, y) in [(0, 0), (out - 1, 0), (0, out - 1), (out - 1, out - 1), (4, 4)] {
            assert_eq!(alpha(x, y), 0, "corner ({x}, {y}) is drawn");
        }
        assert_eq!(alpha(out / 2, out / 2), 255, "the middle is not opaque");
        let middle = ((out / 2 * out + out / 2) * 4) as usize;
        assert_eq!(&face[middle..middle + 3], &[200, 30, 30], "the middle is not the picture");
        // The middle row reaches the edge: the circle is inscribed.
        assert!(alpha(0, out / 2) > 200, "the circle does not reach the edge");
        // Along the diagonal, from the corner in: transparent, then
        // something between, then opaque, never back.
        let diagonal: Vec<u8> = (0..out / 2).map(|k| alpha(k, k)).collect();
        assert_eq!(diagonal[0], 0, "{diagonal:?}");
        assert!(diagonal.iter().any(|a| *a > 0 && *a < 255), "the rim is not anti-aliased: {diagonal:?}");
        assert!(diagonal.windows(2).all(|p| p[0] <= p[1]), "the rim is not monotone: {diagonal:?}");
        // And round: half covered where a circle's rim crosses the
        // diagonal, r(1 - 1/sqrt 2) in from the corner, to a pixel.
        let half = diagonal.iter().position(|a| *a >= 128).unwrap() as f32;
        let r = out as f32 / 2.0;
        let rim = r - r / std::f32::consts::SQRT_2;
        assert!((half - rim).abs() <= 1.0, "half covered at {half}, the rim is at {rim}");
    }

    /// The same picture at the same size is the same handle, so the atlas
    /// keeps one texture; another size is another. Falsified by dropping
    /// the cache.
    #[test]
    fn a_face_is_made_once() {
        let picture = iced::widget::image::Handle::from_rgba(4, 4, vec![255u8; 64]);
        let a = super::round(&picture, 28.0);
        let b = super::round(&picture, 28.0);
        assert_eq!(a.id(), b.id());
        assert_ne!(a.id(), super::round(&picture, 116.0).id());
        let iced::widget::image::Handle::Rgba { width, height, .. } = a else {
            panic!("not pixels")
        };
        assert_eq!((width, height), (56, 56));
    }

    /// Anything already inside the bound is left exactly as it is — a
    /// resample there only loses detail. Falsified by always resizing.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_small_picture_is_left_alone() {
        let small = ::image::RgbImage::new(361, 295);
        let mut png = std::io::Cursor::new(Vec::new());
        ::image::DynamicImage::ImageRgb8(small)
            .write_to(&mut png, ::image::ImageFormat::Png)
            .expect("the fixture encodes");
        let out = super::imp::shrink(png.get_ref(), BOUND).expect("a PNG decodes");
        assert_eq!((out.width, out.height), (361, 295));
    }
}
