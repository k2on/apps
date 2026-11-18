//! The watch's slice in small, as both sides run it: the app (its body,
//! handlers and queries) and what the watch does, given the frames its
//! upstream sends. The probe (`probe.rs`) runs it on the watch over frames
//! recorded at build time; `frames.rs` runs it on the build machine against
//! a real authority and phone node, and records those frames. The same
//! code on both sides, so the watch's entries are the very ones the frames
//! answer.
//!
//! What it does:
//! - eight taps made offline, each a pending write and a cell's count,
//!   kept as pending records through a restart;
//! - three queries subscribed to, and a pause of the phone's player made
//!   offline (refused: live controls don't queue);
//! - connected: its subscriptions, hello and pushes go up, encoded; the
//!   answer is a fill of what the queries read, another device's writes
//!   while its own are pending (so they rebase), its pushes confirmed, and
//!   the hello's welcome with the server's clock;
//! - linked: a pause sent; the phone's playback anchor and a track that
//!   came into its query arrive;
//! - three plans watched all the while (the next four tracks with their
//!   albums, each playlist with its first two tracks, how many tracks there
//!   are), two readers drawing what they read, every frame read in parts.

#![allow(dead_code)]

use alloc::vec::Vec;
use core::hint::black_box;
use core::ops::Bound;

use ark_kernel::codec::{from_slice, to_vec, Error};
use ark_kernel::plan::Agg;
use ark_kernel::store::Range;
use ark_kernel::*;

/// An app's body: a row put, and a tap that asks for one.
#[derive(Clone, Debug)]
pub enum Msg {
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
    fn decode(r: &mut ark_kernel::codec::Reader<'_>) -> Result<Self, Error> {
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

pub static HANDLERS: [Handler<Msg>; 2] = [
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
        // The screen's state: how many taps, a cell.
        let taps = cx.get(TAPS).copied().unwrap_or(0);
        cx.set(TAPS, taps + 1);
        Ok(())
    },
];

/// The tracks, by a queue's number and their own.
pub const TRACKS: TableId = TableId(1);
/// The session table of the phone's playback.
pub const PLAYBACK: TableId = TableId(3);
/// The table of albums, by a track's number.
pub const ALBUMS: TableId = TableId(4);
/// The table of playlists, whose tracks are the queue's rows under its key.
pub const PLAYLISTS: TableId = TableId(5);

fn every(table: TableId) -> Plan {
    scan(table, Bound::Unbounded, Bound::Unbounded)
}

/// What the watch subscribes to: the tracks, the playlists, the albums.
pub static QUERIES: [Query; 3] = [|_| every(TRACKS), |_| every(PLAYLISTS), |_| every(ALBUMS)];

/// A row of the watch's slice: a queue's track, its title padded as a
/// real one would be.
pub fn track(i: u8) -> Msg {
    let mut value = [b' '; 32];
    value[0] = b'A' + i;
    Msg::Put {
        table: TRACKS.0,
        key: [0, i].into(),
        value: value.into(),
    }
}

pub fn playlist() -> Msg {
    Msg::Put {
        table: PLAYLISTS.0,
        key: [0].into(),
        value: b"Queue".into(),
    }
}

/// A track's album, named by one letter.
pub fn album(i: u8, name: u8) -> Msg {
    Msg::Put {
        table: ALBUMS.0,
        key: [i].into(),
        value: [name].into(),
    }
}

/// The phone's playback, as a session row set whole.
pub fn play(anchor: &Anchor) -> Msg {
    Msg::Put {
        table: PLAYBACK.0,
        key: [0].into(),
        value: to_vec(anchor),
    }
}

/// A node of `origin` with no rows, for the user `3`.
pub fn node(origin: u64) -> Node<Msg, Memory> {
    let stores = Spaces {
        device: Memory::new(),
        session: Memory::new(),
        shared: Memory::new(),
    };
    let mut n = Node::new(Origin(origin), 0, Version(1), &HANDLERS, stores);
    n.set_actor(Actor(3));
    n
}

/// The phone's playback, decoded from its session row when read: a plain
/// function, its reads recorded against whatever reader calls it.
fn playing(db: &dyn Read) -> Option<Anchor> {
    db.get(PLAYBACK, &[0]).and_then(|v| from_slice::<Anchor>(&v).ok())
}

/// The screen's one cell, the node's first source.
const TAPS: Cell<u8> = Cell::at(0);

/// What the screen's sink is told: its readers to redraw, and how many
/// patches its lists took.
#[derive(Default)]
struct Screen {
    dirty: Vec<Reader>,
    shown: usize,
}

impl Sink for Screen {
    fn dirty(&mut self, r: Reader) {
        self.dirty.push(r);
    }

