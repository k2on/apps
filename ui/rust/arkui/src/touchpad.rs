//! Fingers resting on a trackpad, as Firefox hears them on Linux: the
//! Wayland hold gesture (`zwp_pointer_gesture_hold_v1`, pointer gestures
//! version 3), which says when fingers land on a trackpad and stay still,
//! and when they lift or move on into a scroll. A coast, or a band
//! springing back, stops the moment they land ([`crate::scroll::elastic`]),
//! as Firefox's does (`apz.gtk.touchpad_hold.enabled`, Firefox 134).
//!
//! GTK 3 and winit pass the gesture on to nobody, so Firefox binds it
//! itself on the display GTK already has (Gecko's `nsWaylandDisplay.cpp`,
//! `gesture_hold_begin` and `gesture_hold_end`, feeding
//! `nsWindow::OnTouchpadHoldEvent`). Here the same is done on the display
//! winit already has ([`listen`]): its `wl_display`, read from the window's
//! raw display handle, gets an event queue of its own on the same
//! connection, a `wl_seat` and a `wl_pointer` of its own for that seat, and
//! the hold gesture for that pointer, dispatched on a thread of its own.
//! The compositor sends a hold only for a surface of the client its pointer
//! is over, so it has to be winit's connection, not a new one.
//!
//! What it hears is said to every scrollable at once ([`fingers`]): how
//! many times fingers have come to rest, and whether they rest now. A
//! coast reads it on its next frame; a band held still under them is woken
//! by [`subscription`] when they lift.
//!
//! Only on Linux, and only on Wayland: on X11, where a compositor lacks
//! the protocol, and in a browser, nothing is heard and nothing changes.
//! Wherever it runs, a scroll still stops for any word from the wheel,
//! even one that moved nowhere, a click or a key.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;

use iced::{Subscription, Task};

use crate::wake::Bell;

/// How many times fingers have come to rest on the trackpad.
static HOLDS: AtomicU64 = AtomicU64::new(0);
/// Whether they rest there now.
static RESTING: AtomicBool = AtomicBool::new(false);
/// Whether the system says when fingers lift: Wayland, once its display is
/// seen.
static SAYS_LIFT: AtomicBool = AtomicBool::new(false);

/// What fingers on the trackpad have done, as a scrollable reads it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fingers {
    /// How many times they have come to rest: a change since last read is
    /// fingers that landed.
    pub holds: u64,
    /// Whether they rest there now.
    pub resting: bool,
}

/// What the fingers have done, as last heard.
pub fn fingers() -> Fingers {
    Fingers {
        holds: HOLDS.load(Ordering::SeqCst),
        resting: RESTING.load(Ordering::SeqCst),
    }
}

/// Whether only the system's word says fingers lifted from a trackpad,
/// never their stillness: macOS (the end of a phase), and Wayland once
/// [`listen`] has seen its display (`axis_stop`). Not X11, which never
/// says, nor a browser, whose `wheel` event has no phase.
pub fn says_lift() -> bool {
    cfg!(target_os = "macos") || SAYS_LIFT.load(Ordering::Relaxed)
}

/// What rings when the fingers come to rest or lift, so a band they held
/// is let go though nothing else moves.
fn bell() -> &'static Bell {
    static BELL: OnceLock<Bell> = OnceLock::new();
    BELL.get_or_init(Bell::new)
}

/// A message each time fingers come to rest on the trackpad or lift: for
/// an app to answer with nothing, so its window draws again and a band
/// they held springs back.
pub fn subscription() -> Subscription<()> {
    bell().subscription()
}

/// Something the hold gesture said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Said {
    /// Fingers came to rest.
    Rest,
    /// They lifted, or moved on (`cancelled`).
    Lift { cancelled: bool },
}

/// What was said, told to every scrollable.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn say(said: Said) {
    match said {
        Said::Rest => {
            HOLDS.fetch_add(1, Ordering::SeqCst);
            RESTING.store(true, Ordering::SeqCst);
        }
        Said::Lift { .. } => RESTING.store(false, Ordering::SeqCst),
    }
    bell().ring();
}

