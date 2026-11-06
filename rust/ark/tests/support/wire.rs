//! What the scripted tests drive peers with: who each one is, and the
//! frames and changes a step's outputs carry, read back.

#![allow(dead_code)]

use ark::frames::{Frame, Verdict};
use ark::log::Logged;
use std::sync::Arc;

use ark::peer::{Ctx, Input, Output, Peer, Role, To};
use ark::store::Change;
use ark::Domain;

pub fn who(name: &str) -> Ctx {
    Ctx::who(name, format!("{name}-session"), vec![])
}

pub fn replica(name: &str, cas: Arc<Domain>) -> Peer {
    Peer::new(cas, Role::Replica, who(name))
}

pub fn authority(cas: Arc<Domain>) -> Peer {
    let role = Role::authority();
    Peer::new(cas, role, who("server"))
}

fn frames(outs: &[Output], fanout: bool) -> Vec<Frame> {
    outs.iter()
        .filter_map(|o| match o {
            Output::Send(To::Back, f) if !fanout => Some(f.clone()),
            Output::Send(To::All, f) if fanout => Some(f.clone()),
            _ => None,
        })
        .collect()
}

pub fn sends(outs: &[Output]) -> Vec<Frame> {
    frames(outs, false)
}

/// Each frame's verdict, `None` for a frame that is no verdict.
pub fn verdicts(fs: &[Frame]) -> Vec<Option<Verdict>> {
    let v = |f: &Frame| match f {
        Frame::Verdict { verdict, .. } => Some(verdict.clone()),
        _ => None,
    };
    fs.iter().map(v).collect()
}

pub fn fanouts(outs: &[Output]) -> Vec<Frame> {
    frames(outs, true)
}

pub fn changes(outs: &[Output]) -> Vec<Change> {
    outs.iter()
        .flat_map(|o| match o {
            Output::Changes(cs) => cs.iter().map(|t| t.change.clone()).collect(),
            _ => vec![],
        })
        .collect()
}

/// A frame from upstream, to a replica.
pub fn down(f: &Frame) -> Input {
    Input::Frame {
        from: None,
        frame: f.clone(),
    }
}

/// A frame from a connection verified as `w`, to an authority.
pub fn up(w: &Ctx, f: &Frame) -> Input {
    Input::Frame {
        from: Some(w.clone()),
        frame: f.clone(),
    }
}

pub fn mutate(name: &str, input: Vec<u8>, now: i64) -> Input {
    Input::Mutate {
        func: super::super::demo::h(name),
        input,
        now,
        cause: None,
    }
}

/// The one entry a page carries.
pub fn logged(f: &Frame) -> Logged {
    match f {
        Frame::Page { logged, .. } if logged.len() == 1 => logged[0].clone(),
        other => panic!("not a batch of one: {other:?}"),
    }
}

/// Every frame a step sent upstream delivered to the authority as `w`; what
/// the authority says back, sends then fanouts.
pub fn deliver_up(server: &mut Peer, w: &Ctx, outs: &[Output]) -> (Vec<Frame>, Vec<Frame>) {
    let (mut s, mut f) = (vec![], vec![]);
    for frame in sends(outs) {
        let o = server.step(up(w, &frame));
        s.extend(sends(&o));
        f.extend(fanouts(&o));
    }
    (s, f)
}
