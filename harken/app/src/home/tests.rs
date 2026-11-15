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

#[test]
fn a_new_song_plays_the_newest_from_itself_and_an_album_opens() {
    let mut rt = Runtime::<Home>::new();
    let mut d = Demo::<Home>::new();
    let p = d.page(&mut rt);
    d.run(&mut rt, [HomeMsg::PlayNew { id: p.newest[1].key.clone() }]);
    assert!(matches!(d.requests.as_slice(), [HarkenRequest::Play { at: 1, .. }]), "{:?}", d.requests);
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

#[test]
fn the_view_draws_and_records() {
    let mut rt = Runtime::<Home>::new();
    let mut d = Demo::<Home>::new();
    let _ = view::home::view(&d.page(&mut rt));
    let _ = view::home::record();
}