/// What to ask the compositor for next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Bind the seat, global `name`, at `version`.
    Seat { name: u32, version: u32 },
    /// Bind the pointer gestures, global `name`, at `version`.
    Gestures { name: u32, version: u32 },
    /// Get a pointer of our own for the seat.
    Pointer,
    /// Get the hold gesture for that pointer.
    Hold,
    /// Let the pointer and its hold go: the seat has none any more.
    Release,
}

/// The hold gesture's state, from the protocol's events alone: which
/// globals there are, whether the seat has a pointer, whether a hold is
/// bound, and whether fingers rest. Pure, so it is tested with the events
/// made up; the Wayland side only carries out what it asks.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Machine {
    seat: Option<u32>,
    gestures: Option<u32>,
    pointer: bool,
    hold: bool,
    resting: bool,
}

/// The seat's version asked for: what a pointer needs, no more.
const SEAT_VERSION: u32 = 5;
/// The pointer gestures' version with the hold gesture.
const HOLD_VERSION: u32 = 3;

impl Machine {
    /// The registry announced global `name`, an `interface` at `version`.
    pub fn global(&mut self, name: u32, interface: &str, version: u32) -> Option<Ask> {
        match interface {
            "wl_seat" if self.seat.is_none() => {
                self.seat = Some(name);
                Some(Ask::Seat {
                    name,
                    version: version.min(SEAT_VERSION),
                })
            }
            "zwp_pointer_gestures_v1" if self.gestures.is_none() && version >= HOLD_VERSION => {
                self.gestures = Some(name);
                Some(Ask::Gestures { name, version: HOLD_VERSION })
            }
            _ => None,
        }
    }

    /// Once the gestures are bound: the hold, if the pointer is there.
    pub fn bound_gestures(&mut self) -> Option<Ask> {
        self.want_hold()
    }

    fn want_hold(&mut self) -> Option<Ask> {
        match self.pointer && self.gestures.is_some() && !self.hold {
            true => {
                self.hold = true;
                Some(Ask::Hold)
            }
            false => None,
        }
    }

    /// The seat says whether it has a pointer.
    pub fn capabilities(&mut self, pointer: bool) -> Vec<Ask> {
        let mut asks = Vec::new();
        match (pointer, self.pointer) {
            (true, false) => {
                self.pointer = true;
                asks.push(Ask::Pointer);
                asks.extend(self.want_hold());
            }
            (false, true) => {
                self.pointer = false;
                self.hold = false;
                asks.push(Ask::Release);
            }
            _ => {}
        }
        asks
    }

    /// The hold gesture began: fingers came to rest.
    pub fn hold_begin(&mut self) -> Option<Said> {
        self.resting = true;
        Some(Said::Rest)
    }

    /// The hold gesture ended: they lifted, or moved on. Nothing if no hold
    /// had begun.
    pub fn hold_end(&mut self, cancelled: bool) -> Option<Said> {
        std::mem::replace(&mut self.resting, false).then_some(Said::Lift { cancelled })
    }

    /// The pointer went: fingers resting on it are taken to have lifted.
    pub fn released(&mut self) -> Option<Said> {
        self.hold_end(true)
    }

    /// Whether there is a hold gesture to listen to, once the globals and
    /// the seat have been heard.
    pub fn listening(&self) -> bool {
        self.hold
    }
}

/// Listen for fingers resting on the trackpad, on the display of the app's
/// first window: a task for an app's boot. On Wayland it also says that the
/// system says when fingers lift ([`says_lift`]). Elsewhere it does
/// nothing.
pub fn listen<M: Send + 'static>() -> Task<M> {
    #[cfg(target_os = "linux")]
    {
        iced::window::oldest()
            .and_then(|id| iced::window::run(id, |window| wayland::attach(window)))
            .discard()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Task::none()
    }
}

#[cfg(target_os = "linux")]
mod wayland {
    //! The hold gesture on winit's own Wayland connection.

    use raw_window_handle::RawDisplayHandle;
    use wayland_client::backend::Backend;
    use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
    use wayland_protocols::wp::pointer_gestures::zv1::client::{zwp_pointer_gesture_hold_v1, zwp_pointer_gestures_v1};

