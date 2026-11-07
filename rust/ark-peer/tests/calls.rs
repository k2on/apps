//! A device calls one of its server's own functions over its link
//! (`ServerConfig::calls`): a `Call` frame answered by a `Reply` to that
//! connection alone, and nothing of it in either log, on another
//! connection, or kept to send again.

use std::sync::{mpsc, Arc};
use std::time::Duration;

use ark::frames::{Frame, Reply};
use ark_peer::{dev, Authority, Call, Config, Handle, Memory, Peer, ServerConfig};
use arkdb::demo::demo;

fn config() -> Config {
    Config::of(Arc::new(demo()))
}

fn hub() -> ServerConfig {
    let mut c = ServerConfig::of(Arc::new(demo()));
    let echo: Call = Arc::new(|who, input| match input {
        b"no" => Err(format!("{} may not", who.actor)),
        _ => Ok([who.actor.as_bytes(), b":", input].concat()),
    });
    c.calls.insert("echo".into(), echo);
    c
}

/// Falsified by routing a call to the log's machine like any frame: the
/// hub would have no call to take, and the log would move.
#[test]
fn a_call_is_answered_to_its_caller_alone_and_logged_nowhere() {
    let mut hub = Authority::serve(hub(), Box::new(Memory::default()), 0).unwrap();
    let mut d = Peer::open(config(), dev("alice@phone").unwrap(), Box::new(Memory::default()));
    let other = Peer::open(config(), dev("bob@phone").unwrap(), Box::new(Memory::default()));
    let c = hub.accept(d.me().clone());
    let seen = hub.accept(other.me().clone());
    d.connected();
    for b in d.take_frames() {
        let _ = hub.frame(c, b, 1);
    }
    let head = hub.log.log().head();

    let id = d.request("echo", b"hunter2".to_vec()).unwrap();
    let mut out = vec![];
    for b in d.take_frames() {
        out.extend(hub.frame(c, b, 2));
    }
    assert!(out.is_empty(), "nothing is said of a call until it is answered");
    let calls = hub.take_calls();
    assert_eq!(calls.len(), 1);
    let (call, f) = &calls[0];
    assert_eq!((call.who.actor.as_str(), call.name.as_str(), call.id), ("alice", "echo", id));
    let answer = f.as_ref().expect("the server has echo")(&call.who, &call.input);
    let out = hub.reply(call.conn, call.id, answer);

    assert_eq!(hub.log.log().head(), head, "the log did not move");
    assert!(out.iter().all(|(to, _)| *to == c && *to != seen), "{out:?}");
    assert_eq!(d.take_reply(id), None, "not until it arrives");
    for (_, b) in out {
        d.receive(b);
    }
    assert_eq!(d.take_reply(id), Some(Ok(b"alice:hunter2".to_vec())));
    assert_eq!(d.take_reply(id), None, "told once");
}

#[test]
fn a_function_the_server_has_not_is_none() {
    let mut hub = Authority::serve(hub(), Box::new(Memory::default()), 0).unwrap();
    let mut d = Peer::open(config(), dev("alice@phone").unwrap(), Box::new(Memory::default()));
    let c = hub.accept(d.me().clone());
    d.connected();
    d.take_frames();
    d.request("nothing", vec![]).unwrap();
    for b in d.take_frames() {
        let _ = hub.frame(c, b, 1);
    }
    let calls = hub.take_calls();
    assert!(calls[0].1.is_none());
}

/// A call goes out on a connection that is up or not at all; one the
/// connection dropped is answered so, never sent again.
#[test]
fn a_call_is_never_kept_to_send_later() {
    let mut d = Peer::open(config(), dev("alice@phone").unwrap(), Box::new(Memory::default()));
    assert!(d.request("echo", vec![]).is_err(), "not linked");
    d.connected();
    d.take_frames();
    let id = d.request("echo", vec![]).unwrap();
    d.disconnected();
    assert!(d.take_reply(id).unwrap().unwrap_err().contains("closed before it answered"));
    d.connected();
    let again = d.take_frames();
    assert!(
        !again.iter().any(|b| matches!(Frame::dec(b), Ok(Frame::Call { .. }))),
        "not sent again on the next connection"
    );
}

/// The server's thread runs each call on a thread of its own and answers
/// the connection: a refusal is said as one, and a function it has not is
/// refused by name.
#[test]
fn the_servers_thread_runs_calls_and_answers() {
    let handle = Handle::spawn(Authority::serve(hub(), Box::new(Memory::default()), 0).unwrap());
    let (tx, rx) = mpsc::channel();
    let c = handle.connect(dev("alice@phone").unwrap(), Box::new(move |b| tx.send(b).is_ok()));
    let call = |id, name: &str, input: &[u8]| {
        Frame::Call {
            id,
            name: name.into(),
            input: input.to_vec(),
        }
        .enc()
    };
    handle.frame(c, call(1, "echo", b"x"));
    handle.frame(c, call(2, "echo", b"no"));
    handle.frame(c, call(3, "nothing", b""));
    let mut replies = std::collections::BTreeMap::new();
    while replies.len() < 3 {
        let b = rx.recv_timeout(Duration::from_secs(5)).expect("answered");
        if let Ok(Frame::Reply { id, reply }) = Frame::dec(&b) {
            replies.insert(id, reply);
        }
    }
    assert_eq!(replies[&1], Reply::Ok { output: b"alice:x".to_vec() });
    assert_eq!(replies[&2], Reply::Refused { why: "alice may not".into() });
    assert_eq!(
        replies[&3],
        Reply::Refused {
            why: "this server has no function nothing".into()
        }
    );
    handle.stop();
}

/// A call is sent as it is made, as a change is: the server runs it with
/// the device not pumped since. Falsified by queuing the frame for the
/// next pump, when a window waits for an answer that never left.
#[test]
fn a_call_goes_out_as_it_is_made() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let ran = Arc::new(AtomicBool::new(false));
    let mut server = hub();

    let seen = ran.clone();
    let mark: Call = Arc::new(move |_, _| {
        seen.store(true, Ordering::SeqCst);
        Ok(vec![])
    });
    server.calls.insert("mark".into(), mark);
    let hub = Handle::spawn(Authority::serve(server, Box::new(Memory::default()), 0).unwrap());
    let mut d = Peer::open(config(), dev("alice@phone").unwrap(), Box::new(Memory::default()));
    d.set_token(Some("alice@phone".into()));
    d.connect_with("", hub.dialer(Arc::new(dev)));
    let up = ark_peer::link::until(Duration::from_secs(5), Duration::from_millis(5), || {
        d.pump();
        d.linked
    });
    assert!(up, "linked");
    let id = d.request("mark", vec![]).unwrap();
    let ran_unpumped = ark_peer::link::until(Duration::from_secs(5), Duration::from_millis(5), || ran.load(Ordering::SeqCst));
    assert!(ran_unpumped, "the call reached the server without another pump");
    let answered = ark_peer::link::until(Duration::from_secs(5), Duration::from_millis(5), || {
        d.pump();
        d.take_reply(id).is_some()
    });
    assert!(answered);
    hub.stop();
}
