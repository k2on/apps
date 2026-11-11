//! The camera: a QR code read, a photo taken, or a photo taken while eyes
//! in Rust watch the frames. Each shows the platform's own camera page and
//! answers when it closes, through a typed reply the platform is handed: a
//! scan's [`ScanReply`] answers the code read (none when the person backed
//! out), a photo's [`PhotoReply`] its JPEG. An update or another module
//! asks with [`scan_qr`] (a [`ScanQr`], output `Option<String>`),
//! [`take_photo`] (a [`TakePhoto`], output `Option<Vec<u8>>`) or [`watch`]
//! (a [`Watch`], the same output).
//!
//! **Watched** ([`Camera::watch`]): the platform's camera page hands each
//! frame of its analysis stream to the [`Eyes`] it was given (a [`Frame`]:
//! its luminance, a small square of its colour, how it turns upright, when)
//! and draws what they answer (a [`Look`]: an outline where they see
//! something, a sentence, whether to take the picture now); the person's
//! own shutter takes it at once. ark-scanner's page finder is such eyes:
//! the camera knows nothing of pages.
//!
//! The phones' implementations are `PhoneCamera` (swift/: AVFoundation;
//! kotlin/: Camera2). README.md says what each does.

use std::sync::{Arc, Mutex};

use ark_module::{Modules, Op, Reply};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// The camera.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Camera: Send + Sync {
    /// A QR code read; what it says to `reply`.
    fn scan_qr(&self, reply: Arc<ScanReply>);
    /// A photo taken; its JPEG to `reply`.
    fn take_photo(&self, reply: Arc<PhotoReply>);
    /// A photo taken while `eyes` watch the frames: each analysis frame
    /// shown to them and what they answer drawn over the preview; the
    /// photo taken when they say so, or at the shutter, its JPEG to `reply`
    /// (none when the person backed out).
    fn watch(&self, eyes: Arc<Eyes>, reply: Arc<PhotoReply>);
}

/// One frame of the camera's analysis stream, as the sensor sees it.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// A byte a pixel, row by row, `width` wide.
    pub luma: Vec<u8>,
    /// The frame squeezed to [`SQUARE`] a side, RGB, row by row; empty
    /// when the platform has no colour to give.
    pub rgb: Vec<u8>,
    /// Quarter turns clockwise that stand the frame upright.
    pub turns: u8,
    /// When it was seen, in milliseconds of a clock that only goes forward.
    pub at_ms: i64,
}

/// The side of a [`Frame`]'s colour square.
pub const SQUARE: u32 = 256;

/// What the eyes make of a frame, for the camera's page to draw.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct Look {
    /// What they see, outlined: four corners clockwise from the top left,
    /// each x then y from 0 to 1 across the frame stood upright; empty for
    /// nothing.
    pub outline: Vec<f32>,
    /// The outline has held still: drawn fuller.
    pub held: bool,
    /// What the person is told: "Point the camera at the receipt".
    pub words: String,
    /// Take the picture now.
    pub take: bool,
}

/// What watches the frames: Rust's, handed to the platform.
pub trait See: Send + Sync {
    fn look(&self, frame: &Frame) -> Look;
}

/// Eyes the platform shows each frame to ([`Camera::watch`]).
#[cfg_attr(feature = "ffi", derive(uniffi::Object))]
pub struct Eyes(Arc<dyn See>);

impl Eyes {
    pub fn new(see: Arc<dyn See>) -> Arc<Eyes> {
        Arc::new(Eyes(see))
    }
}

#[cfg_attr(feature = "ffi", uniffi::export)]
impl Eyes {
    /// What they make of `frame`.
    pub fn look(&self, frame: Frame) -> Look {
        self.0.look(&frame)
    }
}

/// Where the platform says what a scan read, once.
#[cfg_attr(feature = "ffi", derive(uniffi::Object))]
pub struct ScanReply(Reply<Option<String>>);

#[cfg_attr(feature = "ffi", uniffi::export)]
impl ScanReply {
    /// What the code says; none when the person backed out.
    pub fn scanned(&self, code: Option<String>) {
        self.0.done(code)
    }
    /// The camera failed, in the platform's words.
    pub fn failed(&self, why: String) {
        self.0.failed(why)
    }
}

/// Where the platform hands over a photo, once.
#[cfg_attr(feature = "ffi", derive(uniffi::Object))]
pub struct PhotoReply(Reply<Option<Vec<u8>>>);

impl PhotoReply {
    /// A reply that hands the photo to `reply`: what another module that
    /// asks the camera (ark-scanner) makes of it.
    pub fn new(reply: Reply<Option<Vec<u8>>>) -> Arc<PhotoReply> {
        Arc::new(PhotoReply(reply))
    }
}

#[cfg_attr(feature = "ffi", uniffi::export)]
impl PhotoReply {
    /// The photo's JPEG; none when the person backed out.
    pub fn taken(&self, jpeg: Option<Vec<u8>>) {
        self.0.done(jpeg)
    }
    /// The camera failed, in the platform's words.
    pub fn failed(&self, why: String) {
        self.0.failed(why)
    }
}

/// A QR code scanned: what it says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanQr;

impl Op for ScanQr {
    type Module = dyn Camera;
    type Output = Option<String>;
    const MODULE: &'static str = "ark-camera";
    fn run(&self, m: &dyn Camera, reply: Reply<Option<String>>) {
        m.scan_qr(Arc::new(ScanReply(reply)))
    }
}

/// A photo taken: its JPEG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TakePhoto;

impl Op for TakePhoto {
    type Module = dyn Camera;
    type Output = Option<Vec<u8>>;
    const MODULE: &'static str = "ark-camera";
    fn run(&self, m: &dyn Camera, reply: Reply<Option<Vec<u8>>>) {
        m.take_photo(Arc::new(PhotoReply(reply)))
    }
}

