//! Server hooks: what an app does about the world outside the database once
//! an entry is durable (`docs/plan-guards.md` D3).
//!
//! A mutation's server half that writes rows is a private block
//! (`ctx.private`), and the log carries what it wrote like anything else. A
//! hook is for what no row can be — a mail sent, a webhook called, a file
//! written somewhere that is not the log — and so it runs where the engine
//! does not: here, beside the hub, never inside `apply` (the engine stays
//! sans-io, and an `apply` that called out would run again on every replay).
//!
//! **When.** After the write that made the entry durable: the hub collects
//! the entries a message appended whose function has a hook, and hands them
//! over only once the journal holding them has been synced — the same moment
//! the `Ack` is released ([`crate::hub`] module docs). A hook never hears of
//! an entry a restart could lose; an entry sequenced while the disk is
//! failing is handed over with the first write that succeeds. Entries the
//! log already held when the server started were committed by an earlier
//! start, and are not handed over again.
//!
//! **Where.** On a thread of its own, in the order the entries were
//! sequenced. The hub's thread owns every socket, and a hook that waits on a
//! mail server would hold every peer's acknowledgement behind it; on its own
//! thread a slow hook delays only the hooks after it. Nothing waits for a
//! hook, so a hook cannot tell a peer anything — what it does is outside the
//! log by definition.
//!
//! **A panic is caught and said**, and the entry stands: it was committed
//! before the hook was asked, and what a hook failed to do is not a verdict
//! on it.

use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc;

use ark::log::{Entry, Facts, Seq};

/// What [`crate::Builder::on_committed`] takes: the entry as the log holds
/// it, and its facts — the whole run's, a private block's included.
pub type Hook = Box<dyn Fn(&Entry, &Facts) + Send + Sync>;

/// The hooks an app registered, by function name, and the thread they run
/// on.
pub(crate) struct Hooks {
    names: BTreeSet<String>,
    tx: mpsc::Sender<(Seq, String, Entry, Facts)>,
}

impl Hooks {
    /// The thread, started; `None` when nothing is registered, which costs
    /// a server nothing.
    pub(crate) fn start(hooks: Vec<(String, Hook)>) -> Option<Hooks> {
        if hooks.is_empty() {
            return None;
        }
        let names = hooks.iter().map(|(n, _)| n.clone()).collect();
        let (tx, rx) = mpsc::channel::<(Seq, String, Entry, Facts)>();
        std::thread::Builder::new()
            .name("ark-server hooks".into())
            .spawn(move || {
                // Until the hub is gone: its sender is the only one.
                for (n, name, e, f) in rx {
                    for (_, hook) in hooks.iter().filter(|(h, _)| *h == name) {
                        if catch_unwind(AssertUnwindSafe(|| hook(&e, &f))).is_err() {
                            eprintln!("ark-server: the on_committed hook for {name} panicked on entry {n}; the entry stands");
                        }
                    }
                }
            })
            .ok()?;
        Some(Hooks { names, tx })
    }

    /// Whether entries of the function `name` have a hook.
    pub(crate) fn wants(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Hand a durable entry over, in sequence order.
    pub(crate) fn committed(&self, n: Seq, name: String, e: Entry, f: Facts) {
        // A thread that has gone has gone with every hook; the entry stands.
        let _ = self.tx.send((n, name, e, f));
    }
}
