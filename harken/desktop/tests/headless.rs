//! The peer without a terminal or a socket: alone, and against an
//! in-process `ark::protocol::Server` with the frames carried by hand.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ark::canon;
use ark::hash::{closures, state_hash};
use ark::live::{ConnId, Silent};
use ark::peer::Authority;
use ark::protocol::{open_access, trusting, ClientMsg, Server};
use ark::value::Id;
use harken_desktop::domain::{self, LIBRARY, PLAYLISTS};
use harken_desktop::peer::{load_module, Config, Peer};

fn fresh_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("harken-desktop-{name}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn alone(user: &str, data: &Path) -> Config {
    Config {
        server: None,
        user: user.into(),
        data: data.to_path_buf(),
        module: None,
    }
}

fn tracks(p: &Peer) -> Vec<domain::Track> {
    domain::library(&p.db(LIBRARY).unwrap()).unwrap()
}

fn playlists(p: &Peer) -> Vec<domain::Playlist> {
    domain::playlists(&p.db(PLAYLISTS).unwrap()).unwrap()
}

fn items(p: &Peer, pid: Id) -> Vec<(i64, Id)> {
    domain::playlist_items(&p.db(PLAYLISTS).unwrap(), pid)
        .unwrap()
        .into_iter()
        .map(|i| (i.pos, i.track_id))
        .collect()
}

#[test]
fn alone_a_playlist_is_made_filled_and_survives_a_restart() {
    let data = fresh_dir("alone");
    let cfg = alone("alice", &data);
    let mut p = Peer::open(cfg.clone()).unwrap();
    assert!(p.alone());
    assert_eq!(p.scopes(), vec![LIBRARY.to_string(), PLAYLISTS.to_string()]);

    // No client authors add_track; a peer alone seeds its library through
    // the module's own closure, by intent.
    p.author_by_intent(
        "add_track",
        domain::add_track_args("Air", "Bach", Some("Orchestral Suite No. 3"), 300_000, "music/bach/air.flac"),
    )
    .unwrap();
    p.author_by_intent(
        "add_track",
        domain::add_track_args("Aria", "Bach", Some("Goldberg Variations"), 250_000, "music/bach/aria.flac"),
    )
    .unwrap();
    // A rescan is a no-op inside apply.
    p.author_by_intent("add_track", domain::add_track_args("Air", "Bach", None, 300_000, "music/bach/air.flac"))
        .unwrap();
    let ts = tracks(&p);
    assert_eq!(
        ts.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
        vec!["Aria", "Air"],
        "by artist, album, title"
    );

    // harken's mutators, through its own procedures.
    p.call(domain::create_playlist("  Favorites ".into())).unwrap();
    let pls = playlists(&p);
    assert_eq!(pls.len(), 1);
    assert_eq!(pls[0].name, "Favorites", "trimmed by the mutator");
    assert_eq!(pls[0].user_id, "alice");
    let pid = pls[0].id;
    p.call(domain::add_to_playlist(pid, ts[1].id)).unwrap();
    p.call(domain::add_to_playlist(pid, ts[0].id)).unwrap();
    p.call(domain::add_to_playlist(pid, ts[0].id)).unwrap(); // already there: a no-op
    assert_eq!(items(&p, pid), vec![(1, ts[1].id), (2, ts[0].id)]);

    // A refusal is a verdict, records nothing, and is shown.
    let why = p.call(domain::create_playlist("   ".into())).unwrap_err();
    assert!(why.contains("a playlist needs a name"), "{why}");
    assert_eq!(p.status().last_refusal.as_deref(), Some(why.as_str()));
    // A second playlist of the same name for the same person is a no-op.
    p.call(domain::create_playlist("Favorites".into())).unwrap();
    assert_eq!(playlists(&p).len(), 1);

    let st = p.status();
    assert_eq!(st.pending, 0, "alone, nothing stays pending");
    assert_eq!(st.cursors, vec![(LIBRARY.into(), 3), (PLAYLISTS.into(), 5)]);
    if cfg!(debug_assertions) {
        assert_eq!(p.agreement_checks, 6, "every call, the refused one too, was held to the interpreter");
    }
    let before: BTreeMap<String, (i64, Vec<u8>)> = p.scopes().into_iter().map(|s| (s.clone(), p.verify_at(&s).unwrap())).collect();
    assert!(data.join("library.cbor").is_file());
    assert!(data.join("playlists.cbor").is_file());
    drop(p);

    // Reopened from the files: the same rows, the same cursors and hashes.
    let mut p = Peer::open(cfg).unwrap();
    assert_eq!(tracks(&p), ts);
    assert_eq!(playlists(&p), pls);
    assert_eq!(items(&p, pid), vec![(1, ts[1].id), (2, ts[0].id)]);
    let after: BTreeMap<String, (i64, Vec<u8>)> = p.scopes().into_iter().map(|s| (s.clone(), p.verify_at(&s).unwrap())).collect();
    assert_eq!(before, after);

    // …and it goes on sequencing from where it was.
    p.call(domain::remove_from_playlist(pid, ts[1].id).unwrap()).unwrap();
    assert_eq!(items(&p, pid), vec![(2, ts[0].id)]);
    assert_eq!(p.status().cursors, vec![(LIBRARY.into(), 3), (PLAYLISTS.into(), 6)]);
    let _ = std::fs::remove_dir_all(&data);
}

