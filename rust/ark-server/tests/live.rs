//! Live rooms on the sync socket: relayed within an account and never
//! across, a repeated Hello that is paging and not a departure, a snapshot
//! kept when a room empties and across a restart, and a device the server
//! stands in for.

mod common;

use std::sync::{Arc, Mutex};

use ark::value::Value;
use ark_client::args;
use ark_server::{Echo, Live, Peer as RoomPeer, Post};
use common::*;

#[test]
fn a_frame_reaches_the_rest_of_the_account_and_nobody_else() {
    let rt = runtime();
    let running = serve(&rt, None, |b| b.live(Echo));
    let mut phone = peer(&running, "alice");
    let mut laptop = peer(&running, "alice");
    let mut bob = peer(&running, "bob");
    pump_until(&mut [&mut phone, &mut laptop, &mut bob], |ps| ps.iter().all(|p| p.linked()));
    // Two connections of one account are one room.
    pump_until(&mut [&mut phone], |_| {
        rt.block_on(running.hub.read(|h| h.rooms().get("alice").map(Vec::len))).unwrap() == Some(2)
    });
    phone.say_value(&Value::text("pause"));
    let mut heard = vec![];
    pump_until(&mut [&mut phone, &mut laptop, &mut bob], |ps| {
        heard.extend(ps[1].heard_values());
        !heard.is_empty()
    });
    assert_eq!(heard, [Value::text("pause")]);
    pump_for(&mut [&mut phone, &mut laptop, &mut bob], 100);
    assert!(phone.heard().is_empty(), "the speaker is not echoed");
    assert!(bob.heard().is_empty(), "another account hears nothing");
    // Unlinked, a frame is dropped rather than queued: "pause" is not worth
    // saying on Friday.
    laptop.disconnect();
    laptop.say(b"late".to_vec());
    laptop.reconnect();
    pump_until(&mut [&mut laptop, &mut phone], |ps| ps[0].linked());
    pump_for(&mut [&mut laptop, &mut phone], 100);
    assert!(phone.heard().is_empty());
    rt.block_on(running.stop());
}

/// Counts what the room was told, per call.
#[derive(Clone, Default)]
struct Counts(Arc<Mutex<Vec<String>>>);

impl Live for Counts {
    fn open(&mut self, room: &str, kept: Option<&[u8]>) {
        self.0
            .lock()
            .unwrap()
            .push(format!("open {room} {:?}", kept.map(|b| String::from_utf8_lossy(b).to_string())));
    }
    fn join(&mut self, peer: &RoomPeer, _: &mut Post<'_>) {
        self.0.lock().unwrap().push(format!("join {}", peer.room));
    }
    fn say(&mut self, _: &RoomPeer, _: &[u8], _: &mut Post<'_>) {}
    fn part(&mut self, peer: &RoomPeer, _: &mut Post<'_>) {
        self.0.lock().unwrap().push(format!("part {}", peer.room));
    }
    fn close(&mut self, room: &str) {
        self.0.lock().unwrap().push(format!("close {room}"));
    }
}

/// The bug harken's device picker found: a log longer than one batch has
/// the client say `Hello` again on the same socket for the rest, and a
/// server that read every `Hello` as a departure and an arrival emptied the
/// room under a device that never left.
#[test]
fn a_repeated_hello_on_one_connection_is_paging_not_a_departure() {
    let rt = runtime();
    let counts = Counts::default();
    let running = serve(&rt, None, |b| b.live(counts.clone()));
    let mut writer = peer(&running, "writer");
    writer.mutate("create_playlist", args([("name", Value::text("Long"))])).unwrap();
    let list = playlist(&writer, "Long");
    // More than a batch (ark::protocol::BATCH_LIMIT is 256).
    for i in 0..300 {
        writer
            .mutate(
                "add_to_playlist",
                args([("playlist_id", Value::Id(list)), ("track_id", Value::text(format!("t{i}")))]),
            )
            .unwrap();
    }
    pump_until(&mut [&mut writer], |ps| ps[0].pending_len() == 0);

    let mut reader = peer(&running, "reader");
    pump_until(&mut [&mut reader], |ps| ps[0].cursor() == 301);
    pump_for(&mut [&mut reader], 100);
    let seen: Vec<String> = counts
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.ends_with("reader") || l.contains(" reader "))
        .cloned()
        .collect();
    assert_eq!(
        seen,
        ["open reader None", "join reader"],
        "paged through {} entries on one connection",
        301
    );
    assert_eq!(rt.block_on(running.hub.read(|h| h.rooms().get("reader").map(Vec::len))).unwrap(), Some(1));
    rt.block_on(running.stop());
}