    use super::{say, Ask, Machine, SAYS_LIFT};

    /// The window's display, if it is Wayland's: listened to on a thread of
    /// its own. X11, or anything else, is left alone.
    pub fn attach(window: &dyn iced::window::Window) {
        let Ok(handle) = window.display_handle() else {
            return;
        };
        let RawDisplayHandle::Wayland(wayland) = handle.as_raw() else {
            return;
        };
        SAYS_LIFT.store(true, std::sync::atomic::Ordering::Relaxed);
        // A pointer is not `Send`; the display it names lives as long as the
        // event loop, which is as long as the app.
        let display = wayland.display.as_ptr() as usize;
        let _ = std::thread::Builder::new().name("arkui-touchpad".into()).spawn(move || {
            if let Err(e) = run(display) {
                eprintln!("arkui: no touchpad hold gestures: {e}");
            }
        });
    }

    /// The protocol objects the [`Machine`] asks for, as they are made.
    #[derive(Default)]
    struct Glue {
        machine: Machine,
        registry: Option<wl_registry::WlRegistry>,
        seat: Option<wl_seat::WlSeat>,
        gestures: Option<zwp_pointer_gestures_v1::ZwpPointerGesturesV1>,
        pointer: Option<wl_pointer::WlPointer>,
        hold: Option<zwp_pointer_gesture_hold_v1::ZwpPointerGestureHoldV1>,
    }

    impl Glue {
        fn ask(&mut self, ask: Ask, qh: &QueueHandle<Glue>) {
            let Some(registry) = self.registry.clone() else {
                return;
            };
            match ask {
                Ask::Seat { name, version } => self.seat = Some(registry.bind(name, version, qh, ())),
                Ask::Gestures { name, version } => {
                    self.gestures = Some(registry.bind(name, version, qh, ()));
                    if let Some(next) = self.machine.bound_gestures() {
                        self.ask(next, qh);
                    }
                }
                Ask::Pointer => self.pointer = self.seat.as_ref().map(|s| s.get_pointer(qh, ())),
                Ask::Hold => {
                    if let (Some(gestures), Some(pointer)) = (&self.gestures, &self.pointer) {
                        self.hold = Some(gestures.get_hold_gesture(pointer, qh, ()));
                    }
                }
                Ask::Release => {
                    if let Some(hold) = self.hold.take() {
                        hold.destroy();
                    }
                    if let Some(pointer) = self.pointer.take() {
                        pointer.release();
                    }
                    if let Some(said) = self.machine.released() {
                        say(said);
                    }
                }
            }
        }
    }

