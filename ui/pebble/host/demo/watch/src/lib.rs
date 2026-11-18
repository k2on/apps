//! The demo watchapp's Rust, standing in for harken's now playing on the
//! Pebble host: what is playing, where it is, and the next few in the
//! queue, offline and with the phone (here, the relay script and the test
//! upstream) as its upstream.
//!
//! - Playback is the session row the player owns ([`now_playing::PLAYBACK`]):
//!   an anchor the screen extrapolates, redrawn once a second through
//!   [`ark_kernel::Anchor::next`], never by a tick.
//! - The queue's first rows come through the subscription
//!   ([`now_playing::NEXT`]); the list shows the first three of them, a
//!   plan the node watches ([`sources::NEXT`]).
//! - Select toggles play: a session entry setting playback whole at its
//!   position now. Down skips: a shared `Pop` of the next track, pending
//!   until the server confirms it (kept through a restart), and a session
//!   entry playing it from the start.
//! - Playback is the player's row, so with no link the kernel refuses the
//!   watch's toggle (live controls don't queue): nothing changes on the
//!   screen but the status line, which says it can't reach the player,
//!   and nothing fires later. A skip offline still takes the track off the
//!   queue, pending. Each hello's answer brings the player's latest, so
//!   the screen agrees with the player once the link is back.
//! - Times are the server's: the screen extrapolates the anchor at the
//!   tree's time ([`Ctx::time`]), the watch's clock corrected by the offset
//!   its hellos measured.
//! - The screen is a `#[view]` (`now-playing-view`), written out as
//!   Pebble's layers by this crate's build (`build.rs`, ui/docs/pebble.md)
//!   into [`screen`]: its layers and a table binding each to a source of
//!   [`sources`]. No record is made: the node settles, the toolkit draws
//!   again what read what changed, and asks the sources as it draws.

#![no_std]

extern crate alloc;

pub mod sources;

use ark_kernel::codec::from_slice;
use ark_kernel::{Actor, Anchor, Handler, Space, Version};
use ark_pebble::tokens::BACKGROUND;
use ark_pebble::{App, Button, Ctx};
use now_playing::{place, playing, upcoming, Act, Msg, Playing, Track, HANDLERS, NEXT, VERSION};

/// The screen's layers and their bindings, written at build time from its
/// `#[view]`.
pub mod screen {
    include!(concat!(env!("OUT_DIR"), "/screen.rs"));
}

/// How many of the queue the list shows.
const SHOWN: usize = 3;

struct NowPlaying;

ark_pebble::app!(NowPlaying);

/// When the clock next changes on the screen, as the watch's time: a wake.
fn next_wake(cx: &mut Ctx<'_, Msg>) {
    let Some((_, p)) = playing(&cx.node.view(Space::Session)) else {
        return cx.wake_at(None);
    };
    let now = cx.time();
    let at = p.anchor.position(now);
    let next = p.anchor.next(now, 1000).filter(|_| at < u64::from(p.track.ms));
    let wake = next.map(|t| cx.local(t));
    cx.wake_at(wake);
}

impl App for NowPlaying {
    type Body = Msg;

    fn handlers(&self) -> &'static [Handler<Msg>] {
        &HANDLERS
    }

    fn version(&self) -> Version {
        VERSION
    }

    fn actor(&self) -> Actor {
        // The phone's user; the relay signs the watch in as no one else.
        Actor(1)
    }

    fn start(&mut self, cx: &mut Ctx<'_, Msg>) {
        let now = cx.time();
        assert_eq!(cx.node.cell(now), sources::NOW);
        assert_eq!(cx.node.watch(NEXT.0, &[], || upcoming(SHOWN)), sources::NEXT);
        cx.subscribe(NEXT, alloc::vec::Vec::new());
        cx.screen(screen::GLYPHS, BACKGROUND, screen::build, &screen::BINDINGS);
    }

    fn button(&mut self, cx: &mut Ctx<'_, Msg>, button: Button) {
        let now = cx.time();
        let Some(&(_, act)) = screen::PRESS.iter().find(|p| p.0 == button) else {
            return;
        };
        let playing = playing(&cx.node.view(Space::Session));
        match act {
            Act::Toggle => {
                let Some((player, p)) = playing else { return };
                let anchor = Anchor {
                    at: p.anchor.position(now).min(u64::from(p.track.ms)),
                    wall: now,
                    rate: p.anchor.rate,
                    playing: !p.anchor.playing,
                };
                let playing = Playing { track: p.track, anchor };
                let _ = cx.make(Space::Session, Msg::Play { player, playing });
            }
            Act::Skip => {
                let first = cx.node.answer(sources::NEXT).and_then(|l| l.rows().first().cloned());
                let Some((key, value)) = first else { return };
                let (Some(at), Ok(track)) = (place(&key), from_slice::<Track>(&value)) else {
                    return;
                };
                // Pending until the server confirms it, offline or not.
                let _ = cx.make(Space::Shared, Msg::Pop { at });
                let Some((player, _)) = playing else { return };
                let anchor = Anchor {
                    at: 0,
                    wall: now,
                    rate: 1000,
                    playing: true,
                };
                let playing = Playing { track, anchor };
                let _ = cx.make(Space::Session, Msg::Play { player, playing });
            }
        }
    }

    fn wake(&mut self, _: &mut Ctx<'_, Msg>) {}

    /// The time, as the clock's layers read it, and the next wake.
    fn settled(&mut self, cx: &mut Ctx<'_, Msg>) {
        let now = cx.time();
        cx.node.set(sources::NOW, now);
        next_wake(cx);
    }
}
