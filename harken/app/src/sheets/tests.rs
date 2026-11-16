use ark_app::Runtime;

use super::*;
use crate::testing::Demo;

/// A track's playlists: each it is on ticked; pressed, it is put on one or
/// taken off, as a change. Falsified by a press always adding.
#[test]
fn a_track_is_put_on_a_playlist_and_taken_off() {
    let mut rt = Runtime::<Sheets>::new();
    let mut d = Demo::<Sheets>::new();
    let track = d.first_song();
    d.run(&mut rt, [SheetsMsg::AddTo { track: track.clone() }]);
    let p = d.page(&mut rt);
    assert!(p.adding && !p.playlists.is_empty(), "{p:?}");
    let list = p.playlists[0].clone();
    d.run(&mut rt, [SheetsMsg::Toggle { playlist: list.id.clone() }]);
    let on = d.page(&mut rt).playlists[0].on;
    assert_ne!(on, list.on, "pressed: the other way");
    d.run(&mut rt, [SheetsMsg::Toggle { playlist: list.id.clone() }]);
    assert_eq!(d.page(&mut rt).playlists[0].on, list.on);
    d.run(&mut rt, [SheetsMsg::Dismiss]);
    assert!(!d.page(&mut rt).adding);
}

/// Following a show: a feed that is no address is refused with the
/// domain's sentence; one that is, is followed and the sheet goes.
#[test]
fn a_show_is_followed_by_its_feed() {
    let mut rt = Runtime::<Sheets>::new();
    let mut d = Demo::<Sheets>::new();
    d.run(
        &mut rt,
        [SheetsMsg::Follow { kind: "podcast".into() }, SheetsMsg::Url { url: "nope".into() }],
    );
    assert!(!d.page(&mut rt).following.unwrap().can_save);
    d.run(&mut rt, [SheetsMsg::Subscribe]);
    assert!(d.page(&mut rt).following.unwrap().problem.is_some());
    d.run(
        &mut rt,
        [
            SheetsMsg::Url {
                url: "https://example.com/feed.xml".into(),
            },
            SheetsMsg::Subscribe,
        ],
    );
    assert!(d.page(&mut rt).following.is_none());
}
