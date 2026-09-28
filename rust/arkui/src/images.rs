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

    /// The same, with another bound.
    pub fn with_bound(mut self, bound: u32) -> Self {
        self.bound = bound;
        self
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
