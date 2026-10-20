//! What a host knows beyond the bindings (`harken/domain/host.rs`): the
//! keys, each the domain's own helper asked, a play's threshold and a
//! file's URL.

use harken_domain::domain::host::*;

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
/// (a fold, or the alphanumeric runs of a text): `slug` maps each
/// character alone, and makes "a  b" `a--b`.
#[test]
#[ignore = "waits for a std function that sees neighbouring characters; slug does not collapse a run"]
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
    let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    harken_domain::domain::playlists::playlist_name(domain(), &names, name).unwrap()
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

/// A path becomes a URL against whichever server is asking, and a URL is
/// already an answer. Falsified by dropping the percent-encoding.
#[test]
fn a_file_is_joined_to_the_server_that_is_asking() {
    assert_eq!(
        url("http://10.0.0.2:8787", "music/Bach/air.flac"),
        "http://10.0.0.2:8787/media/music/Bach/air.flac"
    );
    assert_eq!(
        url("https://harken.example.com/", "music/a.mp3"),
        "https://harken.example.com/media/music/a.mp3"
    );
    assert_eq!(
        url("http://h", "music/Boléro & co/no #1.mp3"),
        "http://h/media/music/Bol%C3%A9ro%20%26%20co/no%20%231.mp3"
    );
    assert_eq!(
        url("http://h", "https://upload.wikimedia.org/x.mp3"),
        "https://upload.wikimedia.org/x.mp3"
    );
    assert_eq!(url("http://h", ""), "");
}

/// Thirty seconds or half the track, whichever is first. Falsified by
/// dropping the halving.
#[test]
fn a_play_counts_after_thirty_seconds_or_half_the_track() {
    assert_eq!(counts_after(300_000), 30_000);
    assert_eq!(counts_after(40_000), 20_000);
    assert_eq!(counts_after(60_000), 30_000);
    assert_eq!(counts_after(0), 30_000);
}
