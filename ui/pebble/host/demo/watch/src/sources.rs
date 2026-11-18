//! The screen's sources: what each of its layers shows, as functions of
//! what the node holds. The emitter binds the view's paths to them by name
//! (`p.title` is [`title`], a `for t in p.next`'s `t.title` is
//! [`next_title`]), as a table; the toolkit calls them through the node as
//! PebbleOS draws, each layer's reads recorded against it, so a layer is
//! drawn again only when what it read changed.
//!
//! What they read: the playback, a session row the player owns; the
//! queue's first rows, a plan the node watches ([`NEXT`]); the host's
//! status cell ([`STATUS`]); the time, a cell set at each wake ([`NOW`]).
//! Nothing is copied out of them to be compared.

use ark_kernel::codec::from_slice;
use ark_kernel::{Cell, Read, Watch};
use ark_pebble::text::Line;
use ark_pebble::STATUS;
use now_playing::{playing as play, Playing, Track};

/// The tree's time, in milliseconds: the app's first cell, after the
/// host's status.
pub const NOW: Cell<u64> = Cell::at(1);
/// The queue's first rows, as the screen shows them: the node's watch.
pub const NEXT: Watch = Watch::at(2);

fn playing(db: &dyn Read) -> Option<Playing> {
    play(db).map(|p| p.1)
}

/// The link and what waits.
pub fn status(db: &dyn Read, out: &mut Line) {
    let Some(s) = STATUS.get(db) else { return };
    out.s(match (s.online, s.complete) {
        _ if s.unreached => b"Can't Reach Player",
        (false, _) => b"Offline",
        (true, false) => b"Syncing",
        (true, true) => b"Online",
    });
    if s.pending > 0 {
        out.s(b", ").n(s.pending as u64).s(b" Pending");
    }
    if s.refused > 0 {
        out.s(b", ").n(u64::from(s.refused)).s(b" Refused");
    }
}

/// The glyph before the status.
pub fn link(db: &dyn Read) -> &'static [u8] {
    match STATUS.get(db) {
        Some(s) if s.online && s.complete && !s.unreached => b"wifi",
        Some(s) if s.online && !s.unreached => b"refresh-cw",
        _ => b"wifi-off",
    }
}

pub fn title(db: &dyn Read, out: &mut Line) {
    match playing(db) {
        Some(p) => out.s(&p.track.title),
        None => out.s(b"Nothing Playing"),
    };
}

pub fn artist(db: &dyn Read, out: &mut Line) {
    if let Some(p) = playing(db) {
        out.s(&p.track.artist);
    }
}

/// Where playback is now, in milliseconds, and how long the track is.
fn position(db: &dyn Read) -> Option<(u64, u64)> {
    let p = playing(db)?;
    let ms = u64::from(p.track.ms);
    let now = NOW.get(db).copied().unwrap_or_default();
    Some((p.anchor.position(now).min(ms), ms))
}

/// How far along, in thousandths of whole seconds, as the clock under it
/// reads: the watch divides no floats.
pub fn progress(db: &dyn Read) -> u16 {
    let Some((at, ms)) = position(db) else { return 0 };
    let secs = (ms / 1000).max(1) as u32;
    ((at / 1000) as u32 * 1000 / secs) as u16
}

pub fn at(db: &dyn Read, out: &mut Line) {
    if let Some((at, _)) = position(db) {
        out.clock(at);
    }
}

pub fn length(db: &dyn Read, out: &mut Line) {
    if let Some((_, ms)) = position(db) {
        out.clock(ms);
    }
}

fn rows(db: &dyn Read) -> &[(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)] {
    NEXT.get(db).map_or(&[][..], |l| l.rows())
}

/// The queue's heading.
pub fn queue(db: &dyn Read, out: &mut Line) {
    out.s(if rows(db).is_empty() { b"Queue Is Empty" } else { b"Up Next" });
}

/// How many of the queue it shows.
pub fn next(db: &dyn Read) -> usize {
    rows(db).len()
}

fn track(db: &dyn Read, i: usize) -> Option<Track> {
    from_slice(&rows(db).get(i)?.1).ok()
}

pub fn next_title(db: &dyn Read, i: usize, out: &mut Line) {
    if let Some(t) = track(db, i) {
        out.s(&t.title);
    }
}

/// A row's artist and length.
pub fn next_line(db: &dyn Read, i: usize, out: &mut Line) {
    if let Some(t) = track(db, i) {
        out.s(&t.artist).s(b"  ").clock(u64::from(t.ms));
    }
}

/// Select's glyph: pause while playing, else play.
pub fn play_glyph(db: &dyn Read) -> &'static [u8] {
    if playing(db).is_some_and(|p| p.anchor.playing) {
        b"pause"
    } else {
        b"play"
    }
}
