//! devices' first run: what it says of itself on the first-run pages every
//! Ark app shares (`ark_app::welcome`). A phone is usually signed in from
//! the laptop that enrolled it, over adb (a `devices://join?…` link the
//! shared first run takes), so these are mostly read by whoever set it up.

use ark_app::welcome::{IntroCopy, WelcomeCopy};

/// devices' words on its first run; `scan` where a camera can read another
/// device's code, `demo` where another server can be chosen.
pub fn copy(scan: bool, demo: bool) -> WelcomeCopy {
    let sentence = "This phone, held to your household's policy by your own server.";
    WelcomeCopy {
        name: "Devices".into(),
        glyph: "app".into(),
        sentence: sentence.into(),
        placeholder: "devices.example.com".into(),
        server_note: "Where your household's policies are kept.".into(),
        demo: None,
        scan,
        other_servers: demo,
        pages: vec![IntroCopy {
            glyph: "app".into(),
            title: "Welcome to Devices".into(),
            text: sentence.into(),
        }],
    }
}