// -- two peers through one in-process server -------------------------------

struct Net {
    server: Server<Silent>,
    /// Which connection each linked peer is on.
    conns: BTreeMap<usize, ConnId>,
    next: ConnId,
}

impl Net {
    fn new() -> Net {
        let module = load_module(None).unwrap();
        let bodies = closures(&module);
        let mut server = Server::open(trusting(), open_access(), Silent);
        for sc in &module.schema.scopes {
            let mut a = Authority::new(module.schema.clone(), &sc.name, bodies.clone());
            a.hold(harken_domain::module().procedures());
            server.host(a);
        }
        Net {
            server,
            conns: BTreeMap::new(),
            next: 1,
        }
    }

    fn connect(&mut self, i: usize, peers: &mut [Peer]) {
        let c = self.next;
        self.next += 1;
        self.conns.insert(i, c);
        peers[i].connected();
        self.settle(peers);
    }

    fn drop_link(&mut self, i: usize, peers: &mut [Peer]) {
        if let Some(c) = self.conns.remove(&i) {
            self.server.disconnect(c);
        }
        peers[i].disconnected();
        self.settle(peers);
    }

    /// Carry frames both ways until nothing is in flight.
    fn settle(&mut self, peers: &mut [Peer]) {
        loop {
            let mut moved = false;
            for (i, c) in self.conns.clone() {
                for frame in peers[i].take_outgoing() {
                    let msg = ClientMsg::from_value(&canon::decode(&frame).unwrap()).unwrap();
                    self.server.recv(c, msg);
                    moved = true;
                }
            }
            let by_conn: BTreeMap<ConnId, usize> = self.conns.iter().map(|(i, c)| (*c, *i)).collect();
            for (c, msg) in self.server.take_outgoing() {
                if let Some(i) = by_conn.get(&c) {
                    peers[*i].recv_frame(&canon::encode(&msg.to_value())).unwrap();
                    moved = true;
                }
            }
            if !moved {
                return;
            }
        }
    }

    fn hash(&self, scope: &str) -> Vec<u8> {
        state_hash(&self.server.scopes[scope].store)
    }
}

fn against(user: &str, data: &Path) -> Config {
    Config {
        server: Some("ws://in-process/sync".into()),
        user: user.into(),
        data: data.to_path_buf(),
        module: None,
    }
}

