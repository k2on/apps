//! What the scripted tests drive peers with: who each one is, and the
//! frames and changes a step's outputs carry, read back.

#![allow(dead_code)]

use ark::frames::Frame;
use ark::log::Logged;
use ark::peer::{Cas, Input, Journal, Output, Peer, Role, Who};
use ark::store::Change;

pub fn who(name: &str) -> Who {
    Who {
        actor: name.into(),
        session: format!("{name}-session"),
        roles: vec![],
    }
}

pub fn replica(name: &str, cas: Cas) -> Peer {
    Peer::new(super::super::demo::shared(), cas, Role::Replica, who(name))
}

pub fn authority(cas: Cas) -> Peer {
    let role = Role::Authority { journal: Journal::default() };
    Peer::new(super::super::demo::shared(), cas, role, who("server"))
}

fn frames(outs: &[Output], fanout: bool) -> Vec<Frame> {
    outs.iter()
        .filter_map(|o| match o {
            Output::Send(b) if !fanout => Some(Frame::dec(b).unwrap()),
            Output::Fanout(b) if fanout => Some(Frame::dec(b).unwrap()),
            _ => None,
        })
        .collect()
}

pub fn sends(outs: &[Output]) -> Vec<Frame> {
    frames(outs, false)
}

pub fn fanouts(outs: &[Output]) -> Vec<Frame> {
    frames(outs, true)
}

pub fn changes(outs: &[Output]) -> Vec<Change> {
    outs.iter()
        .flat_map(|o| match o {
            Output::Changes(cs) => cs.clone(),
            _ => vec![],
        })
        .collect()
}

/// A frame from upstream, to a replica.
pub fn down(f: &Frame) -> Input {
    Input::Frame { from: None, bytes: f.enc() }
}

/// A frame from a connection verified as `w`, to an authority.
pub fn up(w: &Who, f: &Frame) -> Input {
    Input::Frame {
        from: Some(w.clone()),
        bytes: f.enc(),
    }
}

pub fn mutate(name: &str, input: Vec<u8>, now: i64) -> Input {
    Input::Mutate {
        func: super::super::demo::h(name),
        input,
        now,
    }
}

/// The one entry a batch carries.
pub fn logged(f: &Frame) -> Logged {
    match f {
        Frame::Batch { logged, .. } if logged.len() == 1 => logged[0].clone(),
        other => panic!("not a batch of one: {other:?}"),
    }
}

/// Every frame a step sent upstream delivered to the authority as `w`; what
/// the authority says back, sends then fanouts.
pub fn deliver_up(server: &mut Peer, w: &Who, outs: &[Output]) -> (Vec<Frame>, Vec<Frame>) {
    let (mut s, mut f) = (vec![], vec![]);
    for frame in sends(outs) {
        let o = server.step(up(w, &frame));
        s.extend(sends(&o));
        f.extend(fanouts(&o));
    }
    (s, f)
}
