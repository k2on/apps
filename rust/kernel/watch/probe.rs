//! The size probe: a watch program that links the kernel's whole public
//! surface (the codec, the entry, both stores and the node) as an app
//! would, for `checks.kernel-size` to measure at the watch's settings. Its
//! inputs are opaque to the optimiser (`black_box`), so nothing is folded
//! away; under QEMU it runs and exits 0.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "rt.rs"]
mod rt;

use alloc::vec::Vec;
use core::hint::black_box;
use core::ops::Bound;

use ark_kernel::codec::{from_slice, put_frame, to_vec, Error, Reader};
use ark_kernel::store::Range;
use ark_kernel::*;

#[global_allocator]
static HEAP: rt::Bump<{ 32 * 1024 }> = rt::Bump::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    rt::exit(false)
}

/// An app's body: a row put, and a tap that asks for one.
enum Msg {
    Put { table: u32, key: Vec<u8>, value: Vec<u8> },
    Tap { key: u8 },
}

impl Body for Msg {
    fn handler(&self) -> HandlerId {
        HandlerId(match self {
            Msg::Put { .. } => 0,
            Msg::Tap { .. } => 1,
        })
    }
}

impl Encode for Msg {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Msg::Put { table, key, value } => {
                0u32.encode(out);
                table.encode(out);
                key.encode(out);
                value.encode(out);
            }
            Msg::Tap { key } => {
                1u32.encode(out);
                key.encode(out);
            }
        }
    }
}

impl Decode for Msg {
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        match u32::decode(r)? {
            0 => Ok(Msg::Put {
                table: u32::decode(r)?,
                key: Vec::decode(r)?,
                value: Vec::decode(r)?,
            }),
            1 => Ok(Msg::Tap { key: u8::decode(r)? }),
            _ => Err(Error::Tag),
        }
    }
}

static HANDLERS: [Handler<Msg>; 2] = [
    |cx| {
        let Msg::Put { table, key, value } = &cx.entry.body else {
            return Err(Refusal(256));
        };
        let mut n = 0;
        cx.db.scan(TableId(*table), Range::all(), &mut |_, _| {
            n += 1;
            n < 100
        });
        if cx.db.get(TableId(*table), key).as_deref() == Some(value) {
            cx.db.delete(TableId(*table), key);
        } else {
            cx.db.put(TableId(*table), key, value);
        }
        Ok(())
    },
    |cx| {
        let Msg::Tap { key } = cx.entry.body else { return Err(Refusal(256)) };
        cx.emit(
            Space::Shared,
            Msg::Put {
                table: 1,
                key: [key].into(),
                value: [1].into(),
            },
        );
        cx.emit(
            Space::Local,
            Msg::Put {
                table: 2,
                key: [key].into(),
                value: [2].into(),
            },
        );
        Ok(())
    },
];

fn main() -> ! {
    let stores = Spaces {
        local: Memory::new(),
        device: Memory::new(),
        session: Memory::new(),
        shared: Memory::new(),
    };
    let mut node = Node::new(Origin(black_box(7)), black_box(0), Version(1), &HANDLERS, stores);
    node.set_actor(Actor(black_box(3)));
    let mut sent: Vec<Entry<Msg>> = Vec::new();
    for key in 0..black_box(4u8) {
        let tap = node.make(Space::Local, Msg::Tap { key }, black_box(1_000));
        node.route(tap, &mut sent);
    }

    // What was sent comes back through the wire, in parts, confirmed.
    let mut wire = Vec::new();
    for (i, e) in sent.drain(..).enumerate() {
        put_frame(
            &mut wire,
            &Entry {
                seq: Some(Seq(i as u64 + 1)),
                ..e
            },
        );
    }
    let mut frames = Frames::new(black_box(512));
    for part in wire.chunks(black_box(5)) {
        frames.push(part);
        while let Ok(Some(e)) = frames.read::<Entry<Msg>>() {
            node.receive(e, &mut sent);
        }
    }

    // Writes as facts would travel, applied to a copy.
    let mut copy = Memory::new();
    let mut over = Overlay::new(node.store(Space::Shared));
    over.delete(TableId(1), &[black_box(0)]);
    let lo = [black_box(1u8)];
    over.scan(
        TableId(1),
        Range {
            lo: Bound::Included(&lo),
            hi: Bound::Unbounded,
            reverse: true,
        },
        &mut |k, v| {
            black_box((k, v));
            true
        },
    );
    let writes = over.into_writes();
    if let Ok(w) = from_slice::<Writes>(&to_vec(&writes)) {
        copy.apply(&w);
    }

    black_box((node.take_refused(), node.made(), node.clock(), copy.len(TableId(1)), frames.buffered()));
    rt::exit(true)
}
