//! The event loop: one kernel [`Node`] on the watch, driven by PebbleOS's
//! callbacks, which the C shim passes here (`ark_on_*`).
//!
//! | callback | becomes |
//! |---|---|
//! | a button | the app's entries ([`App::button`]), routed by the node |
//! | AppMessage in | chunks of the upstream's frames, each `Down` into [`Node::receive`] |
//! | the phone's script says its connection | [`Node::connect`]: subscriptions, hello, pending |
//! | the link down, or `connection_service` | [`Node::disconnect`]: session entries for rows it does not own are refused |
//! | the app's timer | [`App::wake`], at the time the app asked: an [`ark_kernel::Anchor`]'s next step |
//! | AppMessage sent or failed | the next chunk, or the same again |
//!
//! After each, the host **settles**: it takes each space's changes from
//! the node ([`Node::take_changes`]) and the refusals, and hands them to
//! the app ([`App::update`]), whose views push them through their queries
//! and redraw only what changed. A change to what the disk keeps (the
//! confirmed rows, the device space, the pending records, the cursor, the
//! count) arms a flush a little later, so a burst of frames is written
//! once; the host flushes at exit too, and at once when the count of
//! entries nears the disk's lease. Time comes from the watch's clock, in
//! milliseconds; there is no tick.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::UnsafeCell;

use ark_kernel::codec::{put_varint, Error, Reader};
use ark_kernel::{
    Actor, Body, Changes, Decode, Down, Encode, Frames, Handler, Node, Origin, Pending, QueryId, Refusal, Seq, Space, Spaces, Store, TableId, Version,
};

use crate::disk::{Disk, Meta};
use crate::link::{AmLink, Wants};
use crate::rows::Rows;
use crate::sys;
use crate::text::Line;
use crate::ui::Button;

/// The largest frame the watch reads: a fill's part is cut to fit.
pub const FRAME_MOST: usize = 4096;
/// How long after a change the disk is written.
const FLUSH_MS: u32 = 400;
/// How long before a refused message is sent again.
const RETRY_MS: u32 = 500;
/// What a message of frames spends on its dictionary: a count, then a
/// header per tuple (the epoch, an int, and the bytes), and some slack.
const MESSAGE_OVERHEAD: usize = 1 + 7 + 4 + 7 + 8;

/// The shim's timers.
mod timer {
    pub const WAKE: u8 = 0;
    pub const FLUSH: u8 = 1;
    pub const RETRY: u8 = 2;
}

/// The link and what waits, for a status line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub online: bool,
    /// The node's shared entries not yet confirmed.
    pub pending: usize,
    /// The entries refused since the app started, but those refused
    /// offline.
    pub refused: u32,
}

/// What an app's code is given: the node, the time, the link.
pub struct Ctx<'a, B: 'static> {
    pub node: &'a mut Node<B, Rows>,
    link: &'a mut AmLink,
    /// The watch's time, in milliseconds.
    pub now: u64,
    wake: &'a mut Option<u64>,
    pub status: Status,
}

impl<B: Body + Clone + Encode + 'static> Ctx<'_, B> {
    /// Makes an entry of the node's now and routes it: stepped here, and
    /// pushed or sent upstream by its space. Refused here (a session
    /// entry for a row the watch does not own, with no link: live
    /// controls don't queue), it says with what.
    pub fn make(&mut self, space: Space, body: B) -> Result<(), Refusal> {
        let e = self.node.make(space, body, self.now);
        let id = e.id;
        self.node.route(e, self.link);
        match self.node.refusals().iter().find(|r| r.0 == id) {
            Some(&(_, why)) => Err(why),
            None => Ok(()),
        }
    }

    /// The tree's time now (the server's clock, as the watch's hellos
    /// measured it): what anchors are extrapolated at.
    pub fn time(&self) -> u64 {
        self.node.time(self.now)
    }

    /// The watch's time at the tree's time `t`: for [`Ctx::wake_at`].
    pub fn local(&self, t: u64) -> u64 {
        t.wrapping_add_signed(self.node.offset().wrapping_neg())
    }

    /// Holds a query's rows, now and at every connection.
    pub fn subscribe(&mut self, query: QueryId, args: Vec<u8>) {
        self.node.subscribe(query, args, self.link);
    }

    /// Asks for [`App::wake`] at `at` (the watch's milliseconds), or never.
    /// An app has one such time; asking again moves it.
    pub fn wake_at(&mut self, at: Option<u64>) {
        *self.wake = at;
    }
}

