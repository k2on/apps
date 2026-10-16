//! Plays over real sockets: `record_play` is a mutation like any other, so
//! a play made with no connection is a pending entry, dated when it was
//! made, and reaches the server — and every other device of that account —
//! whenever the device that made it does.
//!
//! When a play counts (thirty seconds or half the track), that it counts
//! once, and that only the device making the sound authors it are the
//! reconciler's, and tested there (`harken-iced`'s `listening` tests); the
//! rows and the two queries are `harken/domain/tests/agreement.rs`'s.

mod common;

use ark::eval::Args;
use ark::value::Value;
use common::*;
use harken_domain::listening::read::{input, Track};

fn track(title: &str, n: u8) -> Track {
    let mut id = [0u8; 16];
    id[15] = n;
    Track {
        id,
        title: title.into(),
        creator: "Handel".into(),
        album: "Water Music".into(),
        duration_ms: 180_000,
        file: format!("music/{title}.mp3"),
    }
}

/// The titles a play query answers, in its order.
fn titles(p: &ark_client::Peer, query: &str) -> Vec<String> {
    p.query(query, &Args::new())
        .unwrap()
        .as_list()
        .iter()
        .map(|row| match row.field("title") {
            Value::Text(t) => t.to_string(),
            other => panic!("a play's title is {other:?}"),
        })
        .collect()
}

/// A phone plays two tracks while it has no connection: both are there at
/// once, on the phone, newest first, and nowhere else. It comes back, and
/// the laptop signed in as the same person has both, in the same order,
/// with the same answer to `last_played`; somebody else on the server has
/// none of them.
///
/// Falsified three ways: by never reconnecting the phone (the laptop never
/// had them — so nothing but the phone's own push carries them); by
/// `recently_played` ordering oldest first (the phone's own order, offline,
/// failed); and by both queries dropping their filter to the caller (the
/// other account read max's plays).
#[test]
fn a_play_made_offline_reaches_every_device_when_it_comes_back() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |_| {});
    let mut phone = signed_in(&server, "max");
    let mut laptop = signed_in(&server, "max");
    let mut ana = signed_in(&server, "ana");
    pump_until(&mut [&mut phone, &mut laptop, &mut ana], 10, |ps| {
        ps.iter_mut().all(|p| p.linked())
    });

    phone.disconnect();
    pump_until(&mut [&mut phone], 10, |ps| !ps[0].linked());
    let from = "album/Water Music";
    phone
        .mutate(
            "record_play",
            input::record_play(&track("Hornpipe", 1), from),
        )
        .unwrap();
    // A clock that moves between them, so newest is a fact and not a tie
    // broken by an id.
    std::thread::sleep(std::time::Duration::from_millis(5));
    phone
        .mutate(
            "record_play",
            input::record_play(&track("Bourrée", 2), from),
        )
        .unwrap();
    assert_eq!(
        titles(&phone, "recently_played"),
        ["Bourrée", "Hornpipe"],
        "offline, at once"
    );
    assert_eq!(titles(&phone, "last_played"), ["Bourrée"]);
    for _ in 0..40 {
        laptop.pump();
        phone.pump();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        titles(&laptop, "recently_played").is_empty(),
        "nothing reached the laptop while the phone was away"
    );
    assert!(phone.pending_len() > 0, "the plays are pending");

    phone.connect(&server.running.sync_url());
    pump_until(&mut [&mut phone, &mut laptop, &mut ana], 10, |ps| {
        ps[0].linked() && ps[0].pending_len() == 0 && titles(&*ps[1], "recently_played").len() == 2
    });
    assert_eq!(titles(&laptop, "recently_played"), ["Bourrée", "Hornpipe"]);
    assert_eq!(titles(&laptop, "last_played"), ["Bourrée"]);
    assert_eq!(
        titles(&phone, "recently_played"),
        ["Bourrée", "Hornpipe"],
        "confirmed as it was made"
    );
    let row = laptop.query("last_played", &Args::new()).unwrap().as_list()[0].clone();
    assert_eq!(row.field("from"), Value::text(from));
    assert_eq!(
        row.field("track"),
        Value::text(ark::stdlib::text_of_id(&track("Bourrée", 2).id))
    );
    assert!(
        titles(&ana, "recently_played").is_empty(),
        "somebody else's plays"
    );
    assert!(titles(&ana, "last_played").is_empty());
    rt.block_on(server.stop());
}