/// Remembers the last thing said in each room and keeps it.
#[derive(Clone, Default)]
struct Memory(Arc<Mutex<std::collections::BTreeMap<String, Vec<u8>>>>);

impl Live for Memory {
    fn open(&mut self, room: &str, kept: Option<&[u8]>) {
        if let Some(b) = kept {
            self.0.lock().unwrap().insert(room.into(), b.to_vec());
        }
    }
    fn join(&mut self, peer: &RoomPeer, post: &mut Post<'_>) {
        if let Some(b) = self.0.lock().unwrap().get(&peer.room) {
            post.tell_conn(peer.conn, b.clone());
        }
    }
    fn say(&mut self, peer: &RoomPeer, frame: &[u8], _: &mut Post<'_>) {
        self.0.lock().unwrap().insert(peer.room.clone(), frame.to_vec());
    }
    fn snapshot(&mut self, room: &str) -> Option<Vec<u8>> {
        self.0.lock().unwrap().get(room).cloned()
    }
    fn close(&mut self, room: &str) {
        self.0.lock().unwrap().remove(room);
    }
}

#[test]
fn a_room_keeps_its_snapshot_when_it_empties_and_across_a_restart() {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let running = serve(&rt, Some(data.path()), |b| b.live(Memory::default()));
    let mut a = peer(&running, "alice");
    pump_until(&mut [&mut a], |ps| ps[0].linked());
    a.say(b"queue: 3 tracks, at 2".to_vec());
    pump_for(&mut [&mut a], 100);
    drop(a);
    // The last one out: the room closes and its snapshot is written.
    let path = data.path().join("live.cbor");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !path.is_file() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(path.is_file(), "the kept room is on disk");
    rt.block_on(running.stop());

    let running = serve(&rt, Some(data.path()), |b| b.live(Memory::default()));
    let mut a = peer(&running, "alice");
    let mut heard = vec![];
    pump_until(&mut [&mut a], |ps| {
        heard.extend(ps[0].heard());
        !heard.is_empty()
    });
    assert_eq!(heard, [b"queue: 3 tracks, at 2".to_vec()], "the room woke from what it kept");
    rt.block_on(running.stop());
}

#[test]
fn a_device_the_server_stands_in_for_hears_and_speaks() {
    let rt = runtime();
    let running = serve(&rt, None, |b| b.live(Echo));
    let mut a = peer(&running, "alice");
    // Linked is the Hello sent; the room has it when the hub says so.
    pump_until(&mut [&mut a], |_| {
        rt.block_on(running.hub.read(|h| h.rooms().contains_key("alice"))).unwrap()
    });
    let (tx, rx) = std::sync::mpsc::channel();
    let kitchen = running.hub.stand("alice", "media_player.kitchen", move |f| tx.send(f).unwrap()).unwrap();
    a.say(b"play".to_vec());
    pump_for(&mut [&mut a], 50);
    assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), b"play");
    running.hub.say(kitchen, b"now playing".to_vec()).unwrap();
    let mut heard = vec![];
    pump_until(&mut [&mut a], |ps| {
        heard.extend(ps[0].heard());
        !heard.is_empty()
    });
    assert_eq!(heard, [b"now playing".to_vec()]);
    let peers = rt
        .block_on(running.hub.read(|h| h.rooms()["alice"].iter().map(|p| p.who.clone()).collect::<Vec<_>>()))
        .unwrap();
    assert_eq!(a.status().opens, 1, "{:?}", a.status());
    assert_eq!(peers, ["dev", "media_player.kitchen"]);
    running.hub.detach(kitchen).unwrap();
    let n = rt.block_on(running.hub.read(|h| h.rooms()["alice"].len())).unwrap();
    assert_eq!(n, 1);
    rt.block_on(running.stop());
}
