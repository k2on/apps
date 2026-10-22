//! What the peer's tests share: the demo's configuration, a playlist's
//! input, a push as a session makes it, a device linked through a dial,
//! and pumping it until something holds.

#![allow(dead_code)]

use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use ark::format::Encode;
use ark::frames::Frame;
use ark::log::{Entry, Origin};
use ark::peer::Who;
use ark::schema::Space;
use ark_peer::link::{bell, wait, Dial, Pumped};
use ark_peer::{dev, Config, Disk, Memory, Peer};

pub fn config() -> Config {
    Config::of(Arc::new(ark_dev::demo::demo()))
}

/// `create_playlist`'s input.
pub fn named(name: &str) -> Vec<u8> {
    ark_dev::demo::CreatePlaylist { name: name.into() }.to_bytes()
}

/// `who`'s `local`-th intent calling `func` on `input`, its origin named
/// by its session, pushed: the frame's bytes.
pub fn push(who: &Who, local: i64, func: &str, input: Vec<u8>) -> Vec<u8> {
    let entry = Entry::new(who, Origin::named(&who.session), local, func, input, local);
    Frame::push(Space::Durable, [entry]).enc()
}

/// A device of `token`'s login (or one of nobody's when it proves none)
/// on `disk`, its origin named by its session.
pub fn device(token: &str, mut disk: Box<dyn Disk>) -> Peer {
    let who = dev(token).unwrap_or(dev("x@y").unwrap());
    let origin = Origin::named(&who.session);
    ark_peer::local::preset(&mut *disk, |r| r.origin = origin).unwrap();
    Peer::open(config(), who, disk, false)
}

/// `token`'s device, dialling `url` through `dial`, and its bell.
pub fn linked(token: &str, url: &str, dial: impl Dial + 'static) -> (Peer, Receiver<()>) {
    let mut c = device(token, Box::new(Memory::default()));
    let (wake, rx) = bell();
    c.set_waker(wake);
    c.link.backoff = (5, 20);
    c.set_token(Some(token.into()));
    c.connect_with(url, dial);
    (c, rx)
}

/// Pumps a peer until `f` holds, or panics after two seconds.
pub fn until(c: &mut (Peer, Receiver<()>), what: &str, f: impl Fn(&Peer, &Pumped) -> bool) {
    for _ in 0..400 {
        let p = c.0.pump();
        if f(&c.0, &p) {
            return;
        }
        wait(&c.0, &c.1, Duration::from_millis(5));
    }
    panic!("never: {what}");
}
