//! Work a window does not wait for before its first frame: done on a
//! thread, and handed back as a message.
//!
//! iced draws nothing until an application's boot returns, so what boot
//! does is a window that has not opened yet. Opening a replica from disk is
//! the case this is for — harken's 33 MB log is a second of reading and
//! replaying on an Asahi laptop, money's a fraction of one — and the window
//! is better open on nothing, saying so, while it is read.
//!
//! What comes back is a [`Slot`]: a message is `Clone`, and what a thread
//! makes (a replica) need not be, so the message carries a handle to it and
//! whoever handles the message [`Slot::take`]s it, once. In a browser there
//! is no thread to give it to, and the work is done before the task returns.

use std::fmt;
use std::sync::{Arc, Mutex};

use iced::Task;

/// What a thread made, waiting for the message that carries it to be
/// handled. A clone is the same slot: the first `take` has it.
pub struct Slot<T>(Arc<Mutex<Option<T>>>);

impl<T> Slot<T> {
    pub fn new(value: T) -> Slot<T> {
        Slot(Arc::new(Mutex::new(Some(value))))
    }

    /// What the thread made, or `None` once it has been taken.
    pub fn take(&self) -> Option<T> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Slot<T> {
        Slot(self.0.clone())
    }
}

impl<T> fmt::Debug for Slot<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Slot(..)")
    }
}

/// `work` on a thread of its own, and `done` with what it made, as the
/// window's next message.
pub fn off_thread<T, M>(work: impl FnOnce() -> T + Send + 'static, done: impl Fn(Slot<T>) -> M + Send + 'static) -> Task<M>
where
    T: Send + 'static,
    M: Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        Task::perform(async move { rx.await.ok() }, move |made| made.map(|m| done(Slot::new(m)))).and_then(Task::done)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Task::done(done(Slot::new(work())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slot is taken once, by whichever clone of the message is handled
    /// first.
    #[test]
    fn a_slot_is_taken_once() {
        let slot = Slot::new(7);
        let again = slot.clone();
        assert_eq!(slot.take(), Some(7));
        assert_eq!(again.take(), None);
    }
}