/// A photo taken while eyes watch: its JPEG.
#[derive(Clone)]
pub struct Watch(pub Arc<Eyes>);

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Watch")
    }
}

impl PartialEq for Watch {
    fn eq(&self, o: &Watch) -> bool {
        Arc::ptr_eq(&self.0, &o.0)
    }
}

impl Op for Watch {
    type Module = dyn Camera;
    type Output = Option<Vec<u8>>;
    const MODULE: &'static str = "ark-camera";
    fn run(&self, m: &dyn Camera, reply: Reply<Option<Vec<u8>>>) {
        m.watch(self.0.clone(), PhotoReply::new(reply))
    }
}

/// A photo taken while `eyes` watch.
pub fn watch(eyes: Arc<Eyes>) -> Watch {
    Watch(eyes)
}

/// A QR code scanned.
pub fn scan_qr() -> ScanQr {
    ScanQr
}

/// A photo taken.
pub fn take_photo() -> TakePhoto {
    TakePhoto
}

/// A watch waiting: its eyes and where its photo goes.
type Watched = (Arc<Eyes>, Arc<PhotoReply>);

/// The camera, faked for tests: a scan or a photo waits until the test
/// answers it.
#[derive(Clone, Default)]
pub struct Fake {
    scans: Arc<Mutex<Vec<Arc<ScanReply>>>>,
    photos: Arc<Mutex<Vec<Arc<PhotoReply>>>>,
    watched: Arc<Mutex<Vec<Watched>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the camera.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn Camera>(Arc::new(self.clone()));
    }

    fn oldest<T>(of: &Mutex<Vec<T>>) -> Option<T> {
        let mut v = of.lock().unwrap_or_else(|e| e.into_inner());
        (!v.is_empty()).then(|| v.remove(0))
    }

    /// The oldest scan waiting, answered with `code`.
    pub fn scanned(&self, code: Option<String>) {
        if let Some(r) = Fake::oldest(&self.scans) {
            r.scanned(code)
        }
    }

    /// The oldest photo waiting, answered with `jpeg`.
    pub fn photographed(&self, jpeg: Option<Vec<u8>>) {
        if let Some(r) = Fake::oldest(&self.photos) {
            r.taken(jpeg)
        }
    }

    /// Whether a watched photo is waiting.
    pub fn watching(&self) -> bool {
        !self.watched.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }

    /// `frame` shown to the oldest watch's eyes: what they made of it, as
    /// the platform's page would draw it; none while nothing watches.
    pub fn show(&self, frame: Frame) -> Option<Look> {
        let eyes = self.watched.lock().unwrap_or_else(|e| e.into_inner()).first().map(|(e, _)| e.clone())?;
        Some(eyes.look(frame))
    }

    /// The oldest watch answered with `jpeg`: the picture taken (or none:
    /// backed out).
    pub fn watched(&self, jpeg: Option<Vec<u8>>) {
        if let Some((_, r)) = Fake::oldest(&self.watched) {
            r.taken(jpeg)
        }
    }
}

impl Camera for Fake {
    fn scan_qr(&self, reply: Arc<ScanReply>) {
        self.scans.lock().unwrap_or_else(|e| e.into_inner()).push(reply)
    }

    fn take_photo(&self, reply: Arc<PhotoReply>) {
        self.photos.lock().unwrap_or_else(|e| e.into_inner()).push(reply)
    }

    fn watch(&self, eyes: Arc<Eyes>, reply: Arc<PhotoReply>) {
        self.watched.lock().unwrap_or_else(|e| e.into_inner()).push((eyes, reply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_answers_later_and_once() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        m.run(&scan_qr(), move |a| g.lock().unwrap().push(a));
        assert!(got.lock().unwrap().is_empty());
        fake.scanned(Some("money://join?server=a".into()));
        fake.scanned(None);
        assert_eq!(*got.lock().unwrap(), vec![Ok(Some("money://join?server=a".to_string()))]);
    }

    /// The eyes see every frame shown while the camera watches, and the
    /// photo answers once. Falsified by answering the watch before a frame.
    #[test]
    fn a_watch_shows_its_frames_to_the_eyes() {
        struct Count(Mutex<u32>);
        impl See for Count {
            fn look(&self, f: &Frame) -> Look {
                let mut n = self.0.lock().unwrap();
                *n += 1;
                Look {
                    take: *n >= 2,
                    words: format!("{}x{}", f.width, f.height),
                    ..Look::default()
                }
            }
        }
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        assert_eq!(fake.show(Frame::default()), None, "nothing watches yet");
        m.run(&watch(Eyes::new(Arc::new(Count(Mutex::new(0))))), move |a| g.lock().unwrap().push(a));
        let frame = Frame {
            width: 4,
            height: 3,
            ..Frame::default()
        };
        assert!(!fake.show(frame.clone()).unwrap().take);
        let second = fake.show(frame).unwrap();
        assert!(second.take && second.words == "4x3");
        assert!(got.lock().unwrap().is_empty());
        fake.watched(Some(vec![1, 2]));
        assert!(!fake.watching());
        assert_eq!(*got.lock().unwrap(), vec![Ok(Some(vec![1, 2]))]);
    }

    #[test]
    fn a_photo_is_its_bytes() {
        let fake = Fake::new();
        let mut m = Modules::new();
        fake.register(&mut m);
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        m.run(&take_photo(), move |a| g.lock().unwrap().push(a));
        fake.photographed(Some(vec![0xff, 0xd8]));
        assert_eq!(*got.lock().unwrap(), vec![Ok(Some(vec![0xff, 0xd8]))]);
    }
}
