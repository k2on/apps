//! A fill (§1.7): a replica the authority cannot page is sent its rows in
//! parts, each asked for once the one before is held, the tables a domain
//! sends later last; what the authority logs meanwhile is taken as it
//! comes; a fill reopened goes on where it was; an intent made meanwhile
//! is the authority's to judge. The demo, its playlists sent later: the
//! items come before the playlists they name.

use std::collections::VecDeque;

use ark::format::Encode;
use ark::frames::Frame;
use ark::peer::{Input, Journal, Output, Peer as Machine, Role, Who};
use ark_peer::{Instance, Memory, Module};
use arkdb::demo::{AddToPlaylist, CreatePlaylist, Demo, Playlist, RemoveFromPlaylist, RenamePlaylist};

fn module() -> Module {
    Module::of(&arkdb::demo::demo().later("playlist"))
}

fn who(name: &str) -> Who {
    Who {
        actor: name.into(),
        session: format!("{name}-session"),
        roles: vec![],
    }
}

fn server() -> Machine {
    let m = module();
    let mut p = Machine::new(
        m.schema.clone(),
        m.functions.clone(),
        Role::Authority { journal: Journal::default() },
        who("server"),
    );
    p.later = m.later().to_vec();
    p
}

fn replica() -> Machine {
    let m = module();
    Machine::new(m.schema.clone(), m.functions.clone(), Role::Replica, who("alice"))
}

fn alice() -> Instance {
    Instance::new(replica(), Some(Box::new(Memory::default())))
}

/// `func` on `input`, as the server's own.
fn author(server: &mut Machine, func: &str, input: Vec<u8>) -> Vec<Output> {
    let o = server.step(Input::Mutate {
        func: func.into(),
        input,
        now: 1,
        cause: None,
    });
    assert!(o.iter().any(|x| matches!(x, Output::Confirmed(_))), "{func} refused: {o:?}");
    o
}

/// Eighty playlists of 40 KB names, about 3.2 MB, and five items on the
/// first, the horizon at the head: a log no replica is paged from 0, and a
/// fill of four parts or so.
fn library(server: &mut Machine) {
    for i in 0..80 {
        let name = format!("{i:02}{}", "x".repeat(40_000));
        author(server, "demo.create_playlist", CreatePlaylist { name }.to_bytes());
    }
    let first = playlists(server)[0].id;
    for t in 0..5 {
        let input = AddToPlaylist {
            playlist_id: first,
            track_id: format!("t{t}"),
        };
        author(server, "demo.add_to_playlist", input.to_bytes());
    }
    let head = server.log().head().0;
    server.step(Input::Compact(head));
}

fn playlists(p: &Machine) -> Vec<Playlist> {
    ark::db::read::<Demo, _>(&p.store, |db| db.playlist.scan())
}

fn items(p: &Machine) -> usize {
    ark::db::read::<Demo, _>(&p.store, |db| db.item.scan().len())
}

/// The frames a step says, in order: to the replica, what the authority
/// sends it and fans out; to the authority, what the replica sends.
fn frames(outs: &[Output]) -> Vec<Frame> {
    outs.iter()
        .filter_map(|o| match o {
            Output::Send(b) | Output::Fanout(b) => Some(Frame::dec(b).unwrap()),
            _ => None,
        })
        .collect()
}

/// The wire between them: what each has said and the other not heard yet.
#[derive(Default)]
struct Wire {
    up: VecDeque<Frame>,
    down: VecDeque<Frame>,
    parts: usize,
}

impl Wire {
    /// Frames carried both ways, the replica's first, until neither says
    /// anything, or until it has taken `stop` parts: the next it asked
    /// for is then held.
    fn run(&mut self, server: &mut Machine, alice: &mut Instance, stop: Option<usize>) {
        loop {
            if let Some(f) = self.down.pop_front() {
                let part = matches!(f, Frame::Part { .. });
                self.parts += part as usize;
                let o = alice.step(Input::Frame { from: None, bytes: f.enc() });
                self.up.extend(frames(&o));
                if part && Some(self.parts) == stop {
                    return;
                }
            } else if let Some(f) = self.up.pop_front() {
                let o = server.step(Input::Frame {
                    from: Some(who("alice")),
                    bytes: f.enc(),
                });
                self.down.extend(frames(&o));
            } else {
                return;
            }
        }
    }

    /// What the server logged, fanned out to the replica.
    fn fanned(&mut self, outs: &[Output]) {
        self.down.extend(frames(outs));
    }
}

/// The replica connected: its `Hello`, on the wire.
fn connect(alice: &mut Instance) -> Wire {
    let mut w = Wire::default();
    w.up.extend(frames(&alice.step(Input::Connected)));
    w
}

fn same(alice: &Machine, server: &Machine) {
    assert!(alice.fill.is_none(), "the fill is done");
    assert_eq!(alice.log().head(), server.log().head());
    assert_eq!(alice.store.state_hash(), server.store.state_hash());
    assert_eq!(alice.store.snapshot_rows(), server.store.snapshot_rows());
    assert!(alice.diverged.is_empty(), "and never diverged");
}

