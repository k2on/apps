//! The scanner's port: a page (a receipt, a letter) photographed and made
//! flat, as a JPEG, or a photo of one the person already has. An update
//! asks with [`scan_page`], a [`ScanPage`] whose output is the flat page's
//! JPEG, or [`pick_photo`], a [`PickPhoto`] whose output is the photo
//! chosen as a JPEG; either is none when the person backed out. Its
//! implementations — the camera's live page with the eyes in Rust and the
//! system's photo picker on the phones, a file chosen on the desktop and
//! in the browser — and its fake are the module's (modules/ark-scanner,
//! `ark-scanner`).

use crate::{Op, Reply};

/// A page photographed and made flat.
pub trait Scanner: Send + Sync {
    /// The page's JPEG to `reply`, once: none when the person backed out.
    fn scan_page(&self, reply: Reply<Option<Vec<u8>>>);
    /// A photo chosen from the device's library (the phones' photo picker,
    /// a file elsewhere), as a JPEG, to `reply`, once: none when the
    /// person backed out.
    fn pick_photo(&self, reply: Reply<Option<Vec<u8>>>);
}

/// A page scanned: its JPEG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanPage;

impl Op for ScanPage {
    type Module = dyn Scanner;
    type Output = Option<Vec<u8>>;
    const MODULE: &'static str = "ark-scanner";
    fn run(&self, m: &dyn Scanner, reply: Reply<Option<Vec<u8>>>) {
        m.scan_page(reply)
    }
}

/// A page scanned.
pub fn scan_page() -> ScanPage {
    ScanPage
}

/// A photo chosen from the library: its JPEG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickPhoto;

impl Op for PickPhoto {
    type Module = dyn Scanner;
    type Output = Option<Vec<u8>>;
    const MODULE: &'static str = "ark-scanner";
    fn run(&self, m: &dyn Scanner, reply: Reply<Option<Vec<u8>>>) {
        m.pick_photo(reply)
    }
}

/// A photo chosen from the library.
pub fn pick_photo() -> PickPhoto {
    PickPhoto
}
