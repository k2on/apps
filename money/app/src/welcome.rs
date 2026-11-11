//! money's first run: what it says of itself on every client's shared
//! first-run pages (`ark_app::welcome`, drawn by every toolkit, walked by
//! the sign-in machine every Ark app shares). Only the words and the mark
//! are money's.

use ark_app::welcome::{IntroCopy, WelcomeCopy};

/// money's words on its first run; `scan` where a camera can read another
/// device's code, `demo` where another server can be chosen (not in the
/// browser a server served, which is that server's).
pub fn copy(scan: bool, demo: bool) -> WelcomeCopy {
    let sentence = "Your household's budget, every dollar given a job, on your own server.";
    WelcomeCopy {
        name: "Money".into(),
        glyph: "wallet".into(),
        sentence: sentence.into(),
        placeholder: "money.example.com".into(),
        server_note: "Where your household's budget is kept.".into(),
        // The demo is a launch's (`-demo`, `?demo`), never the first run's.
        demo: None,
        scan,
        other_servers: demo,
        // Its one page, Welcome to Money, where the first run has no
        // server's page of its own to say it (the desktop's and the
        // browser's server is theirs): the phones' first page is the
        // server's, which says the same over its field.
        pages: vec![IntroCopy {
            glyph: "wallet".into(),
            title: "Welcome to Money".into(),
            text: sentence.into(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Title Case on every page's title; the camera only where asked, and
    /// the demo never on the first run (it is a launch's). Falsified by
    /// Try the Demo on the server's page.
    #[test]
    fn moneys_words() {
        let c = copy(true, false);
        assert!(c.demo.is_none() && c.scan);
        assert!(copy(false, true).demo.is_none());
        assert_eq!(c.pages.len(), 1);
        assert_eq!((c.pages[0].title.as_str(), c.pages[0].glyph.as_str()), ("Welcome to Money", "wallet"));
        for p in &c.pages {
            assert!(
                p.title
                    .split(' ')
                    .filter(|w| !["a", "on", "the", "of", "to"].contains(w))
                    .all(|w| w.starts_with(char::is_uppercase)),
                "{}",
                p.title
            );
        }
    }
}
