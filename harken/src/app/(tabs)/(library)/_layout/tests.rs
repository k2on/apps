use ark_app::Runtime;

use super::*;
use crate::testing::Demo;

fn library() -> (Runtime<Library>, Demo<Library>) {
    (Runtime::new(), Demo::new())
}

#[test]
fn the_library_opens_on_the_playlists_and_each_chip_shows_its_own() {
    let (mut rt, mut d) = library();
    let p = d.page(&mut rt);
    assert!(p.on_playlists && !p.on_albums);
    assert_eq!(
        p.chips.iter().filter(|c| c.selected).map(|c| c.tag.as_str()).collect::<Vec<_>>(),
        ["playlists"]
    );
    assert!(!p.playlists.is_empty(), "the demo has playlists");
    assert_eq!(p.playlists[0].name, "Favorites");
    d.run(&mut rt, [LibraryMsg::Show { kind: "albums".into() }]);
    let p = d.page(&mut rt);
    assert!(p.on_albums && !p.albums.is_empty() && !p.empty);
    d.run(&mut rt, [LibraryMsg::Show { kind: "songs".into() }]);
    let p = d.page(&mut rt);
    assert!(p.songs.iter().all(|t| !t.title.is_empty()) && !p.songs.is_empty());
}

#[test]
fn a_tile_opens_the_page_it_stands_for() {
    let (mut rt, mut d) = library();
    d.run(&mut rt, [LibraryMsg::Show { kind: "albums".into() }]);
    let a = d.page(&mut rt).albums[0].clone();
    d.run(
        &mut rt,
        [LibraryMsg::OpenAlbum {
            key: a.key.clone(),
            title: a.title.clone(),
        }],
    );
    assert_eq!(d.opened, [Route::Album { key: a.key, title: a.title }]);
}

#[test]
fn a_song_plays_the_songs_from_where_it_is() {
    let (mut rt, mut d) = library();
    d.run(&mut rt, [LibraryMsg::Show { kind: "songs".into() }]);
    let songs = d.page(&mut rt).songs;
    d.run(&mut rt, [LibraryMsg::Play { id: songs[2].id.clone() }]);
    let [HarkenRequest::Play { tracks, at, from, shuffle }] = d.requests.as_slice() else {
        panic!("{:?}", d.requests)
    };
    assert_eq!((*at, from.as_str(), *shuffle), (2, "/library/", false));
    assert_eq!(tracks[2], songs[2].id);
}

#[test]
fn a_new_playlists_name_is_checked_and_a_taken_one_numbered() {
    let (mut rt, mut d) = library();
    d.run(&mut rt, [LibraryMsg::NewPlaylist, LibraryMsg::Name { name: "  ".into() }]);
    let form = d.page(&mut rt).naming.expect("the sheet is up");
    assert!(!form.can_save);
    d.run(&mut rt, [LibraryMsg::SavePlaylist]);
    assert_eq!(d.page(&mut rt).naming.and_then(|f| f.problem).as_deref(), Some("a playlist needs a name"));
    d.run(&mut rt, [LibraryMsg::Name { name: "Favorites".into() }, LibraryMsg::SavePlaylist]);
    let p = d.page(&mut rt);
    assert!(p.naming.is_none());
    assert!(p.playlists.iter().any(|l| l.name == "Favorites (1)"), "{:?}", p.playlists);
}

#[test]
fn every_way_has_its_words_when_it_is_empty() {
    let (mut rt, mut d) = library();
    d.run(&mut rt, [LibraryMsg::Show { kind: "sermons".into() }]);
    let p = d.page(&mut rt);
    assert_eq!(p.empty, p.shows.is_empty());
    assert_eq!(p.empty_title, "No Sermons");
}

#[test]
fn a_song_is_drawn_with_its_albums_cover() {
    let (mut rt, mut d) = library();
    d.run(&mut rt, [LibraryMsg::Show { kind: "albums".into() }]);
    let covers: Vec<String> = d.page(&mut rt).albums.iter().map(|a| a.art.clone()).filter(|a| !a.is_empty()).collect();
    assert!(!covers.is_empty(), "the demo's albums have covers");
    d.run(&mut rt, [LibraryMsg::Show { kind: "songs".into() }]);
    let songs = d.page(&mut rt).songs;
    assert!(songs.iter().any(|t| !t.art.is_empty()), "no song has a cover");
    assert!(
        songs.iter().filter(|t| !t.art.is_empty()).all(|t| covers.contains(&t.art)),
        "a song's cover is its album's"
    );
}