/// An app on the watch: its entries' body, its handlers, and its screen.
pub trait App: 'static {
    type Body: Body + Clone + Encode + Decode + 'static;

    fn handlers(&self) -> &'static [Handler<Self::Body>];
    fn version(&self) -> Version;
    /// Who this watch's entries are from: its phone's user.
    fn actor(&self) -> Actor;
    /// At start, the node put back from the disk: subscribe, draw.
    fn start(&mut self, cx: &mut Ctx<'_, Self::Body>);
    fn button(&mut self, cx: &mut Ctx<'_, Self::Body>, button: Button);
    /// What changed in each space (by [`Space`] as a number) or in the
    /// status since the last call.
    fn update(&mut self, cx: &mut Ctx<'_, Self::Body>, changes: &[Changes; 4]);
    /// The time asked with [`Ctx::wake_at`] has come.
    fn wake(&mut self, cx: &mut Ctx<'_, Self::Body>);
}

/// The host, as the callbacks reach it.
trait Driver {
    fn button(&mut self, id: u8);
    fn hello(&mut self);
    fn link(&mut self, epoch: u32);
    fn chunk(&mut self, epoch: u32, data: &[u8]);
    fn sent(&mut self);
    fn failed(&mut self);
    fn timer(&mut self, kind: u8);
    fn phone(&mut self, connected: bool);
    fn stop(&mut self);
}

struct Global(UnsafeCell<Option<Box<dyn Driver>>>);

// The watch runs the app on one thread, and the callbacks one at a time.
unsafe impl Sync for Global {}

static HOST: Global = Global(UnsafeCell::new(None));

fn with(f: impl FnOnce(&mut dyn Driver)) {
    if let Some(host) = unsafe { &mut *HOST.0.get() } {
        f(&mut **host);
    }
}

pub struct Host<A: App> {
    node: Node<A::Body, Rows>,
    disk: Disk<sys::Sdk>,
    link: AmLink,
    frames: Frames,
    app: A,
    origin: [u8; 16],
    /// The time the app asked to wake at, and the one the timer is set to.
    wake: Option<u64>,
    armed: Option<u64>,
    flush_armed: bool,
    status: Status,
    refused: u32,
    /// The most of the app heap seen in use, after a callback.
    heap_peak: usize,
}

/// A row's path on the disk: its space, its table, its key.
fn path(out: &mut Vec<u8>, space: Space, table: TableId, key: &[u8]) {
    out.clear();
    out.push(space as u8);
    out.extend_from_slice(&table.0.to_be_bytes());
    out.extend_from_slice(key);
}

/// Starts the host: puts the node back from the disk, opens its link, and
/// starts the app. The C shim calls it, through [`crate::app!`], once its
/// window is up.
///
/// A watchapp's stack is 4 KB, the SDK's frames included, so what is large
/// is built on the heap and the steps that hold much are kept apart.
pub fn start<A: App>(app: A) {
    let mut host = open(app);
    host.link.phone(sys::phone_connected());
    host.cx(|app, cx| app.start(cx));
    host.settle();
    unsafe { *HOST.0.get() = Some(host) };
}

#[inline(never)]
fn open<A: App>(app: A) -> Box<Host<A>> {
    let now = sys::now();
    let mut device = Rows::new();
    let mut shared = Rows::new();
    let (disk, loaded) = Disk::open(sys::Sdk, &mut |p, value| {
        if p.len() < 5 {
            return;
        }
        let table = TableId(u32::from_be_bytes([p[1], p[2], p[3], p[4]]));
        match p[0] {
            1 => device.put(table, &p[5..], value),
            3 => shared.put(table, &p[5..], value),
            _ => {}
        }
    });
    let (meta, pending) = match loaded {
        Some(l) => {
            let pending = records::<A::Body>(&l.pending).unwrap_or_else(|_| {
                crate::log!("the pending records did not read: dropped");
                Vec::new()
            });
            (l.meta, pending)
        }
        None => {
            device = Rows::new();
            shared = Rows::new();
            // An origin drawn once, at the first start, and kept.
            let origin = (u64::from(sys::random()) << 32 | u64::from(sys::random())) ^ now;
            (Meta { origin, ..Meta::default() }, Vec::new())
        }
    };
    let rows = shared.len();
    let stores = Spaces {
        device,
        session: Rows::new(),
        shared,
    };
    let mut node = Node::new(Origin(meta.origin), meta.made, app.version(), app.handlers(), stores);
    node.set_actor(app.actor());
    node.resume(Seq(meta.cursor), pending);
    let mut origin = [0u8; 16];
    origin.copy_from_slice(Line::new().hex(meta.origin).bytes());
    let chunk = (sys::outbox_size() as usize).saturating_sub(MESSAGE_OVERHEAD);
    let host = Box::new(Host {
        node,
        disk,
        link: AmLink::new(chunk),
        frames: Frames::new(FRAME_MOST),
        app,
        origin,
        wake: None,
        armed: None,
        flush_armed: false,
        status: Status::default(),
        refused: 0,
        heap_peak: 0,
    });
    crate::log!(
        "start origin ",
        &origin[..],
        " cursor ",
        meta.cursor,
        " made ",
        meta.made,
        " rows ",
        rows,
        " pending ",
        host.node.pending().len(),
        " values ",
        host.disk.stats.used
    );
    host
}

/// Pending records, as the host flushed them.
fn records<B: Decode>(bytes: &[u8]) -> Result<Vec<Pending<B>>, Error> {
    let mut r = Reader::new(bytes);
    let n = r.count()?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(Pending::decode(&mut r)?);
    }
    Ok(out)
}

impl<A: App> Host<A> {
    fn cx<R>(&mut self, f: impl FnOnce(&mut A, &mut Ctx<'_, A::Body>) -> R) -> R {
        let Host {
            node,
            link,
            app,
            wake,
            status,
            ..
        } = self;
        let mut cx = Ctx {
            node,
            link,
            now: sys::now(),
            wake,
            status: *status,
        };
        f(app, &mut cx)
    }

    /// After a callback: hands the node's changes to the app, arms the
    /// app's timer and a flush, and sends what the link has.
    fn settle(&mut self) {
        let changes = Space::ALL.map(|s| self.node.take_changes(s));
        for (id, why) in self.node.take_refused() {
            // Offline refusals are the app's to show, as `Ctx::make` says
            // them.
            if why != Refusal::OFFLINE {
                self.refused += 1;
            }
            crate::log!("refused entry ", id.n, " with ", why.0);
        }
        let status = Status {
            online: self.link.online(),
            pending: self.node.pending().len(),
            refused: self.refused,
        };
        let changed = changes.iter().any(|c| c.reset || !c.rows.is_empty());
        if changed || status != self.status {
            self.status = status;
            self.cx(|app, cx| app.update(cx, &changes));
        }
        let kept = |s: Space| changes[s as usize].reset || !changes[s as usize].rows.is_empty();
        if kept(Space::Shared) || kept(Space::Device) {
            self.dirty();
        }
        if self.node.made() + 4 >= self.disk.lease() {
            self.flush();
        }
        if self.wake != self.armed {
            let ms = self.wake.map(|t| t.saturating_sub(sys::now()).min(u64::from(u32::MAX - 1)) as u32);
            sys::timer(timer::WAKE, ms);
            self.armed = self.wake;
        }
        self.pump();
        let (used, _) = sys::heap();
        self.heap_peak = self.heap_peak.max(used);
    }

    /// Something the disk keeps changed: a flush, soon.
    fn dirty(&mut self) {
        if !self.flush_armed {
            self.flush_armed = true;
            sys::timer(timer::FLUSH, Some(FLUSH_MS));
        }
    }

    /// Writes what changed to the disk.
    fn flush(&mut self) {
        if self.flush_armed {
            self.flush_armed = false;
            sys::timer(timer::FLUSH, None);
        }
        let meta = Meta {
            origin: self.node.origin().0,
            made: self.node.made(),
            cursor: self.node.cursor().0,
            actor: self.app.actor().0,
        };
        let mut pending = Vec::new();
        put_varint(&mut pending, self.node.pending().len() as u64);
        for p in self.node.pending() {
            p.encode(&mut pending);
        }
        let node = &self.node;
        let rows = |f: &mut crate::disk::Visit<'_>| {
            let mut p = Vec::new();
            for space in [Space::Device, Space::Shared] {
                node.store(space).unwrap().each(&mut |table, key, value| {
                    path(&mut p, space, table, key);
                    f(&p, value);
                });
            }
        };
        let t0 = sys::now();
        match self.disk.flush(meta, &rows, &pending) {
            Ok(0) => {}
            Ok(n) => {
                let (used, free) = sys::heap();
                #[cfg(target_os = "none")]
                let (live, peak) = sys::HEAP.held();
                #[cfg(not(target_os = "none"))]
                let (live, peak) = (0usize, 0usize);
                crate::log!(
                    "flush ",
                    n,
                    " values ",
                    sys::now().saturating_sub(t0),
                    " ms, ",
                    self.disk.stats.used,
                    " kept; heap ",
                    used,
                    " (peak ",
                    self.heap_peak,
                    ") free ",
                    free,
                    "; rust ",
                    live,
                    " (peak ",
                    peak,
                    ")"
                );
            }
            Err(_) => crate::log!("flush refused by the storage; kept in memory"),
        }
    }

    /// Sends what the link has next, if the outbox is free.
    fn pump(&mut self) {
        let ok = match self.link.wants() {
            Wants::Nothing => return,
            Wants::Ready => sys::send_ready(sys::inbox_size(), &self.origin),
            Wants::Chunk { epoch, bytes } => sys::send_chunk(epoch, bytes),
        };
        if ok {
            self.link.sending();
        } else {
            sys::timer(timer::RETRY, Some(RETRY_MS));
        }
    }
}

impl<A: App> Driver for Host<A> {
    fn button(&mut self, id: u8) {
        let Some(button) = Button::from(id) else { return };
        if self.node.made() + 8 >= self.disk.lease() {
            self.flush();
        }
        self.cx(|app, cx| app.button(cx, button));
        // An entry made offline must outlive a restart: its record is
        // written with the next flush.
        self.dirty();
        self.settle();
    }

    fn hello(&mut self) {
        self.link.hello();
        self.pump();
    }

    fn link(&mut self, epoch: u32) {
        if self.link.epoch_set(epoch) {
            self.frames = Frames::new(FRAME_MOST);
            self.node.connect(&mut self.link, sys::now());
            crate::log!(
                "link up, epoch ",
                epoch,
                ", cursor ",
                self.node.cursor().0,
                ", pending ",
                self.node.pending().len()
            );
        } else {
            self.node.disconnect();
            crate::log!("link down");
        }
        self.settle();
    }

    fn chunk(&mut self, epoch: u32, data: &[u8]) {
        if !self.link.current(epoch) {
            return;
        }
        self.link.stats.received += data.len() as u32;
        self.frames.push(data);
        loop {
            match self.frames.read::<Down<A::Body>>() {
                Ok(Some(down)) => {
                    let welcome = matches!(down, Down::Welcome { .. });
                    self.node.receive(down, &mut self.link, sys::now());
                    if welcome {
                        crate::log!("welcome: the server's clock is ", self.node.offset(), " ms ahead");
                    }
                }
                Ok(None) => break,
                Err(Error::TooLong) => {
                    crate::log!("a frame past ", FRAME_MOST, " bytes: the stream is dropped");
                    self.frames = Frames::new(FRAME_MOST);
                    break;
                }
                // A whole frame that is not a value is skipped.
                Err(_) => crate::log!("a frame did not read"),
            }
        }
        self.dirty();
        self.settle();
    }

    fn sent(&mut self) {
        self.link.sent();
        self.pump();
    }

    fn failed(&mut self) {
        self.link.failed();
        sys::timer(timer::RETRY, Some(RETRY_MS));
    }

    fn timer(&mut self, kind: u8) {
        match kind {
            timer::WAKE => {
                self.armed = None;
                self.wake = None;
                self.cx(|app, cx| app.wake(cx));
                self.settle();
            }
            timer::FLUSH => {
                self.flush_armed = false;
                self.flush();
            }
            _ => self.pump(),
        }
    }

    fn phone(&mut self, connected: bool) {
        if self.link.phone(connected) {
            self.node.disconnect();
            crate::log!("link down: the phone went");
        }
        self.settle();
    }

    fn stop(&mut self) {
        self.flush();
        let d = self.disk.stats;
        let l = self.link.stats;
        crate::log!(
            "stop: ",
            d.flushes,
            " flushes, ",
            d.writes,
            " values, ",
            d.bytes,
            " B; up ",
            l.sent,
            " B, down ",
            l.received,
            " B, ",
            l.connects,
            " links; heap peak ",
            self.heap_peak
        );
    }
}

// The shim's callbacks.

#[no_mangle]
pub extern "C" fn ark_on_button(id: u8) {
    with(|h| h.button(id));
}

#[no_mangle]
pub extern "C" fn ark_on_hello() {
    with(|h| h.hello());
}

#[no_mangle]
pub extern "C" fn ark_on_link(epoch: u32) {
    with(|h| h.link(epoch));
}

/// # Safety
/// `data` is `len` bytes the shim holds for the call.
#[no_mangle]
pub unsafe extern "C" fn ark_on_chunk(epoch: u32, data: *const u8, len: usize) {
    if data.is_null() {
        return;
    }
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };
    with(|h| h.chunk(epoch, bytes));
}

#[no_mangle]
pub extern "C" fn ark_on_sent() {
    with(|h| h.sent());
}

#[no_mangle]
pub extern "C" fn ark_on_failed() {
    with(|h| h.failed());
}

#[no_mangle]
pub extern "C" fn ark_on_timer(kind: u8) {
    with(|h| h.timer(kind));
}

#[no_mangle]
pub extern "C" fn ark_on_phone(connected: bool) {
    with(|h| h.phone(connected));
}

#[no_mangle]
pub extern "C" fn ark_on_stop() {
    with(|h| h.stop());
}
