//! The typed path (`docs/plan-v5.md`, Experiment: one generator): harken's
//! own types and functions through UniFFI — a mutation with an input and
//! an id for its answer, its check, two views whose rows are the domain's,
//! and a job (`Job<Looked>`) claimed and finished with a `Result` — with
//! no JSON on the way but the old path's, beside it.

use std::sync::Arc;

use ark::effect::Job;
use ark_ffi::{Login, Op, Peer, RowId, Taken, Twin};
use harken_domain::playlists::{playlists_create_playlist, playlists_create_playlist_check, playlists_playlist, playlists_playlists};
use harken_domain::schema::{look_claim, look_due, look_finish, JobLookedFfi, LookClaimFfi, LookFinishFfi, LookOutcomeFfi};
use harken_domain::types::{CreatePlaylistFfi, LookedFfi, PlaylistInputFfi};
use harken_domain::{Looked, UniFfiTag as DomainTag};
use harken_ffi::harken_open;

/// A peer alone whose person holds the library's role, so that it can add
/// a song and ask for a look.
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

fn name(n: &str) -> CreatePlaylistFfi {
    CreatePlaylistFfi { name: n.into() }
}

/// Falsified by `check_value` answering `None`: the blank name passed.
#[test]
fn a_playlist_made_and_read_through_the_domains_own_types() {
    let dir = tempfile::tempdir().unwrap();
    let p = librarian(&dir);
    let lists = playlists_playlists(p.clone()).unwrap();
    let id = playlists_create_playlist(p.clone(), name("Mix")).unwrap();
    assert_eq!(id.len(), 32, "an intent's id, as mutate's");
    let taken = lists.take();
    assert_eq!(
        taken.update,
        Taken::Patched {
            ops: vec![Op::Insert { at: 0 }]
        }
    );
    assert_eq!(taken.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Mix"]);
    assert_eq!(lists.rows()[0].user_id, "u");
    assert_eq!(
        playlists_create_playlist_check(p.clone(), name(" ")).as_deref(),
        Some("a playlist needs a name")
    );
    assert_eq!(playlists_create_playlist_check(p.clone(), name("Fine")), None);
    let refused = playlists_create_playlist(p.clone(), name("  "));
    assert!(matches!(refused, Err(ark_ffi::Refusal::Refused { why }) if why.contains("a playlist needs a name")));

    // A song by the old path, put on the playlist by it too; read typed.
    let song = r#"{"title":"Air","artist":"Bach","album":"","duration_ms":1000,"file":"air.flac","track":1,"part":"","catalogue":"","performer":"","bpm":0,"album_art":"","artist_art":"","disc":1,"work_title":"","movement_no":0}"#;
    p.mutate("library.add_song".into(), song.into()).unwrap();
    let RowId(playlist) = lists.rows()[0].id;
    let library: serde_json::Value = {
        let v = p
            .view("library.library".into(), format!(r#"{{"playlist_id":"{}"}}"#, ark::hash::hex(&playlist)))
            .unwrap();
        serde_json::from_str(&v.rows()[0]).unwrap()
    };
    let media = library["id"].as_str().unwrap();
    let on = playlists_playlist(
        p.clone(),
        PlaylistInputFfi {
            playlist_id: RowId(playlist),
        },
    )
    .unwrap();
    assert!(on.rows().is_empty());
    let add = format!(r#"{{"playlist_id":"{}","media_id":"{media}"}}"#, ark::hash::hex(&playlist));
    p.mutate("playlists.add_to_playlist".into(), add).unwrap();
    let rows = on.rows();
    assert_eq!((rows[0].title.as_str(), rows[0].playlist_pos), ("Air", Some(1)));
    assert_eq!(ark::hash::hex(&rows[0].id.0), media, "an id crosses as its sixteen bytes");
}

/// Falsified by the job's twin writing `Done` as `Pending { due_ms: 0 }`:
/// the round trip came back pending.
#[test]
fn a_look_is_claimed_and_finished_through_its_twins() {
    let done = Job::Done(Looked { read: 3, authored: 2 });
    let ffi = <Job<Looked> as Twin<DomainTag>>::ffi(done.clone());
    assert_eq!(
        ffi,
        JobLookedFfi::Done {
            value: LookedFfi { read: 3, authored: 2 }
        }
    );
    assert_eq!(<Job<Looked> as Twin<DomainTag>>::from_ffi(ffi), done);

    let dir = tempfile::tempdir().unwrap();
    let p = librarian(&dir);
    p.mutate("library.look".into(), r#"{"paths":[]}"#.into()).unwrap();
    let due = look_due(p.clone()).unwrap();
    assert_eq!(due.rows.len(), 1);
    assert!(matches!(due.rows[0].state, JobLookedFfi::Pending { .. }));
    let id = due.rows[0].id;
    look_claim(p.clone(), LookClaimFfi { id, by: "me".into() }).unwrap();
    let outcome = LookOutcomeFfi::Ok {
        value: LookedFfi { read: 1, authored: 1 },
    };
    let finish = |outcome| LookFinishFfi {
        id,
        by: "me".into(),
        outcome,
    };
    look_finish(p.clone(), finish(outcome.clone())).unwrap();
    let after = look_due(p.clone()).unwrap();
    assert!(after.rows.is_empty() && after.next_ms.is_none(), "done: nothing more is due");
    assert!(look_finish(p.clone(), finish(outcome)).is_err(), "a finish of a done job is refused");
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