    /// The display at `display`, a `wl_display` winit made, given a queue of
    /// its own, and listened to for as long as it lasts.
    fn run(display: usize) -> Result<(), Box<dyn std::error::Error>> {
        // SAFETY: the pointer is winit's live `wl_display`, which outlives
        // this thread for as long as the app runs; libwayland lets several
        // threads read one display, each dispatching its own queue.
        let backend = unsafe { Backend::from_foreign_display(display as *mut _) };
        let connection = Connection::from_backend(backend);
        let mut queue: EventQueue<Glue> = connection.new_event_queue();
        let qh = queue.handle();
        let mut glue = Glue {
            registry: Some(connection.display().get_registry(&qh, ())),
            ..Glue::default()
        };
        // The globals, then the seat's capabilities.
        queue.roundtrip(&mut glue)?;
        queue.roundtrip(&mut glue)?;
        if !glue.machine.listening() {
            return Err("the compositor has no pointer gestures v3, or the seat no pointer".into());
        }
        loop {
            queue.blocking_dispatch(&mut glue)?;
        }
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for Glue {
        fn event(state: &mut Self, _: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
            if let wl_registry::Event::Global { name, interface, version } = event {
                if let Some(ask) = state.machine.global(name, &interface, version) {
                    state.ask(ask, qh);
                }
            }
        }
    }

    impl Dispatch<wl_seat::WlSeat, ()> for Glue {
        fn event(state: &mut Self, _: &wl_seat::WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
            if let wl_seat::Event::Capabilities {
                capabilities: WEnum::Value(c),
            } = event
            {
                for ask in state.machine.capabilities(c.contains(wl_seat::Capability::Pointer)) {
                    state.ask(ask, qh);
                }
            }
        }
    }

    impl Dispatch<wl_pointer::WlPointer, ()> for Glue {
        // Our own pointer's motion and buttons are winit's to act on.
        fn event(_: &mut Self, _: &wl_pointer::WlPointer, _: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
    }

    impl Dispatch<zwp_pointer_gestures_v1::ZwpPointerGesturesV1, ()> for Glue {
        fn event(
            _: &mut Self,
            _: &zwp_pointer_gestures_v1::ZwpPointerGesturesV1,
            _: zwp_pointer_gestures_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<zwp_pointer_gesture_hold_v1::ZwpPointerGestureHoldV1, ()> for Glue {
        fn event(
            state: &mut Self,
            _: &zwp_pointer_gesture_hold_v1::ZwpPointerGestureHoldV1,
            event: zwp_pointer_gesture_hold_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            let said = match event {
                zwp_pointer_gesture_hold_v1::Event::Begin { .. } => state.machine.hold_begin(),
                zwp_pointer_gesture_hold_v1::Event::End { cancelled, .. } => state.machine.hold_end(cancelled != 0),
                _ => None,
            };
            if let Some(said) = said {
                say(said);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry, the seat and the gestures in the order a compositor
    /// sends them: the seat bound at the version a pointer needs, the
    /// gestures at version 3, a pointer once the seat says it has one, and
    /// the hold once both are there, whichever came first; a rest and a
    /// lift said, a lift without a rest not. Falsified by binding the hold
    /// before there is a pointer, or by a compositor of version 2 (no hold)
    /// taken for one with it.
    #[test]
    fn the_hold_is_bound_once_the_seat_has_a_pointer_and_heard() {
        let mut m = Machine::default();
        assert_eq!(m.global(1, "wl_compositor", 6), None);
        assert_eq!(m.global(7, "wl_seat", 9), Some(Ask::Seat { name: 7, version: 5 }));
        assert_eq!(m.global(8, "wl_seat", 9), None, "one seat");
        assert_eq!(m.global(12, "zwp_pointer_gestures_v1", 3), Some(Ask::Gestures { name: 12, version: 3 }));
        assert_eq!(m.bound_gestures(), None, "no pointer yet");
        assert!(!m.listening());
        assert_eq!(m.capabilities(true), vec![Ask::Pointer, Ask::Hold]);
        assert!(m.listening());
        assert_eq!(m.capabilities(true), vec![], "said again, nothing new");

        assert_eq!(m.hold_end(false), None, "a lift without a rest");
        assert_eq!(m.hold_begin(), Some(Said::Rest));
        assert_eq!(m.hold_end(true), Some(Said::Lift { cancelled: true }), "moved on into a scroll");
        assert_eq!(m.hold_end(false), None, "said once");
        assert_eq!(m.hold_begin(), Some(Said::Rest));
        assert_eq!(m.capabilities(false), vec![Ask::Release], "the pointer went");
        assert_eq!(m.released(), Some(Said::Lift { cancelled: true }), "and the fingers with it");
        assert!(!m.listening());

        // The pointer first, the gestures after.
        let mut m = Machine::default();
        assert!(m.global(3, "wl_seat", 5).is_some());
        assert_eq!(m.capabilities(true), vec![Ask::Pointer]);
        assert!(m.global(4, "zwp_pointer_gestures_v1", 3).is_some());
        assert_eq!(m.bound_gestures(), Some(Ask::Hold));

        // A compositor with only swipes and pinches: nothing to listen to.
        let mut m = Machine::default();
        let _ = m.global(3, "wl_seat", 5);
        assert_eq!(m.global(4, "zwp_pointer_gestures_v1", 2), None);
        assert_eq!(m.capabilities(true), vec![Ask::Pointer]);
        assert!(!m.listening());
    }
}
