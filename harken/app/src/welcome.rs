//! harken's first run: what it says of itself on the first-run pages every
//! Ark app shares (`ark_app::welcome`, over the sign-in machine,
//! `ark_auth::first_run`). harken has no demo household on a phone and
//! asks for no camera yet; its pages of what it is are none.

use ark_app::welcome::WelcomeCopy;

/// harken's words on its first run: `scan` where a camera reads another
/// device's code; `demo` is every app's to be asked, and harken has none.
pub fn copy(scan: bool, _demo: bool) -> WelcomeCopy {
    WelcomeCopy {
        name: "Harken".into(),
        glyph: "music".into(),
        sentence: "Harken plays from your own server. Where is it?".into(),
        placeholder: "music.example.com".into(),
        server_note: "A name or an address, with a port if it has one.".into(),
        demo: None,
        scan,
        other_servers: true,
        pages: vec![],
    }
}
