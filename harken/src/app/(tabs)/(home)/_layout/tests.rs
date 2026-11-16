use ark_app::Runtime;

use super::*;
use crate::testing::Demo;

#[test]
fn home_shows_the_newest_songs_the_playlists_and_the_albums() {
    let mut rt = Runtime::<Home>::new();
    let mut d = Demo::<Home>::new();
    let p = d.page(&mut rt);
    assert!(!p.empty);
    assert!(!p.newest.is_empty() && p.newest.len() <= NEWEST);
    assert!(p.has_playlists && p.has_albums);
}

/// What was added most recently is each album once, with its cover and
/// its newest songs under it: never the same cover again and again.
/// Pressed, an album opens its page. Falsified by a tile a song (the
/// demo's songs are all on albums), or an album twice.
#[test]
fn the_newest_are_each_album_once_and_open_it() {
    let mut rt = Runtime::<Home>::new();
    let mut d = Demo::<Home>::new();
    let p = d.page(&mut rt);
    assert!(p.newest.iter().all(|t| t.key.starts_with("album:")), "{:?}", p.newest);
    let mut keys: Vec<&str> = p.newest.iter().map(|t| t.key.as_str()).collect();
    keys.dedup();
    assert_eq!(keys.len(), p.newest.len(), "an album twice");
    let mut arts: Vec<&str> = p.newest.iter().map(|t| t.art.as_str()).collect();
    arts.sort();
    arts.dedup();
    assert_eq!(arts.len(), p.newest.len(), "a cover twice");
    let first = p.newest[0].clone();
    assert!(p.albums.iter().any(|a| first.key == format!("album:{}", a.key) && a.title == first.title));
    d.run(&mut rt, [HomeMsg::OpenNew { key: first.key.clone() }]);
    assert_eq!(
        d.opened,
        [Route::Album {
            key: first.key.trim_start_matches("album:").into(),
            title: first.title.clone()
        }]
    );
    d.opened.clear();
    let a = p.albums[0].clone();
    d.run(
        &mut rt,
        [HomeMsg::OpenAlbum {
            key: a.key.clone(),
            title: a.title.clone(),
        }],
    );
    assert_eq!(d.opened, [Route::Album { key: a.key, title: a.title }]);
}
