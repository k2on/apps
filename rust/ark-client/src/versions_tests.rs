//! A peer and a server on different modules (`docs/plan-db.md` D1), as
//! `Peer` reports it: `held` for an intent the server cannot run yet,
//! `behind` for a server whose module is not this peer's. The engine's
//! half is `ark/tests/versions.rs`; this is what an app sees of it. Each
//! test says what falsified it.

use ark::live::{ConnId, Silent};
use ark::peer::Authority;
use ark::protocol::{open_access, trusting, ClientMsg, Server};
use ark::value::Value;

use crate::{args, demo, Options, Peer};

/// A server over the demo's schema holding `keep` of its closures, saying
/// `module` as its own.
fn server(keep: impl Fn(&str) -> bool, module: Vec<u8>) -> Server<Silent> {
    let d = demo::domain();
    let bodies = d
        .closures()
        .iter()
        .filter(|(_, c)| keep(&c.function.name))
        .map(|(h, c)| (h.clone(), c.clone()))
        .collect();
    let mut a = Authority::new(d.module().schema.clone(), bodies);
    a.log.name_if_unnamed([7; 16]);
    Server::open(trusting(), open_access(), Silent, a).with_module(module)
}

/// Frames both ways until nothing moves; what the peer said.
fn settle(s: &mut Server<Silent>, c: ConnId, p: &mut Peer) -> Vec<ClientMsg> {
    let mut said = vec![];
    loop {
        let up = p.take_outgoing();
        let mut moved = !up.is_empty();
        for m in up {
            said.push(m.clone());
            s.recv(c, m);
        }
        for (to, m) in s.take_outgoing() {
            if to == c {
                moved = true;
                p.recv(m);
            }
        }
        if !moved {
            return said;
        }
    }
}

/// An intent the server cannot run is `held`: pending, counted in
/// `status().held`, no rejection taken; and confirmed by the next
/// connection, to a server that can.
///
/// Falsified once: with `Status::held` set to 0 rather than asked of the
/// client, the first assertion failed.
#[test]
fn a_held_intent_is_pending_and_said_until_it_lands() {
    let d = demo::domain();
    let mut p = Peer::open_memory(d.clone(), Options::dev("alice")).unwrap();
    let mut old = server(|n| n != "create_playlist", b"older".to_vec());
    p.connected();
    settle(&mut old, 1, &mut p);
    p.mutate("create_playlist", args([("name", Value::text("Road"))])).unwrap();
    settle(&mut old, 1, &mut p);
    let st = p.status();
    assert_eq!((st.pending, st.held), (1, 1), "pending, and held");
    assert!(p.take_rejections().is_empty(), "nothing was refused");

    let mut new = server(|_| true, d.hash());
    p.disconnected();
    p.connected();
    settle(&mut new, 2, &mut p);
    let st = p.status();
    assert_eq!((st.pending, st.held, st.cursor), (0, 0, 1), "landed");
    assert!(!st.behind, "the server's module is this peer's");
}

/// A server whose module is not this peer's: `behind`, and a verify asked
/// for says nothing on the wire.
///
/// Falsified once: with `Client::verify_all` not consulting `behind`, a
/// `Verify` went out.
#[test]
fn behind_is_said_and_verify_is_not() {
    let mut p = Peer::open_memory(demo::domain(), Options::dev("alice")).unwrap();
    let mut s = server(|_| true, b"another module".to_vec());
    p.connected();
    settle(&mut s, 1, &mut p);
    assert!(p.status().behind, "the server says another module");
    p.verify();
    let said = settle(&mut s, 1, &mut p);
    assert!(!said.iter().any(|m| matches!(m, ClientMsg::Verify { .. })), "{said:?}");
}
