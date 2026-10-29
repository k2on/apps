//! The link's schedule: a connection that had opened is dialled again the
//! moment it closes, and only a dial that fails waits, doubling — so
//! "waiting to dial" is said of a server that was tried and not reached,
//! never of one about to be dialled. Falsified by the phones' "can't reach
//! the server" notice flashing as a dropped connection was dialled again:
//! every close waited `backoff.0` first, and showed as waiting meanwhile.
mod support;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ark_peer::link::{wait, Event, Wake, Wire};
use support::linked;

/// A connection that says what it was told to, once, and nothing after.
struct Scripted(VecDeque<Event>);

impl Wire for Scripted {
    fn send(&mut self, _: Vec<u8>) {}
    fn events(&mut self) -> Vec<Event> {
        self.0.drain(..).collect()
    }
}

#[test]
fn a_dropped_connection_is_dialled_again_at_once_and_a_failing_dial_waits() {
    let started = Instant::now();
    let dials: Arc<Mutex<Vec<Duration>>> = Default::default();
    let d = dials.clone();
    // The first connection opens and then goes, as a server restarting does; every dial after
    // it is refused at once.
    let dial = move |_: &str, _: &str, _: Option<Wake>| -> Box<dyn Wire> {
        let mut d = d.lock().unwrap();
        d.push(started.elapsed());
        Box::new(Scripted(VecDeque::from(if d.len() == 1 {
            vec![Event::Open, Event::Closed("gone".into())]
        } else {
            vec![Event::Closed("refused".into())]
        })))
    };
    // `linked` sets the backoff to 5 ms, doubling to 20.
    let (mut c, rx) = linked("alice@phone", "", dial);

    // One pump: the dial, its opening and its closing, and the dial again — in that same pump,
    // so that nobody between pumps sees the link waiting.
    c.pump();
    assert_eq!(2, dials.lock().unwrap().len(), "dropped: dialled again at once");
    assert_eq!("connecting", c.status().link, "and never shown waiting");

    // The second was refused: now it waits, 5 ms, then 10, then 20 and no longer.
    let deadline = Instant::now() + Duration::from_secs(2);
    while dials.lock().unwrap().len() < 5 && Instant::now() < deadline {
        c.pump();
        if dials.lock().unwrap().len() < 5 {
            assert_eq!("waiting to dial", c.status().link, "refused: waiting, and said so");
        }
        wait(&c, &rx, Duration::from_millis(1));
    }
    let d = dials.lock().unwrap().clone();
    assert_eq!(5, d.len(), "kept dialling: {d:?}");
    let gaps: Vec<u64> = d.windows(2).map(|w| (w[1] - w[0]).as_millis() as u64).collect();
    // The link counts whole milliseconds, so a wait of n may measure as n - 1.
    assert!(gaps[0] < 5, "the drop was dialled again at once: {gaps:?}");
    assert!(
        gaps[1] >= 4 && gaps[2] >= 9 && gaps[3] >= 19,
        "each refusal waits twice as long: {gaps:?}"
    );
}