#[test]
fn an_offline_add_lands_after_what_arrived_meanwhile() {
    let (da, db) = (fresh_dir("a"), fresh_dir("b"));
    let mut net = Net::new();
    let mut peers = vec![Peer::open(against("alice", &da)).unwrap(), Peer::open(against("alice", &db)).unwrap()];
    assert!(!peers[0].alone());
    net.connect(0, &mut peers);
    net.connect(1, &mut peers);
    assert!(peers[0].status().linked && peers[1].status().linked);

    // B, standing in for the scanner, puts three tracks in the library and
    // makes a playlist; A sees all of it.
    for (title, file) in [("One", "1.flac"), ("Two", "2.flac"), ("Three", "3.flac")] {
        peers[1]
            .author_by_intent("add_track", domain::add_track_args(title, "X", None, 1_000, file))
            .unwrap();
    }
    peers[1].call(domain::create_playlist("Shared".into())).unwrap();
    net.settle(&mut peers);
    let ts = tracks(&peers[0]);
    assert_eq!(ts.len(), 3);
    assert_eq!(ts, tracks(&peers[1]));
    let pid = playlists(&peers[0])[0].id;
    let by_title = |t: &str| ts.iter().find(|x| x.title == t).unwrap().id;
    let (t1, t2, t3) = (by_title("One"), by_title("Two"), by_title("Three"));

    peers[1].call(domain::add_to_playlist(pid, t1)).unwrap();
    net.settle(&mut peers);
    assert_eq!(items(&peers[0], pid), vec![(1, t1)]);

    // A goes offline; B adds Two meanwhile; A, alone with what it has, adds
    // Three — at position 2, since that is all it can see.
    net.drop_link(0, &mut peers);
    assert!(!peers[0].status().linked);
    peers[1].call(domain::add_to_playlist(pid, t2)).unwrap();
    net.settle(&mut peers);
    assert_eq!(items(&peers[1], pid), vec![(1, t1), (2, t2)]);
    peers[0].call(domain::add_to_playlist(pid, t3)).unwrap();
    assert_eq!(items(&peers[0], pid), vec![(1, t1), (2, t3)]);
    assert_eq!(peers[0].status().pending, 1);

    // Restarted from its files while still offline, A keeps the pending
    // intent and its optimistic view of it.
    let a = peers.remove(0);
    let cursor_before = a.status().cursors.clone();
    drop(a);
    peers.insert(0, Peer::open(against("alice", &da)).unwrap());
    assert_eq!(peers[0].status().pending, 1);
    assert_eq!(peers[0].status().cursors, cursor_before);
    assert_eq!(items(&peers[0], pid), vec![(1, t1), (2, t3)]);

    // Back online: the rebase. A's item lands after what arrived while it
    // was away, and both peers agree with the authority.
    net.connect(0, &mut peers);
    assert_eq!(items(&peers[0], pid), vec![(1, t1), (2, t2), (3, t3)]);
    assert_eq!(items(&peers[1], pid), vec![(1, t1), (2, t2), (3, t3)]);
    assert_eq!(peers[0].status().pending, 0);
    for scope in [LIBRARY, PLAYLISTS] {
        let (ca, ha) = peers[0].verify_at(scope).unwrap();
        let (cb, hb) = peers[1].verify_at(scope).unwrap();
        assert_eq!(ca, cb, "{scope}: cursors");
        assert_eq!(ha, hb, "{scope}: A and B");
        assert_eq!(ha, net.hash(scope), "{scope}: the authority");
    }
    assert_eq!(peers[0].status().diverged, 0);

    // A removal from B reaches A the same way.
    peers[1].call(domain::remove_from_playlist(pid, t2).unwrap()).unwrap();
    net.settle(&mut peers);
    assert_eq!(items(&peers[0], pid), vec![(1, t1), (3, t3)]);
    let _ = std::fs::remove_dir_all(&da);
    let _ = std::fs::remove_dir_all(&db);
}
