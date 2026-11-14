//! harken's views as the phones hold them (`HarkenSession`): what the
//! shells did by hand before — a view held while a screen shows it, a page
//! as its views, the rows spliced as changes land, the app told which
//! moved, a change's refusal kept.

use std::sync::{Arc, Mutex};

use ark_ffi::{Login, Peer, RowId};
use harken_ffi::{harken_open, Changed, HarkenSession as Session, HarkenView};

fn librarian(dir: &tempfile::TempDir) -> Arc<Peer> {
    let login = Login {
        token: String::new(),
        session: "s".into(),
        user: "u".into(),
        name: String::new(),
        email: String::new(),
        roles: vec![harken_domain::LIBRARY.into()],
        expires_ms: 0,
        picture: String::new(),
    };
    let device = r#"{"can_play":true,"id":"","kind":"phone","name":"Pixel","user":""}"#;
    let p = harken_open(dir.path().to_str().unwrap().into(), Some(login), device.into()).unwrap();
    p.alone();
    p
}

fn song(title: &str, artist: &str, album: &str, file: &str) -> String {
    format!(
        r#"{{"title":"{title}","artist":"{artist}","album":"{album}","duration_ms":1000,"file":"{file}","track":1,"part":"","catalogue":"","performer":"","bpm":0,"album_art":"","artist_art":"","disc":1,"work_title":"","movement_no":0,"artist_banner":"","released":0,"folder":""}}"#
    )
}

#[derive(Default)]
struct Told(Mutex<Vec<HarkenView>>);

impl Changed for Told {
    fn moved(&self, views: Vec<HarkenView>) {
        self.0.lock().unwrap().extend(views);
    }
}

const NONE: RowId = RowId([0; 16]);

/// The library held is read at once and spliced as songs land; the app is
/// told it moved, and the albums held beside it too, but not the playlists,
/// which a song does not touch. Falsified by telling every held view: the
/// playlists were told.
#[test]
fn a_held_view_is_spliced_and_told() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new(librarian(&dir));
    let told = Arc::new(Told::default());
    s.set_listener(Some(told.clone()));
    let library = HarkenView::Library { playlist: NONE };
    let (_l, _a, _p) = (s.hold(library.clone()), s.hold(HarkenView::Albums), s.hold(HarkenView::Playlists));
    assert!(s.rows(library.clone()).is_empty());
    s.send("library.add_song".into(), song("Air", "Bach", "Suites", "a.flac")).unwrap();
    let rows = s.rows(library.clone());
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains("\"title\":\"Air\""), "{rows:?}");
    let titles: Vec<String> = s.tracks(library.clone()).into_iter().map(|t| t.title).collect();
    assert_eq!(titles, ["Air"], "and as records");
    let moved = told.0.lock().unwrap().clone();
    assert!(moved.contains(&library) && moved.contains(&HarkenView::Albums), "{moved:?}");
    assert!(!moved.contains(&HarkenView::Playlists), "{moved:?}");
}

/// An album's page is its tracks and who is on it; held, both are read,
/// and let go, neither is. Falsified by a page whose parts are not held:
/// the album's tracks were none.
#[test]
fn a_page_is_its_views() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new(librarian(&dir));
    s.send("library.add_song".into(), song("Air", "Bach", "Suites", "a.flac")).unwrap();
    let _albums = s.hold(HarkenView::Albums);
    let album = s.rows(HarkenView::Albums)[0].clone();
    let id = serde_json::from_str::<serde_json::Value>(&album).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let page = HarkenView::AlbumPage {
        playlist: NONE,
        id: id.clone(),
    };
    let tracks = HarkenView::Album {
        playlist: NONE,
        id: id.clone(),
    };
    assert_eq!(s.parts(page.clone()), [tracks.clone(), HarkenView::AlbumCredits { id }]);
    let held = s.hold(page.clone());
    assert_eq!(s.rows(tracks.clone()).len(), 1);
    assert!(s.version(page) > 0);
    drop(held);
    assert!(s.rows(tracks).is_empty(), "let go");
}

/// A refusal is the domain's sentence, kept until the next change.
#[test]
fn a_refusal_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new(librarian(&dir));
    assert!(s.send("playlists.create_playlist".into(), r#"{"name":" "}"#.into()).is_err());
    assert!(s.refusal().is_some());
    s.send("playlists.create_playlist".into(), r#"{"name":"Mix"}"#.into()).unwrap();
    assert_eq!(s.refusal(), None);
}
