//! The phone's device modules, registered as the process's (its
//! [`ArkProcess::modules`]): every screen's runtime asks through them. An
//! app writes none of this. Its app.toml lists its modules, each module's
//! module.toml says how the phone makes its device and which of these
//! takes it (`[phone]`), and the generated entry points (ui/prebuild.py)
//! register them at launch, through the app library.

use std::sync::Arc;

use crate::ArkProcess;

#[uniffi::export]
impl ArkProcess {
    /// The phone's haptics (ark-haptics).
    pub fn register_haptics(&self, device: Arc<dyn ark_haptics::phone::Haptics>) {
        self.add_modules(|m| {
            m.set(Some(ark_haptics::phone::port(device)));
        })
    }

    /// The phone's clipboard (ark-clipboard).
    pub fn register_clipboard(&self, device: Arc<dyn ark_clipboard::phone::Clipboard>) {
        self.add_modules(|m| {
            m.set(Some(ark_clipboard::phone::port(device)));
        })
    }

    /// The phone's share sheet (ark-share).
    pub fn register_share(&self, device: Arc<dyn ark_share::phone::Share>) {
        self.add_modules(|m| {
            m.set(Some(ark_share::phone::port(device)));
        })
    }

    /// The phone's camera (ark-camera).
    pub fn register_camera(&self, device: Arc<dyn ark_camera::Camera>) {
        self.add_modules(|m| {
            m.set(Some(device));
        })
    }

    /// The page scanner (ark-scanner): its eyes and its warp are Rust's,
    /// its frames and shutter the camera's, so it is registered after the
    /// camera and over it; with no camera registered, none.
    pub fn register_scanner(&self) {
        self.add_modules(|m| {
            let scanner = m.get::<dyn ark_camera::Camera>().map(ark_scanner::over_camera);
            m.set(scanner);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_module::Reply;

    struct Shutter;

    impl ark_camera::Camera for Shutter {
        fn scan_qr(&self, reply: Arc<ark_camera::ScanReply>) {
            reply.scanned(None)
        }
        fn take_photo(&self, reply: Arc<ark_camera::PhotoReply>) {
            reply.taken(None)
        }
        fn watch(&self, _eyes: Arc<ark_camera::Eyes>, reply: Arc<ark_camera::PhotoReply>) {
            reply.taken(None)
        }
        fn pick_photo(&self, reply: Arc<ark_camera::PhotoReply>) {
            reply.taken(None)
        }
    }

    /// Each module registered is added to the process's, the others kept;
    /// the scanner is the camera's.
    #[test]
    fn devices_are_added_to_the_processs_modules() {
        let p = ArkProcess::default();
        p.register_scanner();
        assert!(p.modules().get::<dyn ark_module::scanner::Scanner>().is_none(), "no camera, no scanner");
        p.register_camera(Arc::new(Shutter));
        p.register_scanner();
        let m = p.modules();
        assert!(m.get::<dyn ark_camera::Camera>().is_some());
        let scanner = m.get::<dyn ark_module::scanner::Scanner>().expect("the scanner over the camera");
        let got = Arc::new(std::sync::Mutex::new(None));
        let g = got.clone();
        scanner.scan_page(Reply::new(move |a| *g.lock().unwrap() = Some(a.is_ok())));
        assert_eq!(*got.lock().unwrap(), Some(true), "the camera's shutter answered the scan");
    }
}
