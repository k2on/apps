//! The hub wakes the devices that are not connected (ui/docs/app.md,
//! Background And Notifications; the `server` feature's).
//!
//! ```text
//!  an entry lands ──► the hub's watch: rows of the app's changed?
//!                       │  each live session with a push row (ark::auth::Push),
//!                       │  not connected now, not woken in the last minute
//!                       ▼
//!                     PushSender (ark_push's port: APNs, UnifiedPush; a test's fake)
//!                       │  a wake: the app's id, and on an iPhone the app's own
//!                       │  generic words — nothing of the rows
//!                       ▼
//!                     the device's runtime: Woken(Push) → it dials, syncs as
//!                     always, and builds its notices from what it then holds
//! ```
//!
//! A connected device is never pushed to: the rows reach it over its
//! socket. A wake is coarse — every device not connected is woken when the
//! app's rows change, whoever's rows they are, which a wake that carries
//! nothing can afford — and quiet for a minute after, so a burst of entries
//! is one wake. Changes to who is signed in (`sign_in`, `role`, `push`)
//! wake nobody. An endpoint the push service says is gone has its row
//! taken away, as the server's own write.

use std::collections::BTreeMap;
use std::sync::{mpsc, Arc, OnceLock};

use ark::auth::{live, Push, Role, SignIn};
use ark::format::Encode;
use ark::peer::Who;
use ark::row::Row;
use ark::store::{Change, Store};
use ark_push::{Endpoint, Outcome, PushSender, Wake};

use crate::handle::Handle;

/// How long a woken device is left before it is woken again.
pub const QUIET_MS: i64 = 60_000;

/// Whether a change is the app's rows, not who is signed in.
fn activity(c: &Change) -> bool {
    ![SignIn::TABLE, Role::TABLE, Push::TABLE].contains(&c.table())
}

/// The push rows to wake for `changes`: of sessions live at `now`, not
/// among `open`, and not woken within [`QUIET_MS`] (`woken`, kept up to
/// date here), each with its endpoint read. Pure: what the hub's watch
/// asks, and what the tests do.
pub fn to_wake(store: &Store, changes: &[Change], open: &[Who], now: i64, woken: &mut BTreeMap<String, i64>) -> Vec<(Push, Endpoint)> {
    if !changes.iter().any(activity) {
        return vec![];
    }
    let sessions: BTreeMap<String, SignIn> = store
        .rows(SignIn::TABLE)
        .filter_map(|r| SignIn::from_bytes(r).ok())
        .map(|s| (s.id.clone(), s))
        .collect();
    woken.retain(|_, at| now - *at < QUIET_MS);
    let mut out = vec![];
    for p in store.rows(Push::TABLE).filter_map(|r| Push::from_bytes(r).ok()) {
        let ok = sessions.get(&p.session).is_some_and(|s| live(s, now) && s.user == p.user);
        if !ok || open.iter().any(|w| w.session == p.session) || woken.contains_key(&p.session) {
            continue;
        }
        let Some(e) = Endpoint::from_json(&p.endpoint) else { continue };
        woken.insert(p.session.clone(), now);
        out.push((p, e));
    }
    out
}

/// The hub's push: what the watch decides, sent from a thread of its own
/// so the hub never waits on a push service.
pub(crate) struct Pusher {
    woken: BTreeMap<String, i64>,
    tx: mpsc::Sender<Vec<(Push, Endpoint)>>,
}

impl Pusher {
    /// Wakes `wake` through `sender`; a gone endpoint's row taken away
    /// through `hub`, once it is there.
    pub(crate) fn start(sender: Arc<dyn PushSender>, wake: Wake, hub: Arc<OnceLock<Handle>>) -> Pusher {
        let (tx, rx) = mpsc::channel::<Vec<(Push, Endpoint)>>();
        std::thread::Builder::new()
            .name("ark-push".into())
            .spawn(move || {
                for batch in rx {
                    let endpoints: Vec<Endpoint> = batch.iter().map(|(_, e)| e.clone()).collect();
                    let outcomes = sender.wake(&endpoints, &wake);
                    for ((p, _), o) in batch.into_iter().zip(outcomes) {
                        match o {
                            Outcome::Delivered => tracing::debug!(session = %p.session, "woken"),
                            Outcome::Gone => {
                                tracing::info!(session = %p.session, "its push endpoint is gone");
                                if let Some(h) = hub.get() {
                                    let c = Change::Remove {
                                        table: Push::TABLE.into(),
                                        row: p.to_bytes(),
                                    };
                                    let _ = h.write(vec![c]);
                                }
                            }
                            Outcome::Failed(why) => tracing::warn!(session = %p.session, %why, "not woken"),
                        }
                    }
                }
            })
            .expect("a thread for the push");
        Pusher { woken: BTreeMap::new(), tx }
    }

    /// What the hub's watch is told, at an entry (`changes`) or a tick.
    pub(crate) fn seen(&mut self, store: &Store, changes: Option<&[Change]>, open: &[Who], now: i64) {
        let Some(changes) = changes else { return };
        let batch = to_wake(store, changes, open, now, &mut self.woken);
        if !batch.is_empty() {
            let _ = self.tx.send(batch);
        }
    }
}
