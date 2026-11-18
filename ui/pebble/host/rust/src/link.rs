//! The node's link to its upstream over AppMessage: the kernel's frames
//! as a stream of bytes, cut into messages the size of the outbox and put
//! back together from the inbox's ([`ark_kernel::Frames`]).
//!
//! The phone's side (the relay script, or later harken's phone app) opens
//! a connection upstream and says so with a number, its **epoch**; each
//! chunk either way carries it, so a chunk of a connection that has ended
//! is never read as part of the next. The link is up while the phone is
//! connected (`connection_service`) and an epoch is open; at each new one
//! the node says its subscriptions, hello and pending entries again
//! ([`ark_kernel::Node::connect`]). Frames made while it is down are
//! dropped: what matters (the pending entries, the subscriptions) is said
//! again at the next connection.
//!
//! AppMessage sends one message at a time and acknowledges each. A chunk
//! is let go once acknowledged; one refused is sent again after a while.
//! The watch's first message says what it is ([`Wants::Ready`]: its inbox
//! size, for the phone to cut its chunks to, and its origin), at its start,
//! when the phone connects again and when the phone's script asks.

use alloc::vec::Vec;

use ark_kernel::codec::put_frame;
use ark_kernel::{Encode, Link, Up};

/// What is in the outbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flight {
    None,
    Ready,
    /// A chunk of so many bytes of `out`; `stale` once the connection it
    /// was sent on has ended, so its acknowledgement lets nothing go.
    Chunk {
        len: usize,
        stale: bool,
    },
}

/// What the link has next to send, for the host to hand the shim.
#[derive(Debug, PartialEq, Eq)]
pub enum Wants<'a> {
    Nothing,
    Ready,
    Chunk { epoch: u32, bytes: &'a [u8] },
}

pub struct AmLink {
    /// The frames to go up, as bytes, from the front.
    out: Vec<u8>,
    flight: Flight,
    ready: bool,
    /// The phone is connected.
    phone: bool,
    /// The phone's connection upstream; 0 while it has none.
    epoch: u32,
    /// The most bytes of frames a message carries.
    chunk: usize,
    pub stats: Stats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub sent: u32,
    pub received: u32,
    pub connects: u32,
    pub refused: u32,
}

impl AmLink {
    /// A link whose messages carry at most `chunk` bytes of frames.
    pub fn new(chunk: usize) -> AmLink {
        AmLink {
            out: Vec::new(),
            flight: Flight::None,
            ready: true,
            phone: false,
            epoch: 0,
            chunk: chunk.max(16),
            stats: Stats::default(),
        }
    }

    pub fn online(&self) -> bool {
        self.phone && self.epoch != 0
    }

    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// Bytes waiting to go up.
    pub fn queued(&self) -> usize {
        self.out.len()
    }

    /// The phone connected or went: on its return the watch says what it
    /// is again. Answers whether the link went down.
    pub fn phone(&mut self, connected: bool) -> bool {
        let was = self.online();
        self.phone = connected;
        if connected {
            self.ready = true;
        } else {
            self.down();
        }
        was && !self.online()
    }

    /// The phone's script asked what the watch is.
    pub fn hello(&mut self) {
        self.ready = true;
    }

    /// The phone opened connection `epoch` upstream, or has none (0): what
    /// was queued for the last one is dropped. Answers whether the link is
    /// now up, for the node to connect.
    pub fn epoch_set(&mut self, epoch: u32) -> bool {
        self.down();
        self.epoch = epoch;
        if self.online() {
            self.stats.connects += 1;
        }
        self.online()
    }

    fn down(&mut self) {
        self.epoch = 0;
        self.out.clear();
        if let Flight::Chunk { stale, .. } = &mut self.flight {
            *stale = true;
        }
    }

    /// Whether a chunk from the phone belongs to the connection open now.
    pub fn current(&self, epoch: u32) -> bool {
        self.online() && epoch == self.epoch
    }

    /// What to send next, if the outbox is free.
    pub fn wants(&self) -> Wants<'_> {
        if self.flight != Flight::None || !self.phone {
            return Wants::Nothing;
        }
        if self.ready {
            return Wants::Ready;
        }
        if self.online() && !self.out.is_empty() {
            let n = self.out.len().min(self.chunk);
            return Wants::Chunk {
                epoch: self.epoch,
                bytes: &self.out[..n],
            };
        }
        Wants::Nothing
    }

    /// What [`AmLink::wants`] answered is in the outbox.
    pub fn sending(&mut self) {
        self.flight = match self.wants() {
            Wants::Nothing => Flight::None,
            Wants::Ready => Flight::Ready,
            Wants::Chunk { bytes, .. } => Flight::Chunk {
                len: bytes.len(),
                stale: false,
            },
        };
    }

    /// The phone acknowledged the message in the outbox.
    pub fn sent(&mut self) {
        match self.flight {
            Flight::Ready => self.ready = false,
            Flight::Chunk { len, stale: false } => {
                self.out.drain(..len);
                self.stats.sent += len as u32;
            }
            _ => {}
        }
        self.flight = Flight::None;
    }

    /// The message in the outbox was refused or timed out: it is sent
    /// again (a stale chunk is not).
    pub fn failed(&mut self) {
        self.stats.refused += 1;
        self.flight = Flight::None;
    }
}

impl<B: Encode> Link<B> for AmLink {
    fn send(&mut self, up: Up<B>) {
        if self.online() {
            put_frame(&mut self.out, &up);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_kernel::{Frames, Seq};

    fn hello(link: &mut AmLink, cursor: u64) {
        Link::<u8>::send(
            link,
            Up::Hello {
                cursor: Seq(cursor),
                time: 0,
            },
        );
    }

    /// Frames go up in chunks, each let go when acknowledged, a refused one
    /// sent again; a connection that ends drops what was queued, and a
    /// late acknowledgement of its chunk lets nothing of the next go.
    #[test]
    fn chunks_go_in_order_and_epochs_part_them() {
        let mut link = AmLink::new(16);
        hello(&mut link, 1);
        assert_eq!(link.wants(), Wants::Nothing);
        link.phone(true);
        assert_eq!(link.wants(), Wants::Ready);
        link.sending();
        link.sent();
        assert!(link.epoch_set(9));
        for c in 0..20 {
            hello(&mut link, 1 << (c * 3));
        }
        let mut wire = Vec::new();
        let mut refuse = true;
        while let Wants::Chunk { epoch, bytes } = link.wants() {
            assert_eq!(epoch, 9);
            assert!(bytes.len() <= 16);
            let bytes = bytes.to_vec();
            link.sending();
            if refuse {
                link.failed();
            } else {
                wire.extend(bytes);
                link.sent();
            }
            refuse = !refuse;
        }
        let mut frames = Frames::new(64);
        frames.push(&wire);
        for c in 0..20 {
            assert_eq!(
                frames.read::<Up<u8>>(),
                Ok(Some(Up::Hello {
                    cursor: Seq(1 << (c * 3)),
                    time: 0
                }))
            );
        }
        hello(&mut link, 5);
        link.sending();
        assert!(link.epoch_set(10));
        hello(&mut link, 6);
        link.sent();
        // The new connection's hello whole: its length, tag, cursor and
        // time.
        assert_eq!(link.queued(), 4);
        link.phone(false);
        assert!(!link.online() && link.queued() == 0);
    }
}
