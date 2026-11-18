//! The probe: a watch program that links what of the kernel a device runs
//! (the codec, the entry, the stores, the node and its frames; not the
//! authority, which only a server links) as an app would, for
//! `checks.kernel-size` to measure at the watch's settings. Its inputs are
//! opaque to the optimiser (`black_box`), so nothing is folded away.
//!
//! Under QEMU (`checks.kernel-qemu`) it runs the watch's slice in small: a
//! queue of sixteen tracks confirmed, eight writes made offline and kept
//! as pending records through a restart, then pushed, rebased over
//! another device's write and confirmed, every frame read in parts, its
//! queue subscribed to at the connection; and two queries kept live over
//! it all, as the watch's screen keeps them:
//! the next four tracks (a bounded take of a filter) and how many there
//! are (a group). It prints the most heap it held and exits 1 past the
//! budget's 24 KB.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "rt.rs"]
mod rt;

use alloc::vec::Vec;
use core::hint::black_box;
use core::ops::Bound;

use ark_kernel::codec::{from_slice, put_frame, to_vec, Error, Reader};
use ark_kernel::plan::Agg;
use ark_kernel::store::Range;
use ark_kernel::*;

#[global_allocator]
static HEAP: rt::Heap<{ 1024 * 1024 }> = rt::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    rt::exit(false)
}

/// An app's body: a row put, and a tap that asks for one.
#[derive(Clone)]
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

/// A row of the watch's slice: a queue's track, its title padded as a
/// real one would be.
fn track(i: u8) -> Msg {
    let mut value = [b' '; 32];
    value[0] = b'A' + i;
    Msg::Put {
        table: 1,
        key: [0, i].into(),
        value: value.into(),
    }
}

/// What a server would say: each entry logged with the writes it made, in
/// frames.
fn logged(wire: &mut Vec<u8>, e: Entry<Msg>, seq: u64, facts: Writes) {
    let id = e.id;
    put_frame(
        wire,
        &Down::Logged(Logged {
            entry: Entry { seq: Some(Seq(seq)), ..e },
            facts,
        }),
    );
    put_frame(
        wire,
        &Down::<Msg>::Verdict {
            id,
            verdict: Verdict::At { seq: Seq(seq), last: id.n },
        },
    );
}

