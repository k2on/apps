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
//!   into [`screen`]: this crate only works out its record, a [`Face`],
//!   and acts on what its buttons send.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use ark_kernel::codec::from_slice;
use ark_kernel::{Actor, Anchor, Changes, Handler, Live, Origin, Refusal, Space, Version};
use ark_pebble::text::Line;
use ark_pebble::tokens::BACKGROUND;
use ark_pebble::{App, Button, Ctx, Ui};
use now_playing::face::{Act, Face, Next};
use now_playing::{place, playing, upcoming, Msg, Playing, Track, HANDLERS, NEXT, PLAYBACK, VERSION};

/// The screen's layers, written at build time from its `#[view]`.
pub mod screen {
    include!(concat!(env!("OUT_DIR"), "/screen.rs"));
}

/// How many of the queue the list shows.
const SHOWN: usize = 3;

struct NowPlaying {
    ui: Option<Ui>,
    face: Face,
    next: Option<Live>,
    /// The player's playback, and whose.
    playing: Option<(Origin, Playing)>,
    /// A control was refused for want of a link, until a hello is
    /// answered.
    unreachable: bool,
}

ark_pebble::app!(NowPlaying {
    ui: None,
    face: Face {
        status: String::new(),
        link: String::new(),
        title: String::new(),
        artist: String::new(),
        progress: 0.0,
        at: String::new(),
        length: String::new(),
        queue: String::new(),
        next: Vec::new(),
        play_glyph: String::new(),
        play_label: String::new(),
    },
    next: None,
    playing: None,
    unreachable: false,
});

/// Bytes as words: what the queue's rows and the clock hold is UTF-8.
fn words(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap_or_default()
}

impl NowPlaying {
    /// The record handed to the screen, which redraws what changed.
    fn draw(&mut self) {
        if let Some(ui) = &mut self.ui {
            screen::draw(ui, &self.face);
        }
    }

    fn track(&mut self, cx: &mut Ctx<'_, Msg>) {
        let f = &mut self.face;
        match self.playing.as_ref().map(|p| &p.1) {
            Some(p) => {
                f.title = words(&p.track.title);
                f.artist = words(&p.track.artist);
                let on = p.anchor.playing;
                f.play_glyph = String::from(if on { "pause" } else { "play" });
                f.play_label = String::from(if on { "Pause" } else { "Play" });
            }
            None => {
                f.title = String::from("Nothing Playing");
                f.artist = String::new();
                f.play_glyph = String::from("play");
                f.play_label = String::from("Play");
            }
        }
        self.time(cx);
    }

    /// Where playback is, and when that next changes on the screen.
    fn time(&mut self, cx: &mut Ctx<'_, Msg>) {
        let f = &mut self.face;
        let Some((_, p)) = &self.playing else {
            f.at = String::new();
            f.length = String::new();
            f.progress = 0.0;
            cx.wake_at(None);
            return;
        };
        let ms = u64::from(p.track.ms);
        let now = cx.time();
        let at = p.anchor.position(now).min(ms);
        let (mut a, mut l) = (Line::new(), Line::new());
        a.clock(at);
        l.clock(ms);
        f.at = words(a.bytes());
        f.length = words(l.bytes());
        // In whole seconds, as the clock under it reads; in thousandths
        // first, so the watch divides no floats.
        let secs = (ms / 1000).max(1) as u32;
        let permille = (at / 1000) as u32 * 1000 / secs;
        f.progress = f64::from(permille) * 0.001;
        let next = p.anchor.next(now, 1000).filter(|_| at < ms);
        let wake = next.map(|t| cx.local(t));
        cx.wake_at(wake);
    }

