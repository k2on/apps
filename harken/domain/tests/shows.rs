//! The shows: podcasts and sermons, as `shows.rs` writes and reads them.
//! Each test was falsified once by breaking the function it names; the doc
//! comment says how.

mod common;

use common::{each, song, Lib};
use harken_domain::Id;
use harken_domain::*;

fn show(kind: &str, title: &str) -> AddShow {
    AddShow {
        kind: kind.into(),
        title: title.into(),
        author: "Timothy Keller".into(),
        feed: "https://podcast.gospelinlife.com/feed.xml".into(),
        ..AddShow::default()
    }
}

fn episode(show: &str, title: &str, file: &str, published_ms: i64) -> AddEpisode {
    AddEpisode {
        show: show.into(),
        title: title.into(),
        duration_ms: 1_800_000,
        file: file.into(),
        published_ms,
        ..AddEpisode::default()
    }
}

const NONE: Id<Playlist> = Id::new([0; 16]);

/// An episode is a media row of its show's kind, beside an episode row;
/// the show lists them newest first, and the music leaves them out.
/// Falsified by `library` not filtering on the kind: the sermon was a song.
#[test]
fn an_episode_is_media_of_its_shows_kind_and_not_music() {
    let mut c = Lib::new();
    c.add(song("Glue", "Bicep", ""));
    c.mutate("library", "add_show", &show("sermon", "Gospel in Life")).unwrap();
    let id = keys::show_key("sermon", "Gospel in Life");
    assert_eq!(id, "sermon/gospel-in-life");
    for (t, f, at) in [
        ("Older", ".feeds/a.mp3", 10),
        ("Newer", ".feeds/b.mp3", 20),
        ("Undated", ".feeds/c.mp3", 0),
    ] {
        c.mutate("library", "add_episode", &episode(&id, t, f, at)).unwrap();
    }
    let rows = c.list::<LibraryEntry>(
        "show",
        &ShowInput {
            playlist_id: NONE,
            id: id.clone(),
        },
    );
    assert_eq!(each(&rows, |r| r.title.clone()), ["Newer", "Older", "Undated"]);
    assert!(rows.iter().all(|r| r.kind == "sermon" && r.creator == "Gospel in Life"));
    assert_eq!(each(&c.library(&NONE), |r| r.title.clone()), ["Glue"]);
    assert_eq!(each(&c.list::<ArtistsEntry>("artists", &()), |a| a.name.clone()), ["Bicep"]);
    let shows = c.list::<ShowsEntry>("shows", &());
    assert_eq!(
        each(&shows, |s| (s.kind.clone(), s.episodes, s.newest_ms)),
        [("sermon".to_string(), 3, 20)]
    );
    assert_eq!(shows[0].author, "Timothy Keller");
    let details = c.list::<EpisodeDetailsEntry>("episode_details", &());
    assert!(details.iter().all(|d| d.show == "Gospel in Life" && d.show_id == id));
}

/// A show or an episode said again with nothing new writes nothing, and an
/// episode said again with more keeps its id and place. Falsified by
/// `add_episode` minting an id for a file it has: the episode was two.
#[test]
fn a_feed_read_again_writes_nothing() {
    let mut c = Lib::new();
    let s = show("podcast", "Full Time Nix");
    assert!(c.mutate("library", "add_show", &s).unwrap() > 0);
    assert_eq!(c.mutate("library", "add_show", &s), Ok(0));
    let id = keys::show_key("podcast", "Full Time Nix");
    let e = episode(&id, "Nix Freaks 51", ".feeds/51.mp3", 5);
    assert!(c.mutate("library", "add_episode", &e).unwrap() > 0);
    assert_eq!(c.mutate("library", "add_episode", &e), Ok(0));
    let before = c.rows::<Media>("media");
    let more = AddEpisode {
        description: "Wild takes.".into(),
        ..e
    };
    assert!(c.mutate("library", "add_episode", &more).unwrap() > 0);
    let after = c.rows::<Media>("media");
    assert_eq!(each(&after, |m| (m.id, m.pos)), each(&before, |m| (m.id, m.pos)));
    assert_eq!(c.rows::<Episode>("episode")[0].description, "Wild takes.");
}

/// Only the library writes a show, a show is a podcast or a sermon, and
/// an episode needs its show. Falsified by `add_episode` not reading the
/// show: an episode of nothing was written.
#[test]
fn a_show_is_the_librarys_and_an_episode_needs_one() {
    let mut c = Lib::new();
    let as_alice = c.ctx("alice", &[]);
    assert_eq!(
        c.mutate_as(&as_alice, "add_show", &show("podcast", "X")),
        Err("only the library writes the library".into())
    );
    assert!(c
        .mutate("library", "add_show", &show("audiobook", "X"))
        .unwrap_err()
        .contains("a show is a podcast or a sermon"));
    assert!(c
        .mutate("library", "add_episode", &episode("podcast/x", "One", "a.mp3", 0))
        .unwrap_err()
        .contains("no show podcast/x"));
}

/// An episode taken out of the library goes off its show, as a song does
/// off its album. Falsified by `remove_media` leaving the episode row.
#[test]
fn an_episode_removed_is_gone_from_its_show() {
    let mut c = Lib::new();
    c.mutate("library", "add_show", &show("podcast", "Full Time Nix")).unwrap();
    let id = keys::show_key("podcast", "Full Time Nix");
    c.mutate("library", "add_episode", &episode(&id, "Bump", ".feeds/b.mp3", 1)).unwrap();
    let media = c.rows::<Media>("media")[0].id;
    c.mutate("library", "remove_media", &RemoveMedia { id: media }).unwrap();
    assert!(c.rows::<Episode>("episode").is_empty());
    assert_eq!(c.list::<ShowsEntry>("shows", &())[0].episodes, 0);
}
