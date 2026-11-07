//! Typed views (`docs/plan-v5.md`, Landed: typed views): harken's view
//! rows through the bridge as their bytes in the value format, which read
//! as the domain's own types — what the phones' generated code reads them
//! as — made by name with the input's JSON, with no JSON per row.

use std::sync::Arc;

use ark::format::Encode;
use ark_ffi::{Login, Op, Peer};
use harken_domain::{LibraryEntry, Playlist};
use harken_ffi::harken_open;

/// Rows as `T`, from their bytes.
fn read<T: Encode>(rows: &[Vec<u8>]) -> Vec<T> {
    rows.iter().map(|r| T::from_bytes(r).unwrap()).collect()
}

/// A peer alone whose person holds the library's role, so that it can add
/// a song.
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
    harken_open(dir.path().to_str().unwrap().into(), Some(login), device.into()).unwrap()
}

/// The playlists and a playlist's contents, read typed while the old path
/// writes. Falsified by `take` handing over no rows (`rows: vec![]`): the
/// insert came with no playlist.
#[test]
fn a_views_rows_are_the_domains_own_types() {
    let dir = tempfile::tempdir().unwrap();
    let p = librarian(&dir);
    let lists = p.view("playlists.playlists".into(), "null".into()).unwrap();
    p.mutate("playlists.create_playlist".into(), r#"{"name":"Mix"}"#.into()).unwrap();
    let taken = lists.take();
    assert_eq!(taken.ops, vec![Op::Insert { at: 0 }]);
    let made: Vec<Playlist> = read(&taken.rows);
    assert_eq!(made.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Mix"]);
    assert_eq!(lists.take().ops, vec![]);
    let playlist = read::<Playlist>(&lists.rows())[0].clone();
    assert_eq!(playlist.user_id, "u");

    let song = r#"{"title":"Air","artist":"Bach","album":"","duration_ms":1000,"file":"air.flac","track":1,"part":"","catalogue":"","performer":"","bpm":0,"album_art":"","artist_art":"","disc":1,"work_title":"","movement_no":0,"artist_banner":"","released":0,"folder":""}"#;
    p.mutate("library.add_song".into(), song.into()).unwrap();
    let input = format!(r#"{{"playlist_id":"{}"}}"#, ark::hash::hex(&playlist.id.0));
    let library = p.view("library.library".into(), input.clone()).unwrap();
    let media = ark::hash::hex(&read::<LibraryEntry>(&library.rows())[0].id.0);
    let on = p.view("playlists.playlist".into(), input).unwrap();
    assert!(on.rows().is_empty());
    let add = format!(r#"{{"playlist_id":"{}","media_id":"{media}"}}"#, ark::hash::hex(&playlist.id.0));
    p.mutate("playlists.add_to_playlist".into(), add).unwrap();
    let rows: Vec<LibraryEntry> = read(&on.rows());
    assert_eq!((rows[0].title.as_str(), rows[0].playlist_pos), ("Air", Some(1)));
    assert_eq!(ark::hash::hex(&rows[0].id.0), media, "an id crosses as its sixteen bytes");
    let taken = library.take();
    assert_eq!(taken.ops, vec![Op::Update { at: 0 }]);
    let marked: Vec<LibraryEntry> = read(&taken.rows);
    assert_eq!(marked[0].playlist_pos, Some(1), "the library marks what is on the playlist");
    let missing = p.view("playlists.nothing".into(), "null".into());
    assert!(missing.is_err(), "no such view");
}