    fn patches(&mut self, _: Watch, p: &[Patch]) {
        self.shown += p.len();
    }
}

/// The watch's two exchanges with its upstream: what it says (each `Up`
/// encoded) at its connection (0) and once linked (1), answered with the
/// frames the upstream sends it, as bytes.
pub type Exchange<'a> = &'a mut dyn FnMut(u8, Vec<Vec<u8>>) -> Vec<u8>;

/// Runs the watch's slice: whether all it shows is right at the end.
pub fn run(exchange: Exchange<'_>) -> bool {
    let mut node = node(black_box(7));
    node.cell(0u8);
    let mut up: Vec<Up<Msg>> = Vec::new();

    // The screen's queries: the next four tracks, each with its album, and
    // how many there are.
    let all = || every(TRACKS);
    let next = || {
        all()
            .filter(|_, v| v[0] != b' ')
            .map_with(|db, k, v| {
                let mut v = v.to_vec();
                v.extend(db.get(ALBUMS, &k[1..]).unwrap_or_default());
                v
            })
            .take(black_box(4), black_box(false))
    };
    // The playlists, each with its first two tracks.
    let lists = || every(PLAYLISTS).related(|k, _| prefix(TRACKS, k).take(2, false));
    let watches = [
        node.watch(0, &[], next),
        node.watch(1, &[], lists),
        node.watch(2, &[], || all().group(|_, _| Vec::new(), |_, v| v.len() as i64)),
    ];
    // The screen's leaves: the track's position, and the list.
    let (at, list) = (node.reader(), node.reader());
    // What a leaf shows, pulled as it draws.
    let draw = move |node: &mut Node<Msg, Memory>, r: Reader| {
        node.read(r, |db| {
            if r == at {
                playing(db).map_or(0, |a| a.at as usize) + usize::from(TAPS.get(db).copied().unwrap_or(0))
            } else {
                watches[0].get(db).map_or(0, |l| l.rows().len())
            }
        })
    };
    let mut drawn = draw(&mut node, at) + draw(&mut node, list);
    let mut screen = Screen::default();
    let mut frames = Frames::new(black_box(512));
    // Frames from upstream, read in parts as a link hands them over, each
    // received, settled and drawn.
    let mut take = |node: &mut Node<Msg, Memory>, wire: Vec<u8>, up: &mut Vec<Up<Msg>>| {
        for part in wire.chunks(black_box(7)) {
            frames.push(part);
            while let Ok(Some(down)) = frames.read::<Down<Msg>>() {
                node.receive(down, up, black_box(3_000));
                node.settle(&mut screen);
                for r in core::mem::take(&mut screen.dirty) {
                    drawn += draw(node, r);
                }
            }
        }
    };
    // What goes up, encoded as the link sends it.
    let said = |up: &mut Vec<Up<Msg>>| up.drain(..).map(|u| to_vec(&u)).collect::<Vec<_>>();

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
    up.clear();
    for q in 0..black_box(3) {
        node.subscribe(QueryId(q), Vec::new(), &mut up);
    }
    // A pause while offline: the phone's row, refused, never sent.
    let pause = play(&Anchor::default());
    let refused = node.make(Space::Session, pause.clone(), black_box(2_500));
    node.route(refused, &mut up);

    // Connected: subscriptions, hello and pushes go up; the answer comes.
    node.connect(&mut up, black_box(3_000));
    let wire = exchange(0, said(&mut up));
    take(&mut node, wire, &mut up);

    // Linked, a session entry is sent; then the phone's playback, and a
    // track that came into its query.
    let control = node.make(Space::Session, pause, black_box(3_000));
    node.route(control, &mut up);
    let linked = up.len() == 1;
    let wire = exchange(1, said(&mut up));
    take(&mut node, wire, &mut up);

    // A scan of the view, as a list on the watch reads it.
    let view = node.view(Space::Shared);
    let lo = [black_box(0u8)];
    view.scan(
        TRACKS,
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
    let [Some(next_), Some(lists_), Some(group)] = watches.map(|w| node.answer(w)) else {
        return false;
    };
    let total = from_slice::<Agg>(&group.rows()[0].1).map_or(0, |a| a.count);
    let right = next_.rows() == next().live(&view).rows()
        && lists_.rows() == lists().live(&view).rows()
        && lists_.rows().len() == 3
        && next_.rows().iter().any(|(k, v)| k[..] == [0, 2] && v.last() == Some(&b'b'))
        && total == node.store(Space::Shared).map_or(0, |s| s.len(TRACKS)) as u64
        && view.get(TRACKS, &[0, 20]).is_some();
    // The server's clock is 1000 ahead of the watch's: the phone's anchor,
    // set at its 4000 from 30 s, is two and a half seconds on at the
    // watch's 5500, and the screen's seconds next change at the tree's 7000.
    let now = node.time(black_box(5_500));
    let right = right
        && node.offset() == 1000
        && playing(&node.view(Space::Session)).is_some_and(|a| a.position(now) == 32_500 && a.next(now, 1000) == Some(7_000));
    let right = right && node.get(TAPS) == Some(&8) && drawn > 0 && linked && node.refusals().len() == 1;

    black_box((screen.shown, node.take_refused(), node.made(), node.clock(), node.pending().len()));
    right && node.pending().is_empty() && (0..3).all(|q| node.complete(QueryId(q), &[]))
}
