//! harken's first run: what it says of itself on every client's shared
//! first-run pages (`ark_app::welcome`, drawn by every toolkit, walked by
//! the sign-in machine every Ark app shares). Only the words and the mark
//! are harken's: its mark is `app`, its own icon's (icon.svg).

use ark_app::welcome::{IntroCopy, WelcomeCopy};

/// harken's words on its first run; `scan` where a camera can read another
/// device's code, `demo` where another server can be chosen (not in the
/// browser a server served, which is that server's).
pub fn copy(scan: bool, demo: bool) -> WelcomeCopy {
    let sentence = "Your music, podcasts and sermons, from your own server.";
    WelcomeCopy {
        name: "Harken".into(),
        glyph: "app".into(),
        sentence: sentence.into(),
        placeholder: "music.example.com".into(),
        server_note: "A name or an address, with a port if it has one.".into(),
        // The demo is a launch's (`-demo`, the page published as the demo), never the first run's.
        demo: None,
        scan,
        other_servers: demo,
        // Its one page, Welcome to Harken, as money's Welcome to Money.
        pages: vec![IntroCopy {
            glyph: "app".into(),
            title: "Welcome to Harken".into(),
            text: sentence.into(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One page of what harken is, in Title Case; the camera only where
    /// asked, and the demo never on the first run. Falsified by a first run
    /// that opens on the server's page (the walks' `welcome.next` finds
    /// nothing).
    #[test]
    fn harkens_words() {
        let c = copy(true, false);
        assert!(c.demo.is_none() && c.scan);
        assert_eq!(c.pages.len(), 1);
        assert_eq!(c.pages[0].title, "Welcome to Harken");
    }
}
