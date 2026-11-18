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
//!   take over the node's view kept live by its changes.
//! - Select toggles play: a session entry setting playback whole at its
//!   position now. Down skips: a shared `Pop` of the next track, pending
//!   until the server confirms it (kept through a restart), and a session
//!   entry playing it from the start.
//! - Offline, only the `Pop` is made. Playback is the player's, and a
//!   session entry is never kept or sent again: one made with no link
//!   would show here what the player never heard, and an upstream's hello
//!   does not say a node's own latest back to it.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use ark_kernel::codec::from_slice;
use ark_kernel::{Actor, Anchor, Changes, Handler, Live, Space, Version};
use ark_pebble::text::Line;
use ark_pebble::{App, Button, Ctx, Glyph, Screen, Slot};
use now_playing::{place, playing, upcoming, Msg, Playing, Track, HANDLERS, NEXT, PLAYBACK, VERSION};

/// How many of the queue the list shows.
const SHOWN: usize = 3;

struct NowPlaying {
    screen: Screen,
    next: Option<Live>,
    playing: Option<Playing>,
}

ark_pebble::app!(NowPlaying {
    screen: Screen::new(),
    next: None,
    playing: None,
});

impl NowPlaying {
    fn draw_track(&mut self, cx: &mut Ctx<'_, Msg>) {
        match &self.playing {
            Some(p) => {
                self.screen.text(Slot::Title, &p.track.title);
                self.screen.text(Slot::Subtitle, &p.track.artist);
                let glyph = if p.anchor.playing { Glyph::Pause } else { Glyph::Play };
                self.screen.action(Button::Select, glyph);
            }
            None => {
                self.screen.text(Slot::Title, b"Nothing Playing");
                self.screen.text(Slot::Subtitle, b"");
                self.screen.action(Button::Select, Glyph::Play);
            }
        }
        self.draw_time(cx);
    }

    /// Where playback is, and when that next changes on the screen.
    fn draw_time(&mut self, cx: &mut Ctx<'_, Msg>) {
        let Some(p) = &self.playing else {
            self.screen.text(Slot::Time, b"");
            self.screen.progress(0);
            cx.wake_at(None);
            return;
        };
        let ms = u64::from(p.track.ms);
        let at = p.anchor.position(cx.now).min(ms);
        let mut l = Line::new();
        l.clock(at).s(b" / ").clock(ms);
        self.screen.text(Slot::Time, l.bytes());
        let secs = (ms / 1000).max(1) as u32;
        self.screen.progress(((at / 1000) as u32 * 1000 / secs) as u16);
        cx.wake_at(if at < ms { p.anchor.next(cx.now, 1000) } else { None });
    }

    fn draw_rows(&mut self) {
        let Some(next) = &self.next else { return };
        let rows: Vec<(Vec<u8>, Vec<u8>)> = next
            .rows()
            .iter()
            .filter_map(|(_, v)| from_slice::<Track>(v).ok())
            .map(|t| {
                let mut sub = Line::new();
                sub.s(&t.artist).s(b"  ").clock(u64::from(t.ms));
                (t.title, sub.bytes().to_vec())
            })
            .collect();
        self.screen
            .text(Slot::Header, if rows.is_empty() { b"Queue Is Empty" } else { b"Up Next" });
        self.screen.rows(rows);
    }

    fn draw_status(&mut self, cx: &mut Ctx<'_, Msg>) {
        let s = cx.status;
        let mut l = Line::new();
        l.s(match (s.online, cx.node.complete(NEXT, &[])) {
            (false, _) => b"Offline",
            (true, false) => b"Syncing",
            (true, true) => b"Online",
        });
        if s.pending > 0 {
            l.s(b", ").n(s.pending as u64).s(b" Pending");
        }
        if s.refused > 0 {
            l.s(b", ").n(u64::from(s.refused)).s(b" Refused");
        }
        self.screen.text(Slot::Status, l.bytes());
    }
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
        cx.subscribe(NEXT, Vec::new());
        self.next = Some(upcoming(SHOWN).live(&cx.node.view(Space::Shared)));
        self.playing = playing(&cx.node.view(Space::Session));
        self.screen.action(Button::Down, Glyph::Skip);
        self.draw_track(cx);
        self.draw_rows();
        self.draw_status(cx);
    }

    fn button(&mut self, cx: &mut Ctx<'_, Msg>, button: Button) {
        let now = cx.now;
        let online = cx.status.online;
        match button {
            Button::Select => {
                let Some(p) = self.playing.as_ref().filter(|_| online) else { return };
                let anchor = Anchor {
                    at: p.anchor.position(now).min(u64::from(p.track.ms)),
                    wall: now,
                    rate: p.anchor.rate,
                    playing: !p.anchor.playing,
                };
                let track = p.track.clone();
                cx.make(Space::Session, Msg::Play(Playing { track, anchor }));
            }
            Button::Down => {
                let Some(next) = &self.next else { return };
                let Some((key, value)) = next.rows().first() else { return };
                let (Some(at), Ok(track)) = (place(key), from_slice::<Track>(value)) else {
                    return;
                };
                cx.make(Space::Shared, Msg::Pop { at });
                if !online {
                    return;
                }
                let anchor = Anchor {
                    at: 0,
                    wall: now,
                    rate: 1000,
                    playing: true,
                };
                cx.make(Space::Session, Msg::Play(Playing { track, anchor }));
            }
            _ => {}
        }
    }

    fn update(&mut self, cx: &mut Ctx<'_, Msg>, changes: &[Changes; 4]) {
        let shared = &changes[Space::Shared as usize];
        if let Some(next) = &mut self.next {
            if !next.push(&cx.node.view(Space::Shared), shared).is_empty() {
                self.draw_rows();
            }
        }
        if changes[Space::Session as usize].rows.iter().any(|(t, _)| *t == PLAYBACK) {
            self.playing = playing(&cx.node.view(Space::Session));
            self.draw_track(cx);
        }
        self.draw_status(cx);
    }

    fn wake(&mut self, cx: &mut Ctx<'_, Msg>) {
        self.draw_time(cx);
    }
}
