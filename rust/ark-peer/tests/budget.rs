//! A pump's budget (`Link::budget`): what a page whose one thread also
//! draws asks of its peer. A pump takes in what the connection said until
//! its time is up, at least one thing, and leaves the rest for the next —
//! which is due at once. Falsified by a pump that took in a whole fill
//! before the page could draw, and by one that left the rest with no
//! deadline to come back for it: the fill stalled until something else
//! woke the page.
mod support;

use std::collections::VecDeque;

use ark_peer::link::{Event, Wake, Wire};
use support::linked;

/// A connection that says everything it was told to at once.
struct Scripted(VecDeque<Event>);

impl Wire for Scripted {
    fn send(&mut self, _: Vec<u8>) {}
    fn events(&mut self) -> Vec<Event> {
        self.0.drain(..).collect()
    }
}

#[test]
fn a_pump_out_of_time_leaves_the_rest_for_the_next_and_says_it_is_due() {
    let dial = |_: &str, _: &str, _: Option<Wake>| -> Box<dyn Wire> {
        let frames = (0..5).map(|_| Event::Frame(vec![0xff]));
        Box::new(Scripted(std::iter::once(Event::Open).chain(frames).collect()))
    };
    let (mut c, _rx) = linked("alice@phone", "", dial);
    // No time at all: one thing a pump.
    c.link.budget = Some(0);
    let mut pumps = 0;
    while c.pump().moved {
        pumps += 1;
        if c.backlog() > 0 {
            let due = c.deadline().expect("the rest is due");
            assert!(due <= ark_peer::monotonic_ms(), "and due now");
        }
    }
    assert_eq!(pumps, 6, "the open and five frames, one a pump");
    assert_eq!(c.backlog(), 0);
}
