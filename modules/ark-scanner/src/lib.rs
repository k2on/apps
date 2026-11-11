//! The scanner: a page (a receipt, a letter) photographed and made flat, as
//! a JPEG. Its port — the [`Scanner`] trait and [`scan_page`], a
//! [`ScanPage`] whose output is `Option<Vec<u8>>` — is the runtime's
//! (`ark_module::scanner`), which an update asks through; here is what
//! implements it:
//!
//! - The phones: [`over_camera`], the scanner over ark-camera's port. The
//!   camera's live page ([`ark_camera::Camera::watch`]) shows each frame to
//!   the scanner's eyes ([`eyes::PageEyes`], all in Rust: the page found,
//!   held still and sharp, the picture taken), and the photo it hands back
//!   is made flat here ([`page::flat`]). No camera of its own: the frames
//!   and the shutter are the camera module's, its Swift and Kotlin.
//! - The desktop (feature `desktop`): [`Chosen`], a file the window asks
//!   the person to choose (an iced window's own chooser, `arkui::files`).
//! - The browser (feature `web`, wasm32): [`Browser`], the page's own file
//!   input.
//! - Tests: [`fake::Fake`].
//!
//! The page is found by DocAligner's corner model on tract where it is
//! built in (feature `model`, [`model`]), and by the frame's largest bright
//! region otherwise ([`vision`]), which the model's answer replaces.

use std::sync::Arc;

pub use ark_module::scanner::*;
use ark_module::Reply;

/// Where a scan's page goes, once.
pub type PageReply = Reply<Option<Vec<u8>>>;

pub mod eyes;
pub mod fake;
#[cfg(feature = "model")]
pub mod model;
pub mod page;
pub mod vision;
pub mod warp;

/// The scanner over the camera: each scan the camera's live page, watched
/// by eyes of its own, and the photo made flat.
pub struct OverCamera(Arc<dyn ark_camera::Camera>);

/// The scanner over `camera`, as the port an update asks through.
pub fn over_camera(camera: Arc<dyn ark_camera::Camera>) -> Arc<dyn Scanner> {
    Arc::new(OverCamera(camera))
}

impl Scanner for OverCamera {
    fn scan_page(&self, reply: Reply<Option<Vec<u8>>>) {
        let eyes = Arc::new(eyes::PageEyes::new());
        let watching = eyes.clone();
        let photo = Reply::new(move |answer: ark_module::Answer<Option<Vec<u8>>>| match answer {
            Ok(Some(jpeg)) => {
                let seen = watching.seen();
                let turns = seen.map_or(0, |s| s.turns);
                match page::flat(&jpeg, seen, turns) {
                    Some(flat) => reply.done(Some(flat)),
                    None => reply.failed("The picture couldn't be made flat"),
                }
            }
            other => reply.answer(other),
        });
        self.0.watch(ark_camera::Eyes::new(eyes), ark_camera::PhotoReply::new(photo))
    }
}

#[cfg(feature = "desktop")]
pub use desktop::Chosen;

#[cfg(feature = "desktop")]
mod desktop {
    use std::sync::{Arc, Mutex};

    use ark_module::Reply;

    /// Scans a window answers with a file the person chooses: a call cannot
    /// open the window's chooser itself, so the host takes what is waiting
    /// after the message ([`Chosen::waiting`]), opens its chooser
    /// (`arkui::files::choose`), and answers with the file's bytes, or
    /// none.
    #[derive(Clone, Default)]
    pub struct Chosen {
        waiting: Arc<Mutex<Vec<crate::PageReply>>>,
    }

    impl Chosen {
        pub fn new() -> Chosen {
            Chosen::default()
        }

        /// Whether a scan waits for a file to be chosen.
        pub fn waiting(&self) -> bool {
            !self.waiting.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
        }

        /// The oldest scan waiting, answered with the file chosen (none:
        /// the chooser was cancelled).
        pub fn chosen(&self, bytes: Option<Vec<u8>>) {
            let mut w = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
            if !w.is_empty() {
                w.remove(0).done(bytes)
            }
        }
    }

    impl super::Scanner for Chosen {
        fn scan_page(&self, reply: Reply<Option<Vec<u8>>>) {
            self.waiting.lock().unwrap_or_else(|e| e.into_inner()).push(reply)
        }
    }
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub use web::Browser;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
mod web {
    use ark_module::Reply;
    use wasm_bindgen::{closure::Closure, JsCast};

    /// The page's own file input (`<input type=file accept=image/*
    /// capture=environment>`): a phone's browser offers its camera, a
    /// computer's its files; the file's bytes are the page.
    #[derive(Clone, Copy, Default)]
    pub struct Browser;

    impl super::Scanner for Browser {
        fn scan_page(&self, reply: Reply<Option<Vec<u8>>>) {
            let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
                return reply.failed("There is no page to choose a file on");
            };
            let Ok(input) = doc.create_element("input").map(|e| e.unchecked_into::<web_sys::HtmlInputElement>()) else {
                return reply.failed("The file input couldn't be made");
            };
            input.set_type("file");
            input.set_accept("image/*");
            let _ = input.set_attribute("capture", "environment");
            let picked = input.clone();
            let on_change = Closure::once_into_js(move || {
                let Some(file) = picked.files().and_then(|f| f.get(0)) else {
                    return reply.done(None);
                };
                wasm_bindgen_futures::spawn_local(async move {
                    match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                        Ok(buf) => reply.done(Some(js_sys::Uint8Array::new(&buf).to_vec())),
                        Err(_) => reply.failed("The file couldn't be read"),
                    }
                });
            });
            input.set_onchange(Some(on_change.unchecked_ref()));
            input.click();
        }
    }
}