/// A replica the authority cannot page is filled in parts, the items
/// first and every one of them in the first part, the playlists after;
/// filled, it is the authority's store. Falsified by a fill sending tables
/// in the schema's order: no item was held after the first part.
#[test]
fn a_fill_comes_in_parts_the_later_tables_last() {
    let mut server = server();
    library(&mut server);
    let mut alice = alice();
    let mut wire = connect(&mut alice);
    wire.run(&mut server, &mut alice, Some(1));
    assert_eq!(items(&alice), 5, "every item, in the first part");
    assert!(playlists(&alice).len() < 80, "and not every playlist");
    assert!(alice.fill.is_some());
    wire.run(&mut server, &mut alice, None);
    assert!(wire.parts >= 3, "{} parts", wire.parts);
    same(&alice, &server);
}

/// What the authority logs while a replica fills is taken as it comes:
/// a playlist renamed that was sent, one renamed that was not, one made,
/// an item removed. Filled, the replica is the authority's store, its log
/// the authority's. Falsified by taking an entry's facts as a store
/// judges them: renaming a playlist not sent was no row to edit, and the
/// replica diverged.
#[test]
fn what_is_logged_during_a_fill_is_taken() {
    let mut server = server();
    library(&mut server);
    let mut alice = alice();
    let mut wire = connect(&mut alice);
    wire.run(&mut server, &mut alice, Some(1));
    let (held, all) = (playlists(&alice), playlists(&server));
    let sent = held[0].id;
    let unsent = all.iter().find(|p| held.iter().all(|h| h.id != p.id)).unwrap().id;
    for (func, input) in [
        (
            "demo.rename_playlist",
            RenamePlaylist {
                playlist_id: sent,
                name: "sent".into(),
            }
            .to_bytes(),
        ),
        (
            "demo.rename_playlist",
            RenamePlaylist {
                playlist_id: unsent,
                name: "unsent".into(),
            }
            .to_bytes(),
        ),
        ("demo.create_playlist", CreatePlaylist { name: "new".into() }.to_bytes()),
        (
            "demo.remove_from_playlist",
            RemoveFromPlaylist {
                playlist_id: all[0].id,
                track_id: "t0".into(),
            }
            .to_bytes(),
        ),
    ] {
        let o = author(&mut server, func, input);
        wire.fanned(&o);
    }
    wire.run(&mut server, &mut alice, None);
    same(&alice, &server);
    assert!(playlists(&alice).iter().any(|p| p.name == "unsent"));
}

/// A replica reopened during its fill holds what it had taken and goes on
/// from where it was: no part is sent it twice. Falsified by a part not
/// made durable: the reopened replica began the fill again.
#[test]
fn a_fill_reopened_goes_on_where_it_was() {
    let mut server = server();
    library(&mut server);
    let mut alice = alice();
    let mut wire = connect(&mut alice);
    wire.run(&mut server, &mut alice, Some(2));
    let (had, taken) = (playlists(&alice).len(), wire.parts);
    let mut alice = Instance::new(replica(), Some(alice.into_disk()));
    let stored = alice.stored();
    alice.step(Input::Open(stored));
    assert_eq!(playlists(&alice).len(), had, "what it had taken, held");
    assert!(alice.fill.is_some(), "and filling");
    let mut wire = connect(&mut alice);
    wire.run(&mut server, &mut alice, None);
    same(&alice, &server);
    let total = {
        let (mut s, mut a) = (server_with_library(), self::alice());
        let mut w = connect(&mut a);
        w.run(&mut s, &mut a, None);
        w.parts
    };
    assert_eq!(taken + wire.parts, total, "no part sent twice");
}

fn server_with_library() -> Machine {
    let mut s = server();
    library(&mut s);
    s
}

/// An intent made during a fill: previewed over what is held, or, naming a
/// playlist not sent yet, pending unrun — either way the authority's to
/// judge, and confirmed. Falsified by a filling replica refusing what its
/// store refuses: the item on the playlist not sent was refused.
#[test]
fn an_intent_during_a_fill_is_the_authoritys_to_judge() {
    let mut server = server();
    library(&mut server);
    let mut alice = alice();
    let mut wire = connect(&mut alice);
    wire.run(&mut server, &mut alice, Some(1));
    let held = playlists(&alice);
    let unsent = playlists(&server).into_iter().find(|p| held.iter().all(|h| h.id != p.id)).unwrap().id;
    for (func, input) in [
        ("demo.create_playlist", CreatePlaylist { name: "mine".into() }.to_bytes()),
        (
            "demo.add_to_playlist",
            AddToPlaylist {
                playlist_id: unsent,
                track_id: "late".into(),
            }
            .to_bytes(),
        ),
    ] {
        let o = alice.step(Input::Mutate {
            func: func.into(),
            input,
            now: 2,
            cause: None,
        });
        assert!(!o.iter().any(|x| matches!(x, Output::Refused { .. })), "{func}: {o:?}");
        wire.up.extend(frames(&o));
    }
    wire.run(&mut server, &mut alice, None);
    assert!(alice.pending.is_empty(), "every intent confirmed");
    same(&alice, &server);
    assert_eq!(items(&alice), 6);
}
