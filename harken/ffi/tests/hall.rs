//! A hall: two devices of one account dial a harken server in the process,
//! stand in its listening session, and one's press is the other's state
//! once carried.

use harken_ffi::{harken_listening, Hall};

fn device(name: &str) -> String {
    format!(r#"{{"can_play":true,"id":"","kind":"phone","name":"{name}","user":""}}"#)
}

fn listening(p: &std::sync::Arc<ark_ffi::Peer>) -> serde_json::Value {
    serde_json::from_str(&harken_listening(p.clone())).unwrap()
}

/// Falsified by `carry` returning before the hub had answered (no
/// barrier): the second device never saw the first standing in the session.
#[test]
fn two_devices_share_a_session() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let hall = Hall::new();
    let phone = hall
        .device(a.path().to_str().unwrap().into(), "alice".into(), "phone".into(), device("Phone"))
        .unwrap();
    let laptop = hall
        .device(b.path().to_str().unwrap().into(), "alice".into(), "laptop".into(), device("Laptop"))
        .unwrap();
    hall.carry();
    let ids: Vec<String> = listening(&laptop)["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["laptop", "phone"]);
    let track = r#"{"album":"","creator":"","duration_ms":1000,"file":"a.flac","id":"00000000000000000000000000000001","title":"A"}"#;
    phone
        .call("start".into(), format!(r#"{{"at":0,"from":"","position":0,"queue":[{track}]}}"#))
        .unwrap();
    hall.carry();
    let s = &listening(&laptop)["session"];
    assert_eq!(s["output"]["Device"]["id"], "phone", "{s}");
    assert_eq!(phone.status().pending, 0);
    let held: serde_json::Value = serde_json::from_str(&hall.listening("alice".into())).unwrap();
    assert_eq!(&held["session"], s, "the hall holds what the laptop does");
}
