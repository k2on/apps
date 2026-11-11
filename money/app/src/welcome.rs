//! money's first run: what it says of itself on every client's shared
//! first-run pages (`ark_app::welcome`, drawn by every toolkit, walked by
//! the sign-in machine every Ark app shares). Only the words and the mark
//! are money's.

use ark_app::welcome::{IntroCopy, WelcomeCopy};

/// money's words on its first run; `scan` where a camera can read another
/// device's code, `demo` where the demo household can run and another
/// server be chosen (not in the browser a server served, which is that
/// server's).
pub fn copy(scan: bool, demo: bool) -> WelcomeCopy {
    let page = |glyph: &str, title: &str, text: &str| IntroCopy {
        glyph: glyph.into(),
        title: title.into(),
        text: text.into(),
    };
    WelcomeCopy {
        name: "Money".into(),
        glyph: "wallet".into(),
        sentence: "Your household's budget, every dollar given a job, on your own server.".into(),
        placeholder: "money.example.com".into(),
        server_note: "Where your household's budget is kept.".into(),
        demo: demo.then(|| "A household of two with three months of spending, on this device alone.".into()),
        scan,
        other_servers: demo,
        pages: vec![
            page(
                "target",
                "Every Dollar a Job",
                "Money that comes in is ready to assign. Give it to a category, and spend from what each one has.",
            ),
            page(
                "users",
                "One Budget, Shared",
                "Everyone in the household signs in to the same budget, and sees what the others spend as they spend it.",
            ),
            page(
                "shield-check",
                "Kept on Your Server",
                "Your budget lives on this device and on your own server. It opens at once, and works offline.",
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Title Case on every page's title; the demo and the camera only
    /// where asked.
    #[test]
    fn moneys_words() {
        let c = copy(true, false);
        assert!(c.demo.is_none() && c.scan);
        assert!(copy(false, true).demo.is_some());
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
