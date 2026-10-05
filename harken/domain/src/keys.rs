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
        Ok(Value::Text(t)) => t.into_string(),
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
        assert_eq!(key_part("Kimiko Ishizaka"), "kimiko-ishizaka");
        assert_eq!(movement_key("johann-sebastian-bach/bwv-988", 3), "johann-sebastian-bach/bwv-988#3");
        assert_eq!(
            recording_key("johann-sebastian-bach/bwv-988", "Kimiko Ishizaka"),
            "johann-sebastian-bach/bwv-988@kimiko-ishizaka"
        );
    }

    // The bodies `slug`, `key_part` and `playlist_name` had before
    // `docs/plan-perf.md` R1, kept here as the reference the new ones are
    // held to: a key is permanent, so a faster body must be the same
    // function.
    fn slug_before(text: Text) -> Text {
        use ark::authoring::{concat, helper, list, pick};
        helper("slug_before", ("text", text), |text: Text| {
            concat(text.chars().map(|x| pick(x.is_alnum(), x.lower(), " ")))
                .trim()
                .chars()
                .fold("", |acc: Text, x| {
                    pick(
                        x.eq(" ").and(acc.chars().last().map_or(false, |x_2| x_2.eq("-"))),
                        acc,
                        concat(list([acc, pick(x.eq(" "), "-", x)])),
                    )
                })
        })
    }

    fn key_part_before(text: Text) -> Text {
        use ark::authoring::{concat, helper, list, pick};
        helper("key_part_before", ("text", text), |text: Text| {
            pick(
                slug_before(text).is_empty(),
                concat(list(["x".into(), text.trim().fnv1a64().to_text()])),
                slug_before(text),
            )
        })
    }

    fn playlist_name_before(names: ark::authoring::List<Text>, name: Text) -> Text {
        use ark::authoring::{helper, pick, List};
        helper(
            "playlist_name_before",
            (("names", names), ("name", name)),
            |names: List<Text>, name: Text| {
                pick(
                    names.contains(name),
                    crate::playlists::numbered(name, crate::playlists::free_number(names, name)),
                    name,
                )
            },
        )
    }

    fn native(v: Result<Value, ark::eval::EvalFault>) -> Value {
        v.unwrap_or_else(|e| panic!("{e:?}"))
    }

    /// `slug` and `key_part` answer what their bodies before R1 answered,
    /// on names with every shape a run can take: leading, trailing and
    /// doubled separators, nothing but separators, the empty name, letters
    /// outside ASCII (one that lowercases to two characters), digits, and
    /// separators that are not spaces. Falsified by starting a run only
    /// when the text so far is empty (dropping `acc.starts_with(" ")`):
    /// "a  b" becomes "a--b".
    #[test]
    fn slug_is_what_it_was() {
        let names = [
            "",
            " ",
            "!!!",
            "???",
            "a",
            "a b",
            "a  b",
            "  --a  b--  ",
            "Dvořák: Symphony No. 9, \"From the New World\"!",
            "İstanbul",
            "ΑΒΓ δ",
            "BWV 988",
            "Op. 23 — No. 4",
            "tab\tand\nnewline",
            "x-y_z.w",
            "999",
            "日本 の 音楽",
            "a!b?c",
            "-",
            "--a",
            "a--",
        ];
        for n in names {
            let (was, is) = (
                native(ark::authoring::evaluate(|| slug_before(Text::from(n)))),
                native(ark::authoring::evaluate(|| crate::library::slug(Text::from(n)))),
            );
            assert_eq!(is, was, "slug({n:?})");
            let (was, is) = (
                native(ark::authoring::evaluate(|| key_part_before(Text::from(n)))),
                native(ark::authoring::evaluate(|| crate::library::key_part(Text::from(n)))),
            );
            assert_eq!(is, was, "key_part({n:?})");
        }
    }

    /// `playlist_name` answers what its `pick` did, for a name that is free,
    /// one that is taken, and one taken with its first numbers taken too.
    /// Falsified by numbering the name when it is free (`.not()` in the
    /// filter): "Favorites" among no playlists comes back "Favorites (1)".
    #[test]
    fn playlist_name_is_what_it_was() {
        use ark::authoring::{list, List};
        let sets: [&[&str]; 4] = [
            &[],
            &["Favorites"],
            &["Favorites", "Favorites (1)", "Favorites (3)", "Evening"],
            &["Evening", "Morning"],
        ];
        for names in sets {
            for name in ["Favorites", "Evening", "Night"] {
                let of = || -> List<Text> { list(names.iter().map(|n| Text::from(*n)).collect::<Vec<_>>()) };
                let was = native(ark::authoring::evaluate(|| playlist_name_before(of(), Text::from(name))));
                let is = native(ark::authoring::evaluate(|| crate::playlists::playlist_name(of(), Text::from(name))));
                assert_eq!(is, was, "{name:?} among {names:?}");
            }
        }
    }

    /// `create_playlist` hands `playlist_name` only the names from `name`
    /// up to `name )` of the person's (`docs/plan-perf.md` R6), and that is
    /// the same answer as every name of theirs: over a thousand playlists
    /// of which a hundred are "Favorites (n)" — numbered with gaps, some
    /// numbered twice over, beside names that sort just outside the range
    /// ("Favorite", "Favorites )", "Favorites!", "Favoritesz") and just
    /// inside it ("Favorites (x)", "Favorites  two") — for the name, for
    /// names that are free, for a numbered sibling asked for itself, and
    /// with the plain name taken or not. Which rows the store examines for
    /// that range is `tests/perf.rs`'s to count. Falsified by an upper
    /// bound of `name (` (the siblings left out): "Favorites" comes back
    /// "Favorites (1)", which the person already has.
    #[test]
    fn playlist_name_over_the_siblings_is_over_every_name() {
        use ark::authoring::{list, List};
        let mut all: Vec<String> = vec!["Favorites".into()];
        // A hundred numbered siblings: 1..=60, then every other number to
        // 138, so the first free number is 61.
        all.extend((1..=60).map(|n| format!("Favorites ({n})")));
        all.extend((0..40).map(|i| format!("Favorites ({})", 62 + 2 * i)));
        all.extend(
            [
                "Favorite",
                "Favorites )",
                "Favorites!",
                "Favoritesz",
                "Favorites (x)",
                "Favorites  two",
                "Favorites (61) b",
            ]
            .map(String::from),
        );
        all.extend((all.len()..1000).map(|i| format!("List {i}")));
        assert_eq!(all.len(), 1000);
        let siblings = |of: &[String], name: &str| -> Vec<String> {
            let hi = format!("{name} )");
            let mut s: Vec<String> = of.iter().filter(|n| n.as_str() >= name && **n < hi).cloned().collect();
            s.sort();
            s
        };
        let without_plain: Vec<String> = all.iter().filter(|n| *n != "Favorites").cloned().collect();
        for of in [&all, &without_plain] {
            for name in ["Favorites", "Favorites (3)", "Favorite", "Night", "List 500", "Favorites (x)"] {
                let names = |xs: &[String]| -> List<Text> { list(xs.iter().map(|n| Text::from(n.as_str())).collect::<Vec<_>>()) };
                let near = siblings(of, name);
                assert!(near.len() <= 110, "{name}: {} names", near.len());
                let was = native(ark::authoring::evaluate(|| playlist_name_before(names(of), Text::from(name))));
                let is = native(ark::authoring::evaluate(|| {
                    crate::playlists::playlist_name(names(&near), Text::from(name))
                }));
                assert_eq!(is, was, "{name:?}, plain name taken: {}", of.len() == 1000);
            }
        }
        let taken = native(ark::authoring::evaluate(|| {
            playlist_name_before(
                list(all.iter().map(|n| Text::from(n.as_str())).collect::<Vec<_>>()),
                Text::from("Favorites"),
            )
        }));
        assert_eq!(taken, Value::text("Favorites (61)"));
        assert_eq!(siblings(&all, "Favorites").len(), 1 + 100 + 3);
    }
}
