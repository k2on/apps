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
    let details = c.list::<EpisodeDetailsEntry>(
        "episode_details",
        &ShowInput {
            playlist_id: NONE,
            id: id.clone(),
        },
    );
    assert!(details.iter().all(|d| d.show == "Gospel in Life" && d.show_id == id));
    assert_eq!(each(&details, |d| d.published_ms), [20, 10, 0], "as `show` lists them, newest first");
    assert!(c
        .list::<EpisodeDetailsEntry>(
            "episode_details",
            &ShowInput {
                playlist_id: NONE,
                id: "podcast/nobody".into(),
            },
        )
        .is_empty());
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

const FEED: &str = "https://feeds.transistor.fm/full-time-nix";

fn subscribe(kind: &str) -> Subscribe {
    Subscribe {
        kind: kind.into(),
        url: FEED.into(),
    }
}

/// A feed is followed from a client: a subscription keeping the newest
/// twenty, said again with its kind as said now and the rest as it was;
/// the server's read gives it its title and how many episodes the feed
/// has, and a failed read keeps them and says why. Falsified by
/// `feed_read` taking the title of a failed read: it was empty.
#[test]
fn a_feed_is_followed_and_read() {
    let mut c = Lib::new();
    c.mutate("library", "subscribe", &subscribe("podcast")).unwrap();
    let read = FeedRead {
        url: FEED.into(),
        title: "Full Time Nix".into(),
        items: 51,
        error: String::new(),
    };
    c.mutate("library", "feed_read", &read).unwrap();
    assert_eq!(c.mutate("library", "feed_read", &read), Ok(0));
    let failed = FeedRead {
        error: "connection refused".into(),
        ..FeedRead::default()
    };
    c.mutate("library", "feed_read", &FeedRead { url: FEED.into(), ..failed }).unwrap();
    c.mutate("library", "subscribe", &subscribe("sermon")).unwrap();
    let subs = c.list::<SubscriptionsEntry>("subscriptions", &());
    assert_eq!(
        each(&subs, |s| (s.kind.clone(), s.title.clone(), s.items, s.keep, s.error.clone())),
        [(
            "sermon".to_string(),
            "Full Time Nix".to_string(),
            51,
            shows::KEEP,
            "connection refused".to_string()
        )]
    );
    assert!(c
        .mutate(
            "library",
            "subscribe",
            &Subscribe {
                url: "feed.xml".into(),
                ..subscribe("podcast")
            }
        )
        .unwrap_err()
        .contains("a feed is an http or https URL"));
    let as_alice = c.ctx("alice", &[]);
    assert_eq!(
        c.mutate_as(&as_alice, "subscribe", &subscribe("podcast")),
        Err("only the library writes the library".into())
    );
}

/// Older episodes are more kept: twenty more, then every one, and every
/// one stays every one. Falsified by adding to a keep of 0: "every one"
/// became twenty.
#[test]
fn older_episodes_are_more_kept() {
    let mut c = Lib::new();
    c.mutate("library", "subscribe", &subscribe("podcast")).unwrap();
    let keep = |c: &mut Lib| c.rows::<Subscription>("subscription")[0].keep;
    let more = |n| FetchOlder { url: FEED.into(), more: n };
    c.mutate("library", "fetch_older", &more(20)).unwrap();
    assert_eq!(keep(&mut c), 40);
    c.mutate("library", "fetch_older", &more(0)).unwrap();
    assert_eq!(keep(&mut c), 0);
    assert_eq!(c.mutate("library", "fetch_older", &more(20)), Ok(0));
    assert!(c
        .mutate(
            "library",
            "fetch_older",
            &FetchOlder {
                url: "https://elsewhere".into(),
                more: 1
            }
        )
        .unwrap_err()
        .contains("not following"));
}

