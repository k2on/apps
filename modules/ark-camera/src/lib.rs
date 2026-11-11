//! The camera: a QR code read, a photo taken. Each shows the platform's
//! own camera page and answers when it closes, through a typed reply the
//! platform is handed: a scan's [`ScanReply`] answers the code read (none
//! when the person backed out), a photo's [`PhotoReply`] its JPEG. An
//! update asks with [`scan_qr`] (a [`ScanQr`], output `Option<String>`) or
//! [`take_photo`] (a [`TakePhoto`], output `Option<Vec<u8>>`).
//!
//! No native implementation yet (README.md): money's join sheet keeps its
//! hand-written `QrScanner` until a screen that scans moves to Rust.

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

/// A QR code scanned.
pub fn scan_qr() -> ScanQr {
    ScanQr
}

/// A photo taken.
pub fn take_photo() -> TakePhoto {
    TakePhoto
}

/// The camera, faked for tests: a scan or a photo waits until the test
/// answers it.
#[derive(Clone, Default)]
pub struct Fake {
    scans: Arc<Mutex<Vec<Arc<ScanReply>>>>,
    photos: Arc<Mutex<Vec<Arc<PhotoReply>>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the camera.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn Camera>(Arc::new(self.clone()));
    }

    fn oldest<T>(of: &Mutex<Vec<Arc<T>>>) -> Option<Arc<T>> {
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
}

impl Camera for Fake {
    fn scan_qr(&self, reply: Arc<ScanReply>) {
        self.scans.lock().unwrap_or_else(|e| e.into_inner()).push(reply)
    }

    fn take_photo(&self, reply: Arc<PhotoReply>) {
        self.photos.lock().unwrap_or_else(|e| e.into_inner()).push(reply)
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
