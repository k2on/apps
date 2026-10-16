//! `docs/plan-live.md` A peer's live state: the room of the demo, through
//! a server in this process frame by frame — the sans-io halves, as
//! `ark-server`'s fleet test does over sockets.

use ark::live::state::DeviceDecl;
use ark::protocol::{open_access, ClientMsg, Server, ServerMsg};
use ark::value::Value;

use crate::{args, demo, Options, Peer};

fn peer(name: &str) -> Peer {
    let opts = Options::server("alice", format!("{name}-login"), Some("alice".into())).with_device(DeviceDecl::new(name, "phone", name, true));
    Peer::open_memory(demo::live_domain(), opts).unwrap()
}

fn server() -> Server<ark::live::Silent> {
    let d = demo::live_domain();
    let mut a = ark::peer::Authority::new(d.module().schema.clone(), d.closures().clone());
    a.hold(d.native_list());
    let auth: ark::protocol::Authenticate = Box::new(|tok| {
        let t = tok?;
        let (user, session) = t.split_once('@')?;
        Some(ark::protocol::Identity::new(user, session))
    });
    Server::open(auth, open_access(), ark::live::Silent, a).with_live(d.live().clone())
}

// Everything each peer said, to the server; everything the server said, to
// its peer; until nothing moves.
fn exchange(sv: &mut Server<ark::live::Silent>, peers: &mut [(&mut Peer, i64)]) {
    for _ in 0..20 {
        let mut moved = false;
        for (p, c) in peers.iter_mut() {
            for m in p.take_outgoing() {
                moved = true;
                sv.recv(*c, m);
            }
        }
        let out: Vec<(i64, ServerMsg)> = sv.take_outgoing();
        for (to, m) in out {
            if let Some((p, _)) = peers.iter_mut().find(|(_, c)| *c == to) {
                moved = true;
                p.recv(m);
            }
        }
        if !moved {
            return;
        }
    }
}

/// The third copy of the room emits what `module/live.json` records.
/// Falsified by leaving `devices` out of this copy's `on_restore`: the
/// bytes differed.
#[test]
fn the_rooms_third_copy_emits_the_vectors_bytes() {
    let v = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/vectors/module/live.json")).unwrap();
    let j = ark::json::decode(&v).unwrap();
    let want = j.field("bytes");
    let got = ark::value::hex(&ark::canon::encode(&ark::ir::module_value(demo::live_domain().module())));
    assert_eq!(Value::text(got), want);
}

/// Two devices of one account: what one does reaches the other through the
/// frames, its own pointer is never told back to it, and a peer's view
/// moves the moment it calls. Falsified by a `hello` that leaves out the
/// device the options declare: the room never reached either peer.
#[test]
fn two_peers_share_the_room() {
    let mut phone = peer("phone");
    let mut laptop = peer("laptop");
    phone.set_token(Some("alice@phone-login".into()));
    laptop.set_token(Some("alice@laptop-login".into()));
    let mut sv = server();
    phone.connected();
    laptop.connected();
    exchange(&mut sv, &mut [(&mut phone, 1), (&mut laptop, 2)]);
    assert_eq!(phone.live("room").unwrap().field("devices").as_struct().len(), 2);
    phone.call("room", "bump", args([("by", Value::Int(3))])).unwrap();
    assert_eq!(phone.live("room").unwrap().field("count"), Value::Int(3), "at once");
    phone.call("room", "point", args([("at", Value::Int(8))])).unwrap();
    let sent = phone.take_outgoing();
    assert!(
        matches!(&sent[..], [ClientMsg::Call(a), ClientMsg::Call(b)] if a.counter.is_none() && b.counter.is_some()),
        "{sent:?}"
    );
    for m in sent {
        sv.recv(1, m);
    }
    let out = sv.take_outgoing();
    assert!(
        !out.iter()
            .any(|(to, m)| *to == 1 && matches!(m, ServerMsg::Applied(a) if a.transition == "point")),
        "not its own pointer"
    );
    for (to, m) in out {
        match to {
            1 => phone.recv(m),
            _ => laptop.recv(m),
        }
    }
    let seen = laptop.live("room").unwrap();
    assert_eq!(
        (seen.field("count"), seen.field("pointer").field("phone")),
        (Value::Int(3), Value::Int(8))
    );
    assert_eq!(phone.live("room"), laptop.live("room"));
    assert!(phone.server_offset().is_some(), "the server's clock, read off what it stamped");
}

/// A label over a slot's size is refused by the preview, and the server
/// refuses it to a device that sends it anyway. Falsified by not capping:
/// the preview took it.
#[test]
fn an_oversize_slot_is_refused() {
    let mut phone = peer("phone");
    phone.set_token(Some("alice@phone-login".into()));
    let mut sv = server();
    phone.connected();
    exchange(&mut sv, &mut [(&mut phone, 1)]);
    let long = "x".repeat(ark::live::state::SLOT_CAP + 1);
    let r = phone.call("room", "label", args([("text", Value::text(long.clone()))]));
    assert!(
        matches!(&r, Err(crate::Error::Refused(ark::store::Refusal::Refused(w))) if w.starts_with("label: a slot holds at most 1024 bytes")),
        "{r:?}"
    );
    let call = ark::live::state::Call {
        instance: phone.instance("room").unwrap(),
        transition: "label".into(),
        input: args([("text", Value::text(long))]),
        id: [9; 16],
        counter: Some(i64::MAX),
    };
    sv.recv(1, ClientMsg::Call(call));
    let out = sv.take_outgoing();
    assert!(
        matches!(&out[..], [(1, ServerMsg::Refused(r))] if r.reason.contains("at most 1024")),
        "{out:?}"
    );
}

/// The throttle: of a burst of ephemeral calls one is sent at once and the
/// last after the window, and nothing in between; the view took every one.
/// Falsified by sending the first held call rather than the last: the
/// trailing send carried 2, not 5.
#[test]
fn a_throttled_call_sends_the_first_and_the_last() {
    let mut phone = peer("phone");
    phone.set_token(Some("alice@phone-login".into()));
    let mut sv = server();
    phone.connected();
    exchange(&mut sv, &mut [(&mut phone, 1)]);
    for at in 1..=5 {
        phone.call_throttled("room", "point", args([("at", Value::Int(at))])).unwrap();
    }
    assert_eq!(phone.live("room").unwrap().field("pointer").field("phone"), Value::Int(5));
    let first = phone.take_outgoing();
    assert!(
        matches!(&first[..], [ClientMsg::Call(c)] if c.input.get("at") == Some(&Value::Int(1))),
        "{first:?}"
    );
    assert!(phone.deadline().is_some(), "a held call wants a pump");
    phone.pump_at(crate::autos::monotonic_ms() + crate::THROTTLE_MS + 1);
    let last = phone.take_outgoing();
    assert!(
        matches!(&last[..], [ClientMsg::Call(c)] if c.input.get("at") == Some(&Value::Int(5))),
        "{last:?}"
    );
    assert!(phone.deadline().is_none());
}