/// An episode is in the library before its file is: queued, then how its
/// download goes, then on the server. Falsified by `downloaded` leaving
/// the row: the episode was still queued.
#[test]
fn an_episode_is_listed_before_its_file_is_here() {
    let mut c = Lib::new();
    c.mutate("library", "add_show", &show("podcast", "Full Time Nix")).unwrap();
    let id = keys::show_key("podcast", "Full Time Nix");
    c.mutate("library", "add_episode", &episode(&id, "Nix Freaks 51", ".feeds/51.mp3", 5))
        .unwrap();
    let queue = QueueDownload {
        file: ".feeds/51.mp3".into(),
        source: "https://media/51.mp3".into(),
    };
    c.mutate("library", "queue_download", &queue).unwrap();
    assert_eq!(c.mutate("library", "queue_download", &queue), Ok(0));
    let downloads = |c: &mut Lib| {
        each(&c.list::<DownloadsEntry>("downloads", &()), |d| {
            (d.title.clone(), d.state.clone(), d.bytes, d.total)
        })
    };
    assert_eq!(downloads(&mut c), [("Nix Freaks 51".to_string(), "queued".to_string(), 0, 0)]);
    let going = DownloadProgress {
        file: ".feeds/51.mp3".into(),
        state: "downloading".into(),
        bytes: 5,
        total: 10,
        error: String::new(),
    };
    c.mutate("library", "download_progress", &going).unwrap();
    assert_eq!(downloads(&mut c), [("Nix Freaks 51".to_string(), "downloading".to_string(), 5, 10)]);
    assert!(c
        .mutate(
            "library",
            "download_progress",
            &DownloadProgress {
                state: "lost".into(),
                ..going.clone()
            }
        )
        .unwrap_err()
        .contains("queued, downloading or failed"));
    c.mutate(
        "library",
        "downloaded",
        &Downloaded {
            file: ".feeds/51.mp3".into(),
        },
    )
    .unwrap();
    assert!(downloads(&mut c).is_empty());
    assert_eq!(
        c.mutate(
            "library",
            "downloaded",
            &Downloaded {
                file: ".feeds/51.mp3".into()
            }
        ),
        Ok(0)
    );
    assert!(c
        .mutate("library", "download_progress", &going)
        .unwrap_err()
        .contains("not being downloaded"));
    assert!(c
        .mutate(
            "library",
            "queue_download",
            &QueueDownload {
                file: ".feeds/none.mp3".into(),
                ..queue
            }
        )
        .unwrap_err()
        .contains("no media has the file"));
}

/// A feed unfollowed takes its show and its episodes with it, a download
/// still to come too, and leaves another show be.
/// Falsified by `take_out` leaving the download: the media row's delete
/// was refused.
#[test]
fn a_feed_unfollowed_takes_its_show_with_it() {
    let mut c = Lib::new();
    c.mutate("library", "subscribe", &subscribe("podcast")).unwrap();
    c.mutate(
        "library",
        "add_show",
        &AddShow {
            feed: FEED.into(),
            ..show("podcast", "Full Time Nix")
        },
    )
    .unwrap();
    c.mutate("library", "add_show", &show("sermon", "Gospel in Life")).unwrap();
    let nix = keys::show_key("podcast", "Full Time Nix");
    let gil = keys::show_key("sermon", "Gospel in Life");
    c.mutate("library", "add_episode", &episode(&nix, "Fifty", ".feeds/50.mp3", 1)).unwrap();
    c.mutate("library", "add_episode", &episode(&nix, "Fifty-one", ".feeds/51.mp3", 2))
        .unwrap();
    c.mutate("library", "add_episode", &episode(&gil, "Grace", ".feeds/g.mp3", 3)).unwrap();
    let queue = QueueDownload {
        file: ".feeds/51.mp3".into(),
        source: "https://media/51.mp3".into(),
    };
    c.mutate("library", "queue_download", &queue).unwrap();
    assert_eq!(
        each(&c.list::<SubscriptionsEntry>("subscriptions", &()), |s| (s.show_id.clone(), s.episodes)),
        [(nix.clone(), 2)]
    );
    c.mutate("library", "unsubscribe", &Unsubscribe { url: FEED.into() }).unwrap();
    assert!(c.list::<SubscriptionsEntry>("subscriptions", &()).is_empty());
    assert_eq!(each(&c.list::<ShowsEntry>("shows", &()), |s| s.id.clone()), [gil]);
    assert_eq!(each(&c.rows::<Media>("media"), |m| m.title.clone()), ["Grace"]);
    assert!(c.rows::<Download>("download").is_empty());
}

/// An episode in the library, its media id.
fn one_episode(c: &mut Lib) -> Id<Media> {
    c.mutate("library", "add_show", &show("podcast", "Full Time Nix")).unwrap();
    let id = keys::show_key("podcast", "Full Time Nix");
    c.mutate("library", "add_episode", &episode(&id, "Nix Freaks 51", ".feeds/51.mp3", 5))
        .unwrap();
    c.rows::<Media>("media")[0].id
}

fn report(media_id: Id<Media>, position_ms: i64, name: &str) -> ReportProgress {
    ReportProgress {
        media_id,
        position_ms,
        duration_ms: 0,
        name: name.into(),
    }
}

