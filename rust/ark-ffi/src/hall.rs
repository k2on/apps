//! A hall: an app's server in this process — the hub a server runs, over
//! memory — which devices dial without a socket. Its devices are pumped by
//! hand ([`Hall::carry`]), so a test says when what was said has been
//! answered: what the apps' tests, the phones' among them, sync devices
//! through.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use ark::peer::Ctx;
use ark_peer::{now_ms, Authority, Config, Handle, Memory, ServerConfig};

use crate::{Peer, Refusal};

pub struct Hall {
    pub hub: Handle<Authority>,
    devices: Mutex<Vec<Weak<Peer>>>,
    /// Who each token the hall handed a device is.
    who: Arc<Mutex<BTreeMap<String, Ctx>>>,
}

fn held<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Hall {
    /// A server of `config`, its log empty.
    pub fn new(config: impl Into<ServerConfig>) -> Hall {
        let hub = Authority::serve(config, Box::new(Memory::default()), now_ms()).expect("an empty log reads");
        Hall {
            hub: Handle::spawn(hub),
            devices: Mutex::default(),
            who: Arc::default(),
        }
    }

    /// A device of `config` on the disk at `dir`, signed in as `user` on
    /// the session `session` holding `roles`, dialling this hall.
    pub fn device(&self, config: Config, dir: &str, user: String, session: String, roles: Vec<String>) -> Result<Arc<Peer>, Refusal> {
        let token = format!("{user}/{session}");
        let me = Ctx::who(user, session, roles);
        held(&self.who).insert(token.clone(), me.clone());
        let p = Peer::held(Handle::by_hand(crate::opened(config, dir, Some((me, token)))?));
        let who = self.who.clone();
        p.connect_with("hall", self.hub.dialer(Arc::new(move |t: &str| held(&who).get(t).cloned())));
        held(&self.devices).push(Arc::downgrade(&p));
        self.carry();
        Ok(p)
    }

    /// Everything every device said answered and taken in, until nobody
    /// has anything more to say.
    pub fn carry(&self) {
        let devices: Vec<Arc<Peer>> = held(&self.devices).iter().filter_map(Weak::upgrade).collect();
        let mut still = 0;
        for _ in 0..64 {
            // Whatever the hub had, answered: its thread runs this after.
            let _ = self.hub.with(|_| ());
            let moved = devices.iter().fold(false, |m, p| p.pump() | m);
            still = if moved { 0 } else { still + 1 };
            if still == 2 {
                return;
            }
        }
    }
}
