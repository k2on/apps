//! Records the frames the watch's slice (`slice.rs`) is sent, for the
//! probe to embed: run on the build machine, with the kernel's upstream,
//! against a real authority, a phone node and the same slice the probe
//! runs on the watch.
//!
//!   frames OUT
//!
//! - Before the watch connects, the phone makes the queue of sixteen
//!   tracks, its playlist and two albums, logged by the authority.
//! - At the watch's connection (exchange 0), its subscriptions, hello and
//!   pending pushes go to the authority; between its hello and its pushes
//!   the phone takes a track off and renames an album, so the watch's
//!   pushes rebase over them. The authority's clock is 1000 ahead.
//! - Linked (exchange 1), its pause is forwarded; the phone sets its
//!   playback anchor and adds a track that comes into the watch's query.
//!
//! Each exchange's frames for the watch are written after their length,
//! in four bytes; the run panics if the slice did not end right.

#[path = "slice.rs"]
mod slice;

extern crate alloc;

use ark_kernel::codec::{from_slice, put_frame};
use ark_kernel::*;
use slice::Msg;

/// The authority's time when the watch's is 3000.
const SERVER: u64 = 4_000;
const PHONE: Conn = Conn(1);
const WATCH: Conn = Conn(2);

/// What the phone said goes to the authority; what it answers the watch
/// is kept, as frames.
fn to_watch(a: &mut Authority<Msg, Memory>, ups: &mut Vec<Up<Msg>>, now: u64, wire: &mut Vec<u8>) {
    for up in ups.drain(..) {
        for (c, d) in a.receive(PHONE, up, now) {
            if c == WATCH {
                put_frame(wire, &d);
            }
        }
    }
}

fn main() {
    let out = std::env::args().nth(1).expect("frames OUT");
    let mut a = Authority::new(&slice::HANDLERS, Memory::new());
    a.set_queries(&slice::QUERIES);
    // Each frame within what the watch reads at once.
    a.set_part(4);
    a.open(PHONE, Actor(3), Some(Origin(9)));
    a.open(WATCH, Actor(3), Some(Origin(7)));
    let mut phone = slice::node(9);
    let mut ups: Vec<Up<Msg>> = Vec::new();
    phone.connect(&mut ups, 1_000);
    let mut wire = Vec::new();
    for i in 0..16 {
        let e = phone.make(Space::Shared, slice::track(i), 1_000);
        phone.route(e, &mut ups);
    }
    for m in [slice::playlist(), slice::album(1, b'a'), slice::album(2, b'a')] {
        let e = phone.make(Space::Shared, m, 1_000);
        phone.route(e, &mut ups);
    }
    to_watch(&mut a, &mut ups, 1_000, &mut wire);
    wire.clear();

    let mut recorded: Vec<u8> = Vec::new();
    let right = slice::run(&mut |n, said| {
        let mut wire = Vec::new();
        for bytes in said {
            let up: Up<Msg> = from_slice(&bytes).expect("an up frame");
            let hello = matches!(up, Up::Hello { .. });
            for (c, d) in a.receive(WATCH, up, SERVER) {
                if c == WATCH {
                    put_frame(&mut wire, &d);
                }
            }
            if hello {
                // Another device's writes, logged while the watch's are
                // pending: a track off the queue, an album renamed.
                for m in [slice::track(1), slice::album(2, b'b')] {
                    let e = phone.make(Space::Shared, m, SERVER);
                    phone.route(e, &mut ups);
                }
                to_watch(&mut a, &mut ups, SERVER, &mut wire);
            }
        }
        if n == 1 {
            let anchor = Anchor {
                at: 30_000,
                wall: SERVER,
                rate: 1000,
                playing: true,
            };
            let e = phone.make(Space::Session, slice::play(&anchor), SERVER);
            phone.route(e, &mut ups);
            let mut z = slice::track(20);
            if let Msg::Put { value, .. } = &mut z {
                value.fill(b'Z');
            }
            let e = phone.make(Space::Shared, z, SERVER);
            phone.route(e, &mut ups);
            to_watch(&mut a, &mut ups, SERVER, &mut wire);
        }
        recorded.extend((wire.len() as u32).to_le_bytes());
        recorded.extend(&wire);
        wire
    });
    assert!(right, "the watch's slice did not end right over the frames recorded");
    std::fs::write(&out, &recorded).expect("writing the frames");
    println!("the watch's frames: {} bytes, recorded in {out}", recorded.len());
}
