//! The derived keys, on the host: what `add_song` computes for a track's
//! work, movement and recording, for a caller that needs the same key
//! outside an entry. Each runs the helper's own definition through the
//! interpreter ([`eval`], `Domain::ask`), so there is no second copy of the rule to drift
//! from it; the inputs are trimmed as `add_song` trims its arguments.

use std::sync::OnceLock;

use ark::author::{Domain, Node};
use ark::interp::V;
use ark::log::EntryId;
use ark::peer::Ctx;
use ark::store::Store;

/// A function of the module, by name, applied to `args` over an empty
/// store: a pure helper's answer, or its refusal.
pub fn eval(name: &str, args: Vec<V>) -> Result<Vec<u8>, ark::Refusal> {
    static D: OnceLock<Domain> = OnceLock::new();
    let d = D.get_or_init(crate::domain);
    let ctx = Ctx {
        id: EntryId::of("host", 0),
        actor: String::new(),
        session: String::new(),
        roles: vec![],
        now: 0,
    };
    d.ask(&Store::new(d.schema.clone()), &ctx, name, &V::Tup(args).to_bytes())
}

pub fn text(s: &str) -> V {
    V::Bytes(s.as_bytes().to_vec())
}

fn answer(name: &str, args: Vec<V>) -> String {
    match eval(name, args).and_then(|b| String::from_bytes(&b)) {
        Ok(t) => t,
        Err(e) => unreachable!("a key helper returns text and never refuses: {e}"),
    }
}

/// `library::work_key`: a work's key, from its composer, catalogue number
/// and title.
pub fn work_key(composer: &str, catalogue: &str, title: &str) -> String {
    answer(
        "library.work_key",
        vec![text(composer.trim()), text(catalogue.trim()), text(title.trim())],
    )
}

/// `library::movement_key`: a movement's key, from its work's and its number.
pub fn movement_key(work_id: &str, no: i64) -> String {
    answer("library.movement_key", vec![text(work_id), V::Int(no)])
}

/// `library::recording_key`: a recording's key, from what it is of and who
/// played.
pub fn recording_key(of: &str, who: &str) -> String {
    answer("library.recording_key", vec![text(of), text(who.trim())])
}

/// `library::key_part`: one name as it stands in a key.
pub fn key_part(name: &str) -> String {
    answer("library.key_part", vec![text(name.trim())])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys as v4 made them, where a name has no run of separators.
    /// Falsified by dropping `trim` from `slug`: " Goldberg Variations "
    /// keyed with a leading dash.
    #[test]
    fn keys_are_names_every_peer_agrees_on() {
        assert_eq!(
            work_key("Johann Sebastian Bach", "BWV 988", "Goldberg Variations"),
            "johann-sebastian-bach/bwv-988"
        );
        assert_eq!(
            work_key("Johann Sebastian Bach", "", " Goldberg Variations "),
            "johann-sebastian-bach/goldberg-variations"
        );
        let bang = key_part("!!!");
        assert!(bang.starts_with('x') && bang.len() > 1, "{bang}");
        assert_ne!(bang, key_part("???"), "two names of no letters are two keys");
        assert_eq!(key_part("Kimiko Ishizaka"), "kimiko-ishizaka");
        assert_eq!(movement_key("johann-sebastian-bach/bwv-988", 3), "johann-sebastian-bach/bwv-988#3");
        assert_eq!(
            recording_key("johann-sebastian-bach/bwv-988", "Kimiko Ishizaka"),
            "johann-sebastian-bach/bwv-988@kimiko-ishizaka"
        );
    }

    /// v4's keys for names with a run of separators: one dash per run.
    /// Waits for a standard function that sees a character beside another
    /// (a fold, or the alphanumeric runs of a text): v5's `slug` maps each
    /// character alone, and makes "a  b" `a--b`.
    #[test]
    #[ignore = "waits for a std function that sees neighbouring characters; v5's slug does not collapse a run"]
    fn a_run_of_separators_is_one_dash() {
        assert_eq!(
            key_part("Dvořák: Symphony No. 9, \"From the New World\"!"),
            "dvořák-symphony-no-9-from-the-new-world"
        );
        assert_eq!(key_part("  --a  b--  "), "a-b", "no dash leads, trails or doubles");
    }

    /// The name v4's rule gives: the name, or when it is taken the first
    /// "name (n)" free.
    fn playlist_name_was(names: &[&str], name: &str) -> String {
        if !names.contains(&name) {
            return name.into();
        }
        (1..).map(|n| format!("{name} ({n})")).find(|c| !names.contains(&c.as_str())).unwrap()
    }

    fn playlist_name(names: &[&str], name: &str) -> String {
        let names = V::List(names.iter().map(|n| text(n)).collect());
        answer("playlists.playlist_name", vec![names, text(name)])
    }

    /// `playlist_name` answers v4's rule, for a name that is free, one that
    /// is taken, and one taken with its first numbers taken too. Falsified
    /// by numbering the name when it is free: "Favorites" among no
    /// playlists came back "Favorites (1)".
    #[test]
    fn playlist_name_is_what_it_was() {
        let sets: [&[&str]; 4] = [
            &[],
            &["Favorites"],
            &["Favorites", "Favorites (1)", "Favorites (3)", "Evening"],
            &["Evening", "Morning"],
        ];
        for names in sets {
            for name in ["Favorites", "Evening", "Night"] {
                assert_eq!(playlist_name(names, name), playlist_name_was(names, name), "{name:?} among {names:?}");
            }
        }
    }

    /// Over a hundred numbered siblings with gaps, the first free number is
    /// found among the ranks (61). Falsified by answering `len + 1` always:
    /// it came back "Favorites (102)".
    #[test]
    fn playlist_name_over_many_siblings() {
        let mut all: Vec<String> = vec!["Favorites".into()];
        all.extend((1..=60).map(|n| format!("Favorites ({n})")));
        all.extend((0..40).map(|i| format!("Favorites ({})", 62 + 2 * i)));
        let names: Vec<&str> = all.iter().map(String::as_str).collect();
        assert_eq!(playlist_name(&names, "Favorites"), "Favorites (61)");
        assert_eq!(playlist_name(&names, "Favorites"), playlist_name_was(&names, "Favorites"));
    }
}
