use ark_app::Runtime;

use super::*;
use crate::library::{Library, LibraryMsg};
use crate::testing::Demo;

fn first_album() -> (String, String) {
    let mut rt = Runtime::<Library>::new();
    let mut d = Demo::<Library>::new();
    d.run(&mut rt, [LibraryMsg::Show { kind: "albums".into() }]);
    let a = d.page(&mut rt).albums[0].clone();
    (a.key, a.title)
}

#[test]
fn an_albums_page_is_its_header_and_its_tracks() {
    let (key, title) = first_album();
    let mut rt = Runtime::<Page>::new();
    let mut d = Demo::<Page>::new();
    d.run(
        &mut rt,
        [PageMsg::Show {
            route: Route::Album { key, title: title.clone() },
        }],
    );
    let p = d.page(&mut rt);
    assert_eq!((p.kicker.as_str(), p.title.as_str()), ("Album", title.as_str()));
    assert!(!p.tracks.is_empty() && !p.empty);
    assert!(p.caption.contains("track"), "{}", p.caption);
}

#[test]
fn play_and_shuffle_play_the_page_from_its_first_and_a_row_from_itself() {
    let (key, title) = first_album();
    let route = Route::Album { key, title };
    let mut rt = Runtime::<Page>::new();
    let mut d = Demo::<Page>::new();
    d.run(&mut rt, [PageMsg::Show { route: route.clone() }]);
    let tracks = d.page(&mut rt).tracks;
    d.run(
        &mut rt,
        [
            PageMsg::PlayAll { shuffle: true },
            PageMsg::Play {
                id: tracks.last().unwrap().id.clone(),
            },
        ],
    );
    let [HarkenRequest::Play {
        at: 0, shuffle: true, from, ..
    }, HarkenRequest::Play { at, shuffle: false, .. }] = d.requests.as_slice()
    else {
        panic!("{:?}", d.requests)
    };
    assert_eq!(*from, route.url());
    assert_eq!(*at as usize, tracks.len() - 1);
}

#[test]
fn a_playlists_page_is_its_tracks_and_a_persons_is_round() {
    let mut rt = Runtime::<Page>::new();
    let mut d = Demo::<Page>::new();
    let favorites: Vec<Playlist> = {
        d.hold(vec![HarkenView::Playlists]);
        d.store
            .rows::<Vec<u8>>(&HarkenView::Playlists)
            .iter()
            .filter_map(|b| <Playlist as ark::format::Encode>::from_bytes(b).ok())
            .collect()
    };
    let f = &favorites[0];
    d.run(
        &mut rt,
        [PageMsg::Show {
            route: Route::Playlist {
                id: f.id.hex(),
                name: String::new(),
            },
        }],
    );
    let p = d.page(&mut rt);
    assert_eq!((p.kicker.as_str(), p.title.as_str()), ("Playlist", "Favorites"));
    assert_eq!(p.caption, crate::words::tracks(p.tracks.len() as i64));
    d.run(
        &mut rt,
        [PageMsg::Show {
            route: Route::Artist { name: "Glenn Gould".into() },
        }],
    );
    assert!(d.page(&mut rt).round);
}

#[test]
fn the_view_draws_the_header_and_the_rows() {
    let (key, title) = first_album();
    let mut rt = Runtime::<Page>::new();
    let mut d = Demo::<Page>::new();
    d.run(
        &mut rt,
        [PageMsg::Show {
            route: Route::Album { key, title },
        }],
    );
    let v = view::record::view(&d.page(&mut rt));
    let ark_app::view::View::Part(ark_app::view::Part::Detail { .. }) = v else {
        panic!("a record's page is a detail page")
    };
    let _ = view::record::record();
}