/// Each person has their own place in an episode, and every member sees
/// everybody's: the show's progress lists both, and one's own lists only
/// theirs. Near its end it is played; a song keeps no place. Falsified by
/// keying progress by the episode alone: Bob's place moved Alice's.
#[test]
fn each_person_keeps_their_own_place_and_everybody_sees_it() {
    let mut c = Lib::new();
    let ep = one_episode(&mut c);
    c.add(song("Glue", "Bicep", ""));
    let glue = c.rows::<Media>("media").into_iter().find(|m| m.title == "Glue").unwrap().id;
    let alice = c.ctx("alice", &[]);
    c.mutate_as(&alice, "report_progress", &report(ep, 600_000, "Alice")).unwrap();
    let bob = c.ctx("bob", &[]);
    c.mutate_as(&bob, "report_progress", &report(ep, 1_790_000, "Bob")).unwrap();
    let alice = c.ctx("alice", &[]);
    assert_eq!(
        c.mutate_as(&alice, "report_progress", &report(ep, 600_000, "Alice")),
        Ok(0),
        "where it was: nothing"
    );
    assert_eq!(
        c.mutate_as(&alice, "report_progress", &report(glue, 10_000, "Alice")),
        Ok(0),
        "a song keeps no place"
    );
    let nobody = c.ctx("", &[]);
    assert!(c.mutate_as(&nobody, "report_progress", &report(ep, 1, "")).is_err());

    let id = keys::show_key("podcast", "Full Time Nix");
    let all: Vec<ProgressEntry> = c.query("carol", "show_progress", &ShowInput { playlist_id: NONE, id }).unwrap();
    let mut seen = each(&all, |p| (p.name.clone(), p.position_ms, p.duration_ms, p.finished));
    seen.sort();
    assert_eq!(
        seen,
        [
            ("Alice".to_string(), 600_000, 1_800_000, false),
            ("Bob".to_string(), 1_790_000, 1_800_000, true)
        ]
    );
    let mine: Vec<ProgressEntry> = c.query("alice", "my_progress", &()).unwrap();
    assert_eq!(each(&mine, |p| p.user_id.clone()), ["alice"]);
}

/// Marked played, it is at its end; unplayed, it has no place at all; a
/// song is neither. Falsified by `mark_played` keeping the position of an
/// unplayed one: it resumed where it was.
#[test]
fn an_episode_is_marked_played_or_not() {
    let mut c = Lib::new();
    let ep = one_episode(&mut c);
    let alice = c.ctx("alice", &[]);
    c.mutate_as(&alice, "report_progress", &report(ep, 60_000, "Alice")).unwrap();
    let alice = c.ctx("alice", &[]);
    c.mutate_as(
        &alice,
        "mark_played",
        &MarkPlayed {
            media_id: ep,
            finished: true,
            name: String::new(),
        },
    )
    .unwrap();
    let rows = c.rows::<Progress>("progress");
    assert_eq!(
        each(&rows, |p| (p.position_ms, p.finished, p.name.clone())),
        [(1_800_000, true, "Alice".to_string())]
    );
    let alice = c.ctx("alice", &[]);
    c.mutate_as(
        &alice,
        "mark_played",
        &MarkPlayed {
            media_id: ep,
            finished: false,
            name: String::new(),
        },
    )
    .unwrap();
    assert!(c.rows::<Progress>("progress").is_empty());
}

/// An episode's chapters are its feed's, in order, replaced when the feed
/// says others, and gone with the episode. Falsified by `set_chapters`
/// adding without taking away: the old second chapter stayed.
#[test]
fn an_episodes_chapters_are_its_feeds() {
    let mut c = Lib::new();
    let ep = one_episode(&mut c);
    let at = |start_ms: i64, title: &str| ChapterAt {
        start_ms,
        title: title.into(),
        ..ChapterAt::default()
    };
    let set = |chapters: Vec<ChapterAt>| SetChapters {
        file: ".feeds/51.mp3".into(),
        chapters,
    };
    c.mutate("library", "set_chapters", &set(vec![at(90_000, "News"), at(0, "Intro")]))
        .unwrap();
    assert_eq!(c.mutate("library", "set_chapters", &set(vec![at(0, "Intro"), at(90_000, "News")])), Ok(0));
    let read = |c: &Lib| {
        each(&c.list::<ChaptersEntry>("chapters", &EpisodeInput { media_id: ep }), |c| {
            (c.start_ms, c.title.clone())
        })
    };
    assert_eq!(read(&c), [(0, "Intro".to_string()), (90_000, "News".to_string())]);
    c.mutate("library", "set_chapters", &set(vec![at(0, "Intro"), at(120_000, "Interview")]))
        .unwrap();
    assert_eq!(read(&c), [(0, "Intro".to_string()), (120_000, "Interview".to_string())]);
    let alice = c.ctx("alice", &[]);
    c.mutate_as(&alice, "report_progress", &report(ep, 1, "Alice")).unwrap();
    c.mutate("library", "remove_media", &RemoveMedia { id: ep }).unwrap();
    assert!(c.rows::<Chapter>("chapter").is_empty() && c.rows::<Progress>("progress").is_empty());
}
