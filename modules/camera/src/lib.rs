//! The camera: a QR code read, a photo taken. Each shows the platform's
//! own camera page and answers when it closes, through a [`ModuleReply`]: a scan
//! as `Text` (none when the person backed out), a photo as its JPEG's
//! `Bytes`. An update asks with [`scan_qr`] or [`take_photo`].
//!
//! No native implementation yet (README.md): money's join sheet keeps its
//! hand-written `QrScanner` until a screen that scans moves to Rust.

use std::sync::{Arc, Mutex};

use ark_module::{Call, ModuleAnswer, ModuleCall, ModuleReply, Modules};

#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// The camera.
#[cfg_attr(feature = "ffi", uniffi::export(with_foreign))]
pub trait Camera: Send + Sync {
    /// Answers `Text`.
    fn scan_qr(&self, reply: Arc<ModuleReply>);
    /// Answers `Bytes` (a JPEG).
    fn take_photo(&self, reply: Arc<ModuleReply>);
}

/// What an update asks of the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraCall {
    ScanQr,
    TakePhoto,
}

impl Call for CameraCall {
    type Module = dyn Camera;
    const MODULE: &'static str = "camera";
    fn run(&self, m: &dyn Camera, reply: Arc<ModuleReply>) {
        match self {
            CameraCall::ScanQr => m.scan_qr(reply),
            CameraCall::TakePhoto => m.take_photo(reply),
        }
    }
}

/// A QR code scanned.
pub fn scan_qr() -> ModuleCall {
    CameraCall::ScanQr.into()
}

/// A photo taken.
pub fn take_photo() -> ModuleCall {
    CameraCall::TakePhoto.into()
}

/// The camera, faked for tests: a scan or a photo waits until the test
/// answers it.
#[derive(Clone, Default)]
pub struct Fake {
    scans: Arc<Mutex<Vec<Arc<ModuleReply>>>>,
    photos: Arc<Mutex<Vec<Arc<ModuleReply>>>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// This fake registered as the camera.
    pub fn register(&self, modules: &mut Modules) {
        modules.register::<dyn Camera>(Arc::new(self.clone()));
    }

    fn oldest(of: &Mutex<Vec<Arc<ModuleReply>>>) -> Option<Arc<ModuleReply>> {
        let mut v = of.lock().unwrap_or_else(|e| e.into_inner());
        (!v.is_empty()).then(|| v.remove(0))
    }

    /// The oldest scan waiting, answered with `code`.
    pub fn scanned(&self, code: Option<String>) {
        if let Some(r) = Fake::oldest(&self.scans) {
            r.answer(ModuleAnswer::Text { value: code })
        }
    }

    /// The oldest photo waiting, answered with `jpeg`.
    pub fn photographed(&self, jpeg: Option<Vec<u8>>) {
        if let Some(r) = Fake::oldest(&self.photos) {
            r.answer(ModuleAnswer::Bytes { value: jpeg })
        }
    }
}

impl Camera for Fake {
    fn scan_qr(&self, reply: Arc<ModuleReply>) {
        self.scans.lock().unwrap_or_else(|e| e.into_inner()).push(reply)
    }

    fn take_photo(&self, reply: Arc<ModuleReply>) {
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
        m.call(scan_qr(), move |a| g.lock().unwrap().push(a));
        assert!(got.lock().unwrap().is_empty());
        fake.scanned(Some("money://join?server=a".into()));
        fake.scanned(None);
        assert_eq!(
            *got.lock().unwrap(),
            vec![ModuleAnswer::Text {
                value: Some("money://join?server=a".into())
            }]
        );
    }
}