fn main() -> ! {
    let stores = Spaces {
        local: Memory::new(),
        device: Memory::new(),
        session: Memory::new(),
        shared: Memory::new(),
    };
    let mut node = Node::new(Origin(black_box(7)), black_box(0), Version(1), &HANDLERS, stores);
    node.set_actor(Actor(black_box(3)));
    let mut up: Vec<Up<Msg>> = Vec::new();
    let mut wire = Vec::new();

    // The phone's queue of sixteen tracks, confirmed, arriving in parts.
    let mut phone = Node::new(
        Origin(black_box(9)),
        0,
        Version(1),
        &HANDLERS,
        Spaces {
            local: Memory::new(),
            device: Memory::new(),
            session: Memory::new(),
            shared: Memory::new(),
        },
    );
    let mut seq = 0;
    for i in 0..black_box(16u8) {
        let e = phone.make(Space::Shared, track(i), black_box(1_000));
        let facts = Writes(alloc::vec![Write {
            table: TableId(1),
            key: [0, i].into(),
            value: match &e.body {
                Msg::Put { value, .. } => Some(value.clone()),
                _ => None,
            }
        }]);
        seq += 1;
        logged(&mut wire, e, seq, facts);
    }
    // The screen's queries: the next four tracks, and how many there are.
    let all = || scan(TableId(1), Bound::Unbounded, Bound::Unbounded);
    let mut queries = [
        all()
            .filter(|_, v| v[0] != b' ')
            .take(black_box(4), black_box(false))
            .live(&node.view(Space::Shared)),
        all()
            .group(|_, _| Vec::new(), |_, v| v.len() as i64, black_box(false), true)
            .live(&node.view(Space::Shared)),
    ];
    let mut shown = 0;
    let mut watch = |node: &mut Node<Msg, Memory>| {
        let changes = node.take_changes();
        for q in &mut queries {
            shown += q.push(&node.view(Space::Shared), &changes).len();
        }
    };
    let mut frames = Frames::new(black_box(512));
    let mut take = |node: &mut Node<Msg, Memory>, wire: &mut Vec<u8>, up: &mut Vec<Up<Msg>>| {
        for part in wire.chunks(black_box(7)) {
            frames.push(part);
            while let Ok(Some(down)) = frames.read::<Down<Msg>>() {
                node.receive(down, up);
                watch(node);
            }
        }
        wire.clear();
    };
    take(&mut node, &mut wire, &mut up);

    // Offline: eight taps, each a pending write seen at once.
    for key in 0..black_box(8u8) {
        let tap = node.make(Space::Local, Msg::Tap { key }, black_box(2_000));
        node.route(tap, &mut up);
    }
    // A restart: the pending records kept and put back.
    let kept = to_vec(&node.pending().to_vec());
    if let Ok(pending) = from_slice::<Vec<Pending<Msg>>>(&kept) {
        node.resume(node.cursor(), pending);
    }

    // Connected: the pushes go up as frames; the server logs another
    // device's write first, so the rest rebase, then confirms them.
    up.clear();
    node.subscribe(QueryId(black_box(0)), Vec::new(), &mut up);
    up.clear();
    node.connect(&mut up);
    let pushed = to_vec(&up);
    let Ok(pushed) = from_slice::<Vec<Up<Msg>>>(&pushed) else {
        rt::exit(false)
    };
    let other = phone.make(Space::Shared, track(1), black_box(3_000));
    seq += 1;
    logged(
        &mut wire,
        other,
        seq,
        Writes(alloc::vec![Write {
            table: TableId(1),
            key: [0, 1].into(),
            value: None
        }]),
    );
    take(&mut node, &mut wire, &mut up);
    for p in pushed {
        if let Up::Push(e) = p {
            let facts = node
                .pending()
                .iter()
                .find(|p| p.entry.id == e.id)
                .map(|p| p.writes.clone())
                .unwrap_or_default();
            seq += 1;
            logged(&mut wire, e, seq, facts);
        }
    }
    put_frame(
        &mut wire,
        &Down::<Msg>::Got {
            query: QueryId(0),
            args: Vec::new(),
        },
    );
    take(&mut node, &mut wire, &mut up);

    // A scan of the view, as a list on the watch reads it.
    let view = node.view(Space::Shared);
    let lo = [black_box(0u8)];
    view.scan(
        TableId(1),
        Range {
            lo: Bound::Included(&lo),
            hi: Bound::Unbounded,
            reverse: black_box(false),
        },
        &mut |k, v| {
            black_box((k, v));
            true
        },
    );

    // The queries kept live are what a fresh run over the view answers.
    let view = node.view(Space::Shared);
    let next = all().filter(|_, v| v[0] != b' ').take(4, false).live(&view);
    let total = from_slice::<Agg>(&queries[1].rows()[0].1).map_or(0, |a| a.count);
    let right = queries[0].rows() == next.rows() && total == node.store(Space::Shared).len(TableId(1)) as u64;

    black_box((
        shown,
        node.take_refused(),
        node.made(),
        node.clock(),
        node.pending().len(),
        node.store(Space::Shared).len(TableId(1)),
    ));
    // The heap at most, against the kernel's budget of 24 KB.
    let peak = HEAP.peak();
    rt::print("the kernel's probe held at most ");
    rt::print_number(peak);
    rt::print(" bytes of heap, of the 24576 its budget allows\n");
    rt::exit(peak <= 24 * 1024 && node.pending().is_empty() && right && node.complete(QueryId(0), &[]))
}
