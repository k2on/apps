//! The derived keys, on the host: what `add_song` computes for a track's
//! work, movement and recording, for a caller that needs the same key
//! outside an entry — the demo's seed describing a recording it just
//! authored, a page opened from a link. Each runs the helper's own
//! definition ([`ark::authoring::evaluate`]), so there is no second copy of
//! the rule to drift from it; the inputs are trimmed as `add_song` trims
//! its arguments.

use ark::authoring::{evaluate, Text};
use ark::value::Value;

use crate::library;

fn text(v: Result<Value, ark::eval::EvalFault>) -> String {
    match v {
        Ok(Value::Text(t)) => t,
        other => unreachable!("a key helper returns text and never refuses: {other:?}"),
    }
}

/// `library::work_key`: a work's key, from its composer, catalogue number
/// and title.
pub fn work_key(composer: &str, catalogue: &str, title: &str) -> String {
    text(evaluate(|| {
        library::work_key(Text::from(composer.trim()), Text::from(catalogue.trim()), Text::from(title.trim()))
    }))
}

/// `library::movement_key`: a movement's key, from its work's and its number.
pub fn movement_key(work_id: &str, no: i64) -> String {
    text(evaluate(|| library::movement_key(Text::from(work_id), no.into())))
}

/// `library::recording_key`: a recording's key, from what it is of (a
/// work's key, or `album/title` keys for a track of no work) and who played.
pub fn recording_key(of: &str, who: &str) -> String {
    text(evaluate(|| library::recording_key(Text::from(of), Text::from(who.trim()))))
}

/// `library::key_part`: one name as it stands in a key.
pub fn key_part(name: &str) -> String {
    text(evaluate(|| library::key_part(Text::from(name.trim()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_names_every_peer_agrees_on() {
        assert_eq!(
            work_key("Johann Sebastian Bach", "BWV 988", "Goldberg Variations"),
            "johann-sebastian-bach/bwv-988"
        );
        assert_eq!(
            work_key("Johann Sebastian Bach", "", " Goldberg Variations "),
            "johann-sebastian-bach/goldberg-variations",
            "the title where there is no catalogue number"
        );
        assert_eq!(
            key_part("Dvořák: Symphony No. 9, \"From the New World\"!"),
            "dvořák-symphony-no-9-from-the-new-world"
        );
        assert_eq!(key_part("  --a  b--  "), "a-b", "no dash leads, trails or doubles");
        let bang = key_part("!!!");
        assert!(bang.starts_with('x') && bang.len() > 1, "{bang}");
        assert_ne!(bang, key_part("???"), "two names of no letters are two keys");
        assert_eq!(movement_key("johann-sebastian-bach/bwv-988", 3), "johann-sebastian-bach/bwv-988#3");
        assert_eq!(
            recording_key("johann-sebastian-bach/bwv-988", "Kimiko Ishizaka"),
            "johann-sebastian-bach/bwv-988@kimiko-ishizaka"
        );
    }
}
