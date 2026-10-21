//! harken's peer through the bridge: opened alone on a disk, a playlist
//! made by name and seen in a view, and the listening session read.

use ark_ffi::ViewUpdate;
use harken_ffi::{harken_listening, harken_open};

/// Falsified by `config` registering no domain for the log
/// (`Config::new` over the module alone): `playlists` was no function.
#[test]
fn harken_opens_and_is_called_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let device = r#"{"can_play":true,"id":"","kind":"phone","name":"Pixel","user":""}"#;
    let p = harken_open(dir.path().to_str().unwrap().into(), None, device.into()).unwrap();
    p.alone();
    let lists = p.view("playlists".into(), "null".into()).unwrap();
    p.mutate("create_playlist".into(), r#"{"name":"Mix"}"#.into()).unwrap();
    let ViewUpdate::Patched { patches } = lists.take() else {
        panic!("patched")
    };
    assert_eq!(patches.len(), 1);
    assert_eq!(
        p.check("create_playlist".into(), r#"{"name":" "}"#.into()).as_deref(),
        Some("a playlist needs a name")
    );
    let l: serde_json::Value = serde_json::from_str(&harken_listening(p)).unwrap();
    assert_eq!(l, serde_json::json!({"devices": [], "scrub": {}, "session": null}));
}
