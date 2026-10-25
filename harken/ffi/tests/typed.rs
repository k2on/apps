//! Typed views (`docs/plan-v5.md`, Landed: typed views): harken's view
//! rows through UniFFI as the domain's own types — a view object per row
//! type (`PlaylistView`, `LibraryEntryView`) made by name with its input's
//! JSON, its rows and patches read with no JSON — and the twins of the
//! types a row may hold (nested sums, options, lists, ids).

use std::sync::Arc;

use ark_ffi::{Login, Op, Peer, Refusal, RowId, Taken, Twin};
use harken_domain::{LibraryEntryView, PlaylistView};
use harken_ffi::harken_open;

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
    };
    let device = r#"{"can_play":true,"id":"","kind":"phone","name":"Pixel","user":""}"#;
    let p = harken_open(dir.path().to_str().unwrap().into(), Some(login), device.into()).unwrap();
    p.alone();
    p
}

/// The playlists and a playlist's contents, read typed while the old path
/// writes. Falsified by the view object's `take` handing over no rows (`rows:
/// vec![]`): the insert came with no playlist; and by `view_typed` not
/// comparing a view's rows with its type: `library.albums` was opened as
/// `LibraryEntry` rows.
#[test]
fn a_views_rows_are_the_domains_own_types() {
    let dir = tempfile::tempdir().unwrap();
    let p = librarian(&dir);
    let lists = PlaylistView::new(p.clone(), "playlists.playlists".into(), "null".into()).unwrap();
    p.mutate("playlists.create_playlist".into(), r#"{"name":"Mix"}"#.into()).unwrap();
    let taken = lists.take();
    assert_eq!(
        taken.update,
        Taken::Patched {
            ops: vec![Op::Insert { at: 0 }]
        }
    );
    assert_eq!(taken.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Mix"]);
    assert_eq!(lists.take().update, Taken::Unchanged);
    assert_eq!(lists.rows()[0].user_id, "u");

    let song = r#"{"title":"Air","artist":"Bach","album":"","duration_ms":1000,"file":"air.flac","track":1,"part":"","catalogue":"","performer":"","bpm":0,"album_art":"","artist_art":"","disc":1,"work_title":"","movement_no":0,"artist_banner":""}"#;
    p.mutate("library.add_song".into(), song.into()).unwrap();
    let RowId(playlist) = lists.rows()[0].id;
    let input = format!(r#"{{"playlist_id":"{}"}}"#, ark::hash::hex(&playlist));
    let library = LibraryEntryView::new(p.clone(), "library.library".into(), input.clone()).unwrap();
    let media = ark::hash::hex(&library.rows()[0].id.0);
    let on = LibraryEntryView::new(p.clone(), "playlists.playlist".into(), input).unwrap();
    assert!(on.rows().is_empty());
    let add = format!(r#"{{"playlist_id":"{}","media_id":"{media}"}}"#, ark::hash::hex(&playlist));
    p.mutate("playlists.add_to_playlist".into(), add).unwrap();
    let rows = on.rows();
    assert_eq!((rows[0].title.as_str(), rows[0].playlist_pos), ("Air", Some(1)));
    assert_eq!(ark::hash::hex(&rows[0].id.0), media, "an id crosses as its sixteen bytes");
    let taken = library.take();
    assert_eq!(
        taken.update,
        Taken::Patched {
            ops: vec![Op::Update { at: 0 }]
        }
    );
    assert_eq!(taken.rows[0].playlist_pos, Some(1), "the library marks what is on the playlist");

    let wrong = LibraryEntryView::new(p.clone(), "library.albums".into(), "null".into());
    assert!(matches!(wrong, Err(Refusal::Refused { why }) if why.contains("rows are not")));
    let missing = PlaylistView::new(p.clone(), "playlists.nothing".into(), "null".into());
    assert!(missing.is_err(), "no such view");
}

/// UniFFI's tag for this test's own twins: what `setup_scaffolding!` would
/// define, which this test cannot call beside the library's own.
pub struct UniFfiTag;

mod nested {
    use ark::prelude::*;
    use harken_domain::Media;

    #[derive(Clone, Debug, PartialEq, Describe)]
    #[ark(ffi)]
    pub struct Inner {
        pub at: Option<i64>,
        pub ids: Vec<Id<Media>>,
    }

    #[derive(Clone, Debug, PartialEq, Describe)]
    #[ark(ffi)]
    pub enum Outer {
        Has { inner: Inner, tags: Vec<Option<String>> },
        Wraps(Inner),
        Nothing,
    }
}

/// Falsified by the enum twin's way back dropping a lone unnamed field
/// (`unreachable!()` in its place): the round trip of `Wraps` panicked.
#[test]
fn nested_sums_options_lists_and_ids_have_twins() {
    use nested::{Inner, InnerFfi, Outer, OuterFfi};
    let inner = Inner {
        at: Some(3),
        ids: vec![ark::row::Id::new([7; 16])],
    };
    let all = [
        Outer::Has {
            inner: inner.clone(),
            tags: vec![Some("a".into()), None],
        },
        Outer::Wraps(inner.clone()),
        Outer::Nothing,
    ];
    for o in all {
        let t = <Outer as Twin<crate::UniFfiTag>>::ffi(o.clone());
        assert_eq!(<Outer as Twin<crate::UniFfiTag>>::from_ffi(t), o);
    }
    let w = <Outer as Twin<crate::UniFfiTag>>::ffi(Outer::Wraps(inner));
    let OuterFfi::Wraps { value: InnerFfi { at, ids } } = w else {
        panic!("Wraps is a record of its value: {w:?}")
    };
    assert_eq!((at, ids), (Some(3), vec![RowId([7; 16])]));
}