    fn rows(&mut self) {
        let Some(next) = &self.next else { return };
        self.face.next = next
            .rows()
            .iter()
            .filter_map(|(_, v)| from_slice::<Track>(v).ok())
            .map(|t| {
                let mut line = Line::new();
                line.s(&t.artist).s(b"  ").clock(u64::from(t.ms));
                Next {
                    title: words(&t.title),
                    line: words(line.bytes()),
                }
            })
            .collect();
        self.face.queue = String::from(if self.face.next.is_empty() { "Queue Is Empty" } else { "Up Next" });
    }

    fn status(&mut self, cx: &mut Ctx<'_, Msg>) {
        let s = cx.status;
        let (words_, glyph) = match (s.online, cx.node.complete(NEXT, &[])) {
            _ if self.unreachable => ("Can't Reach Player", "wifi-off"),
            (false, _) => ("Offline", "wifi-off"),
            (true, false) => ("Syncing", "refresh-cw"),
            (true, true) => ("Online", "wifi"),
        };
        let mut l = Line::new();
        l.s(words_.as_bytes());
        if s.pending > 0 {
            l.s(b", ").n(s.pending as u64).s(b" Pending");
        }
        if s.refused > 0 {
            l.s(b", ").n(u64::from(s.refused)).s(b" Refused");
        }
        self.face.status = words(l.bytes());
        self.face.link = String::from(glyph);
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
        let mut ui = Ui::new(screen::GLYPHS, BACKGROUND);
        screen::build(&mut ui);
        self.ui = Some(ui);
        cx.subscribe(NEXT, Vec::new());
        self.next = Some(upcoming(SHOWN).live(&cx.node.view(Space::Shared)));
        self.playing = playing(&cx.node.view(Space::Session));
        self.track(cx);
        self.rows();
        self.status(cx);
        self.draw();
    }

    fn button(&mut self, cx: &mut Ctx<'_, Msg>, button: Button) {
        let now = cx.time();
        let played = match screen::press(&self.face, button) {
            Some(Act::Toggle) => {
                let Some((player, p)) = self.playing.clone() else { return };
                let anchor = Anchor {
                    at: p.anchor.position(now).min(u64::from(p.track.ms)),
                    wall: now,
                    rate: p.anchor.rate,
                    playing: !p.anchor.playing,
                };
                let playing = Playing { track: p.track, anchor };
                cx.make(Space::Session, Msg::Play { player, playing })
            }
            Some(Act::Skip) => {
                let Some(next) = &self.next else { return };
                let Some((key, value)) = next.rows().first() else { return };
                let (Some(at), Ok(track)) = (place(key), from_slice::<Track>(value)) else {
                    return;
                };
                // Pending until the server confirms it, offline or not.
                let _ = cx.make(Space::Shared, Msg::Pop { at });
                let Some((player, _)) = self.playing else { return };
                let anchor = Anchor {
                    at: 0,
                    wall: now,
                    rate: 1000,
                    playing: true,
                };
                cx.make(
                    Space::Session,
                    Msg::Play {
                        player,
                        playing: Playing { track, anchor },
                    },
                )
            }
            None => return,
        };
        if played == Err(Refusal::OFFLINE) {
            self.unreachable = true;
            self.status(cx);
            self.draw();
        }
    }

    fn update(&mut self, cx: &mut Ctx<'_, Msg>, changes: &[Changes; 4]) {
        let shared = &changes[Space::Shared as usize];
        if let Some(next) = &mut self.next {
            if !next.push(&cx.node.view(Space::Shared), shared).is_empty() {
                self.rows();
            }
        }
        // Reached again once a hello is answered, not at a link that
        // closes before it is.
        if cx.status.online && cx.node.complete(NEXT, &[]) {
            self.unreachable = false;
        }
        if changes[Space::Session as usize].rows.iter().any(|(t, _)| *t == PLAYBACK) {
            self.playing = playing(&cx.node.view(Space::Session));
            self.track(cx);
        }
        self.status(cx);
        self.draw();
    }

    fn wake(&mut self, cx: &mut Ctx<'_, Msg>) {
        self.time(cx);
        self.draw();
    }
}
